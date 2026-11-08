use base64::Engine as _;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, net::TcpListener, path::PathBuf, process::{Child, Command, Stdio}, sync::Mutex};
use tauri::State;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Download {
    gid: String,
    source: String,
    #[serde(default)]
    is_torrent: bool,
    name: String,
    status: String,
    completed_length: u64,
    total_length: u64,
    download_speed: u64,
    upload_speed: u64,
    path: String,
    error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    connections: u8,
    concurrent: u8,
    speed_limit_kib: u64,
    #[serde(default)]
    seed_minutes: u16,
    download_dir: String,
}

impl Settings {
    fn defaults() -> Self {
        let folder = home().join("Downloads");
        Self { connections: 8, concurrent: 3, speed_limit_kib: 0, seed_minutes: 0, download_dir: folder.to_string_lossy().into_owned() }
    }
    fn validate(&self) -> Result<(), String> {
        if !(1..=16).contains(&self.connections) { return Err("Connections must be 1–16".into()); }
        if !(1..=10).contains(&self.concurrent) { return Err("Concurrent downloads must be 1–10".into()); }
        if self.download_dir.trim().is_empty() { return Err("Download folder cannot be empty".into()); }
        if self.speed_limit_kib > 1_048_576 { return Err("Speed limit must be no more than 1048576 KiB/s".into()); }
        Ok(())
    }
}

struct Engine {
    client: reqwest::Client,
    endpoint: String,
    token: String,
    child: Mutex<Child>,
    downloads: Mutex<BTreeMap<String, Download>>,
    settings: Mutex<Settings>,
    data_dir: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn load_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    fs::read(path).ok().and_then(|raw| serde_json::from_slice(&raw).ok())
}

fn save_json<T: Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

impl Engine {
    fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let data_dir = home().join(".local").join("share").join("download-manager");
        fs::create_dir_all(&data_dir)?;
        let settings: Settings = load_json(&data_dir.join("settings.json")).unwrap_or_else(Settings::defaults);
        settings.validate()?;
        fs::create_dir_all(&settings.download_dir)?;
        let mut history: BTreeMap<String, Download> = load_json(&data_dir.join("downloads.json")).unwrap_or_default();
        // A process restart may invalidate an old aria2 GID. Avoid showing stale speed.
        for download in history.values_mut() {
            download.download_speed = 0;
            download.upload_speed = 0;
            if matches!(download.status.as_str(), "active" | "waiting") {
                download.status = "interrupted".into();
            }
        }
        let session = data_dir.join("aria2.session");
        if !session.exists() { fs::write(&session, "")?; }
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let token = uuid::Uuid::new_v4().to_string();
        // Construct fallible dependencies before spawning aria2. Otherwise a
        // TLS/client initialization error could leave an unmanaged child alive.
        let client = reqwest::Client::builder().no_proxy().build()?;
        let child = Command::new("aria2c")
            .arg("--no-conf=true")
            .arg("--enable-rpc=true")
            .arg("--rpc-listen-all=false")
            .arg(format!("--rpc-listen-port={port}"))
            .arg(format!("--rpc-secret={token}"))
            .arg("--rpc-allow-origin-all=false")
            .arg("--continue=true")
            .arg("--always-resume=false")
            .arg("--follow-torrent=true")
            .arg("--follow-metalink=true")
            .arg("--save-session-interval=10")
            .arg(format!("--input-file={}", session.display()))
            .arg(format!("--save-session={}", session.display()))
            .arg(format!("--dir={}", settings.download_dir))
            .arg(format!("--split={}", settings.connections))
            .arg(format!("--max-connection-per-server={}", settings.connections))
            .arg(format!("--max-concurrent-downloads={}", settings.concurrent))
            .arg(format!("--max-overall-download-limit={}K", settings.speed_limit_kib))
            .arg("--bt-save-metadata=true")
            .arg("--seed-time=0")
            .arg("--console-log-level=warn")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|error| std::io::Error::new(error.kind(),
                format!("Could not start aria2c ({error}). Install it with: sudo apt install aria2")))?;
        Ok(Self {
            client,
            endpoint: format!("http://127.0.0.1:{port}/jsonrpc"),
            token, child: Mutex::new(child), downloads: Mutex::new(history),
            settings: Mutex::new(settings), data_dir,
        })
    }

    async fn rpc(&self, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let mut params = vec![json!(format!("token:{}", self.token))];
        params.extend(args);
        // A small startup grace period also covers the first call after launching aria2c.
        for attempt in 0..10 {
            let result = self.client.post(&self.endpoint)
                .json(&json!({"jsonrpc":"2.0","id":"download-manager","method":format!("aria2.{method}"),"params":params}))
                .timeout(std::time::Duration::from_secs(3))
                .send().await;
            match result {
                Ok(response) => {
                    let response: Value = response.json().await.map_err(|e| e.to_string())?;
                    if let Some(error) = response.get("error") {
                        return Err(error.get("message").and_then(Value::as_str).unwrap_or("aria2 RPC error").to_string());
                    }
                    return response.get("result").cloned().ok_or_else(|| "Missing aria2 RPC result".into());
                }
                Err(error) if error.is_connect() && attempt < 9 => {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                Err(error) => return Err(format!("aria2 RPC unavailable: {error}")),
            }
        }
        Err("aria2 RPC unavailable".into())
    }

    fn remember(&self, item: Download) -> Result<(), String> {
        let mut entries = self.downloads.lock().map_err(|e| e.to_string())?;
        entries.insert(item.gid.clone(), item);
        save_json(&self.data_dir.join("downloads.json"), &*entries)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            // On Linux/macOS, SIGTERM lets aria2 persist its session on shutdown.
            // After a short grace period, force-kill to avoid hanging app exit.
            #[cfg(unix)]
            {
                if let Ok(Some(_)) = child.try_wait() { return; }
                let _ = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
                for _ in 0..20 {
                    if let Ok(Some(_)) = child.try_wait() { return; }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn validate_source(input: &str) -> Result<(), String> {
    if input.len() > 8192 { return Err("Download source is too long".into()); }
    if input.is_empty() { return Err("Paste a download URL, magnet link or a .torrent/.meta4 path".into()); }
    if input.chars().any(|ch| ch == '\n' || ch == '\r' || ch == '\0') {
        return Err("Download source contains invalid characters".into());
    }
    if input.starts_with("magnet:?") {
        if !input.split('&').any(|part| {
            part.strip_prefix("magnet:?xt=urn:btih:").is_some_and(|hash| !hash.is_empty())
                || part.strip_prefix("xt=urn:btih:").is_some_and(|hash| !hash.is_empty())
                || part.strip_prefix("magnet:?xt=urn:btmh:").is_some_and(|hash| !hash.is_empty())
                || part.strip_prefix("xt=urn:btmh:").is_some_and(|hash| !hash.is_empty())
        }) {
            return Err("Magnet link has no recognized BitTorrent info hash".into());
        }
        return Ok(());
    }
    if input.contains("://") {
        let parsed = url::Url::parse(input).map_err(|_| "Invalid download URL")?;
        if !matches!(parsed.scheme(), "http" | "https" | "ftp" | "file") {
            return Err("Only HTTP, HTTPS, FTP, magnet links and local torrent/metalink files are supported".into());
        }
        if parsed.scheme() == "file" {
            let path = parsed.to_file_path().map_err(|_| "Invalid local file URL")?;
            if !is_metadata_file(&path) { return Err("Only torrent and metalink files can be imported locally".into()); }
        } else if parsed.host_str().is_none() {
            return Err("Download URL has no host".into());
        }
    } else if !is_metadata_file(std::path::Path::new(input)) {
        return Err("Select a .torrent, .meta4 or .metalink file, or enter a supported URL".into());
    }
    Ok(())
}

fn is_historical_status(status: &str) -> bool {
    matches!(status, "complete" | "removed" | "error")
}

fn is_missing_gid_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("gid") && normalized.contains("not found")
}

fn is_metadata_file(path: &std::path::Path) -> bool {
    matches!(path.extension().and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase()).as_deref(), Some("torrent" | "meta4" | "metalink"))
}

async fn enqueue(engine: &Engine, source: String) -> Result<Download, String> {
    let input = source.trim();
    validate_source(input)?;
    let settings = engine.settings.lock().map_err(|e| e.to_string())?.clone();
    let mut opts = json!({
        "dir": settings.download_dir,
        "split": settings.connections.to_string(),
        "max-connection-per-server": settings.connections.to_string(),
        "continue": "true",
    });
    let gid = if input.starts_with("magnet:?") {
        opts["seed-time"] = json!(settings.seed_minutes.to_string());
        engine.rpc("addUri", vec![json!([input]), opts]).await?
    } else if input.starts_with("http://") || input.starts_with("https://")
        || input.starts_with("ftp://") {
        let url_path = input.split('?').next().unwrap_or(input);
        if url_path.to_ascii_lowercase().ends_with(".torrent") {
            opts["seed-time"] = json!(settings.seed_minutes.to_string());
        }
        engine.rpc("addUri", vec![json!([input]), opts]).await?
    } else {
        let path = if input.starts_with("file://") {
            url::Url::parse(input).map_err(|e| e.to_string())?
                .to_file_path().map_err(|_| "Invalid file URL")?
        } else { PathBuf::from(input) };
        if !path.is_file() { return Err("Torrent or metalink file was not found".into()); }
        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > 20 * 1024 * 1024 { return Err("Torrent/metalink metadata is larger than 20 MB".into()); }
        let content = fs::read(&path).map_err(|e| e.to_string())?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(content);
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "torrent" => {
                opts["seed-time"] = json!(settings.seed_minutes.to_string());
                engine.rpc("addTorrent", vec![json!(b64), json!([]), opts]).await?
            },
            "meta4" | "metalink" => engine.rpc("addMetalink", vec![json!(b64), opts]).await?,
            _ => return Err("Select a .torrent, .meta4 or .metalink file".into()),
        }
    };
    let gid = if let Some(s) = gid.as_str() { s.to_string() }
        else if let Some(s) = gid.as_array().and_then(|x| x.first()).and_then(Value::as_str) { s.to_string() }
        else { return Err("aria2 returned an unexpected download ID".into()); };
    let name = input.rsplit('/').next().unwrap_or(input).chars().take(120).collect::<String>();
    let item = Download { gid, source: input.to_string(), is_torrent: input.starts_with("magnet:") || input.to_ascii_lowercase().contains(".torrent"), name, status: "waiting".into(), completed_length: 0,
        total_length: 0, download_speed: 0, upload_speed: 0, path: String::new(), error: None };
    if let Err(error) = engine.remember(item.clone()) {
        // The RPC transfer is already running. Returning an error here could
        // tempt users to retry and accidentally enqueue a duplicate download.
        eprintln!("Download started, but its local history could not be saved: {error}");
    }
    // Persist queue state immediately. If disk saving fails, do not report an
    // enqueue failure: aria2 has already accepted the transfer.
    let _ = engine.rpc("saveSession", vec![]).await;
    Ok(item)
}

#[tauri::command]
async fn add_download(engine: State<'_, Engine>, source: String) -> Result<Download, String> {
    enqueue(&engine, source).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResult {
    added: usize,
    errors: Vec<String>,
}

#[tauri::command]
async fn add_batch(engine: State<'_, Engine>, sources: Vec<String>) -> Result<BatchResult, String> {
    if sources.is_empty() || sources.len() > 50 {
        return Err("A batch must contain between 1 and 50 links".into());
    }
    let mut result = BatchResult { added: 0, errors: Vec::new() };
    for (index, source) in sources.into_iter().enumerate() {
        match enqueue(&engine, source).await {
            Ok(_) => result.added += 1,
            Err(error) => result.errors.push(format!("Line {}: {}", index + 1, error)),
        }
    }
    Ok(result)
}

#[tauri::command]
async fn list_downloads(engine: State<'_, Engine>) -> Result<Vec<Download>, String> {
    let snapshot: Vec<Download> = engine.downloads.lock().map_err(|e| e.to_string())?.values().cloned().collect();
    let mut updated = Vec::with_capacity(snapshot.len());
    let mut discovered = Vec::new();
    // Poll transfers concurrently rather than serially: a slow RPC response should not
    // delay every subsequent transfer in the queue.
    // Bounded, ordered concurrency keeps hundreds of downloads from flooding the
    // local RPC server while preserving a stable row order.
    let statuses: Vec<_> = stream::iter(snapshot.into_iter().map(|item| async {
        // Completed/removed/error entries are historical records, not live jobs.
        // Avoid querying aria2 for every such record every 1.5 seconds.
        let status = if is_historical_status(&item.status) {
            None
        } else {
            Some(engine.rpc("tellStatus", vec![json!(item.gid)]).await)
        };
        (item, status)
    })).buffered(8).collect().await;
    for (mut item, result) in statuses {
        if let Some(Ok(status)) = result.as_ref() {
            // aria2 creates a follow-up GID after retrieving magnet/torrent metadata.
            // Track it separately so the actual payload remains visible and controllable.
            if let Some(children) = status.get("followedBy").and_then(Value::as_array) {
                for next in children.iter().filter_map(Value::as_str) {
                    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(next)
                        && !discovered.iter().any(|d: &Download| d.gid == next) {
                        let mut child = item.clone();
                        child.gid = next.to_string();
                        child.status = "waiting".into();
                        child.completed_length = 0;
                        child.total_length = 0;
                        child.download_speed = 0;
                        child.upload_speed = 0;
                        child.path.clear();
                        child.error = None;
                        discovered.push(child);
                    }
                }
            }
            let get_num = |key: &str| status.get(key).and_then(Value::as_str).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            item.status = status.get("status").and_then(Value::as_str).unwrap_or("unknown").to_string();
            if status.get("bittorrent").is_some() { item.is_torrent = true; }
            item.completed_length = get_num("completedLength");
            item.total_length = get_num("totalLength");
            item.download_speed = get_num("downloadSpeed");
            item.upload_speed = get_num("uploadSpeed");
            if let Some(name) = status.get("bittorrent").and_then(|v| v.get("info")).and_then(|v| v.get("name")).and_then(Value::as_str) {
                item.name = name.to_string();
            } else if let Some(path) = status.get("files").and_then(Value::as_array).and_then(|a| a.first())
                .and_then(|v| v.get("path")).and_then(Value::as_str) {
                if !path.is_empty() {
                    item.path = path.to_string();
                    if let Some(name) = std::path::Path::new(path).file_name().and_then(|s| s.to_str()) { item.name = name.to_string(); }
                }
            }
            if item.status == "error" {
                item.error = status.get("errorMessage").and_then(Value::as_str).map(str::to_string);
            }
        } else if result.as_ref().and_then(|result| result.as_ref().err())
            .is_some_and(|error| is_missing_gid_error(error)) {
            if matches!(item.status.as_str(), "active" | "waiting" | "paused") {
                item.status = "interrupted".into();
            }
            item.download_speed = 0;
            item.upload_speed = 0;
        }
        updated.push(item);
    }
    updated.extend(discovered);
    updated.reverse();
    // Store completed state and filenames for display after restarting the application.
    if let Ok(mut entries) = engine.downloads.lock() {
        let mut changed = false;
        for item in &updated {
            if entries.get(&item.gid) != Some(item) {
                entries.insert(item.gid.clone(), item.clone());
                changed = true;
            }
        }
        if changed { let _ = save_json(&engine.data_dir.join("downloads.json"), &*entries); }
    }
    Ok(updated)
}

#[tauri::command]
async fn retry_download(engine: State<'_, Engine>, gid: String) -> Result<Download, String> {
    let original = engine.downloads.lock().map_err(|e| e.to_string())?
        .get(&gid).cloned().ok_or("Unknown download")?;
    if !matches!(original.status.as_str(), "error" | "removed" | "interrupted") {
        return Err("This download cannot be retried. Use Resume for paused transfers.".into());
    }
    // A restored aria2 session may already be running the same GID. Never start a
    // second transfer if the engine has recovered the original download.
    match engine.rpc("tellStatus", vec![json!(gid.clone())]).await {
        Ok(status) => {
            match status.get("status").and_then(Value::as_str) {
                Some("active" | "waiting" | "paused") =>
                    return Err("The original transfer is still in the engine; resume it instead".into()),
                Some("complete") =>
                    return Err("The original download already completed; refresh the list".into()),
                Some("error" | "removed") => {}
                _ => return Err("Cannot safely retry: aria2 reported an unknown transfer state".into()),
            }
        }
        Err(error) if is_missing_gid_error(&error) => {
            // The old identifier is absent; it is safe to submit a fresh job.
        }
        Err(error) => {
            // A timeout or disconnected RPC must never turn into a duplicate transfer.
            return Err(format!("Cannot check whether this transfer recovered: {error}"));
        }
    }
    let replacement = enqueue(&engine, original.source.clone()).await?;
    let mut entries = engine.downloads.lock().map_err(|e| e.to_string())?;
    entries.remove(&gid);
    if let Err(error) = save_json(&engine.data_dir.join("downloads.json"), &*entries) {
        eprintln!("Retry succeeded but history cleanup could not be saved: {error}");
    }
    Ok(replacement)
}

#[tauri::command]
async fn control_download(engine: State<'_, Engine>, gid: String, action: String) -> Result<(), String> {
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let method = match action.as_str() {
        "pause" => "pause",
        "resume" => "unpause",
        "cancel" => "forceRemove",
        _ => return Err("Unsupported action".into()),
    };
    engine.rpc(method, vec![json!(gid.clone())]).await?;
    // aria2 can restore paused and waiting transfers from a saved session.
    let _ = engine.rpc("saveSession", vec![]).await;
    if action == "cancel" {
        let mut entries = engine.downloads.lock().map_err(|e| e.to_string())?;
        if let Some(entry) = entries.get_mut(&gid) { entry.status = "removed".into(); }
        save_json(&engine.data_dir.join("downloads.json"), &*entries)?;
    }
    Ok(())
}


#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TorrentFile {
    index: u32,
    path: String,
    length: u64,
    completed_length: u64,
    selected: bool,
}

fn parse_torrent_files(value: &Value) -> Result<Vec<TorrentFile>, String> {
    let entries = value.as_array().ok_or("aria2 did not return a file list")?;
    entries.iter().map(|entry| {
        let numeric = |field: &str| -> Result<u64, String> {
            entry.get(field).and_then(Value::as_str)
                .ok_or_else(|| format!("Missing {field}"))
                .and_then(|s| s.parse::<u64>().map_err(|_| format!("Invalid {field}")))
        };
        let index = u32::try_from(numeric("index")?).map_err(|_| "Invalid file index")?;
        if index == 0 { return Err("Invalid zero-based file index".into()); }
        Ok(TorrentFile {
            index,
            path: entry.get("path").and_then(Value::as_str).unwrap_or("").to_string(),
            length: numeric("length")?,
            completed_length: numeric("completedLength")?,
            selected: entry.get("selected").and_then(Value::as_str) == Some("true"),
        })
    }).collect()
}

fn validate_file_selection(selected: &[u32], files: &[TorrentFile]) -> Result<String, String> {
    if selected.is_empty() { return Err("Choose at least one file".into()); }
    let mut values = selected.to_vec();
    values.sort_unstable();
    values.dedup();
    if values.len() != selected.len() { return Err("Duplicate file indexes".into()); }
    if values.iter().any(|index| !files.iter().any(|file| file.index == *index)) {
        return Err("Selected file index does not belong to this download".into());
    }
    Ok(values.iter().map(ToString::to_string).collect::<Vec<_>>().join(","))
}

#[tauri::command]
async fn get_torrent_files(engine: State<'_, Engine>, gid: String) -> Result<Vec<TorrentFile>, String> {
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let status = engine.rpc("tellStatus", vec![json!(gid.clone())]).await?;
    if status.get("bittorrent").is_none() {
        return Err("Waiting for torrent metadata. Try again after metadata is available.".into());
    }
    let response = engine.rpc("getFiles", vec![json!(gid)]).await?;
    parse_torrent_files(&response)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TorrentInfo {
    peers: usize,
    trackers: Vec<String>,
}

#[tauri::command]
async fn get_torrent_info(engine: State<'_, Engine>, gid: String) -> Result<TorrentInfo, String> {
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let status = engine.rpc("tellStatus", vec![json!(gid.clone())]).await?;
    let bittorrent = status.get("bittorrent").ok_or("Torrent metadata is not available yet")?;
    let trackers = bittorrent.get("announceList").and_then(Value::as_array)
        .map(|tiers| tiers.iter().filter_map(Value::as_array)
            .flat_map(|tier| tier.iter().filter_map(Value::as_str))
            .map(str::to_string).take(64).collect::<Vec<_>>())
        .unwrap_or_default();
    let peers = engine.rpc("getPeers", vec![json!(gid)]).await.ok()
        .and_then(|v| v.as_array().map(Vec::len)).unwrap_or(0);
    Ok(TorrentInfo { peers, trackers })
}

#[tauri::command]
async fn select_torrent_files(engine: State<'_, Engine>, gid: String, indexes: Vec<u32>) -> Result<(), String> {
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let status = engine.rpc("tellStatus", vec![json!(gid.clone())]).await?;
    if status.get("bittorrent").is_none() {
        return Err("Torrent metadata is not available yet".into());
    }
    let response = engine.rpc("getFiles", vec![json!(gid.clone())]).await?;
    let files = parse_torrent_files(&response)?;
    let selection = validate_file_selection(&indexes, &files)?;
    engine.rpc("changeOption", vec![json!(gid), json!({ "select-file": selection })]).await?;
    Ok(())
}

#[cfg(test)]
mod torrent_selection_tests {
    use super::*;
    #[test]
    fn parses_file_data_and_validates_selection() {
        let files = parse_torrent_files(&json!([
            { "index":"1", "path":"/tmp/one", "length":"100", "completedLength":"25", "selected":"true" },
            { "index":"2", "path":"/tmp/two", "length":"200", "completedLength":"0", "selected":"false" }
        ])).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].completed_length, 25);
        assert!(files[0].selected);
        assert_eq!(validate_file_selection(&[2, 1], &files).unwrap(), "1,2");
        assert!(validate_file_selection(&[], &files).is_err());
        assert!(validate_file_selection(&[3], &files).is_err());
        assert!(validate_file_selection(&[1, 1], &files).is_err());
    }
}

#[tauri::command]
async fn move_queued_download(engine: State<'_, Engine>, gid: String, direction: String) -> Result<(), String> {
    let position = match direction.as_str() {
        "top" => 0,
        "bottom" => 0,
        _ => return Err("Unknown queue action".into()),
    };
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let how = if direction == "top" { "POS_SET" } else { "POS_END" };
    engine.rpc("changePosition", vec![json!(gid), json!(position), json!(how)]).await?;
    let _ = engine.rpc("saveSession", vec![]).await;
    Ok(())
}

#[tauri::command]
fn get_settings(engine: State<'_, Engine>) -> Result<Settings, String> {
    Ok(engine.settings.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
async fn save_settings(engine: State<'_, Engine>, settings: Settings) -> Result<(), String> {
    settings.validate()?;
    fs::create_dir_all(&settings.download_dir).map_err(|e| e.to_string())?;
    engine.rpc("changeGlobalOption", vec![json!({
        "max-concurrent-downloads": settings.concurrent.to_string(),
        "max-overall-download-limit": format!("{}K", settings.speed_limit_kib),
        "dir": settings.download_dir
    })]).await?;
    {
        let mut existing = engine.settings.lock().map_err(|e| e.to_string())?;
        *existing = settings.clone();
    }
    save_json(&engine.data_dir.join("settings.json"), &settings)
}

#[tauri::command]
fn forget_download(engine: State<'_, Engine>, gid: String) -> Result<(), String> {
    let mut entries = engine.downloads.lock().map_err(|e| e.to_string())?;
    let item = entries.get(&gid).ok_or("Unknown download")?;
    if matches!(item.status.as_str(), "active" | "waiting" | "paused") {
        return Err("Stop the download before removing it from history".into());
    }
    entries.remove(&gid);
    save_json(&engine.data_dir.join("downloads.json"), &*entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_connection_limits() {
        let mut settings = Settings::defaults();
        assert!(settings.validate().is_ok());
        settings.connections = 0;
        assert!(settings.validate().is_err());
        settings.connections = 16;
        assert!(settings.validate().is_ok());
        settings.concurrent = 11;
        assert!(settings.validate().is_err());
        settings.concurrent = 3;
        settings.speed_limit_kib = 1_048_577;
        assert!(settings.validate().is_err());
    }
    #[test]
    fn does_not_poll_historical_downloads() {
        assert!(is_historical_status("complete"));
        assert!(is_historical_status("removed"));
        assert!(is_historical_status("error"));
        assert!(!is_historical_status("active"));
        assert!(!is_historical_status("waiting"));
        assert!(!is_historical_status("paused"));
        assert!(!is_historical_status("interrupted"));
    }
    #[test]
    fn missing_gid_detection_only_accepts_confirmed_not_found() {
        assert!(is_missing_gid_error("GID abcd is not found"));
        assert!(is_missing_gid_error("gid abc NOT FOUND"));
        assert!(!is_missing_gid_error("Connection refused"));
        assert!(!is_missing_gid_error("Request timed out"));
    }
    #[test]
    fn rejects_invalid_sources_without_starting_engine() {
        assert!(validate_source("").is_err());
        assert!(validate_source("magnet:?dn=missing").is_err());
        assert!(validate_source("https://example.org/one.zip\ntwo.zip").is_err());
        assert!(validate_source("https://example.org/file.zip").is_ok());
        assert!(validate_source("magnet:?xt=urn:btih:abcdef123").is_ok());
        assert!(validate_source("https://example.org/file.zip").is_ok());
        assert!(validate_source("sftp://example.org/file.zip").is_err());
        assert!(validate_source("not-a-url.zip").is_err());
        assert!(validate_source("/tmp/test.TORRENT").is_ok());
        assert!(validate_source("file:///tmp/test.torrent").is_ok());
    }
    #[test]
    fn json_round_trip_keeps_download_identifiers() {
        let item = Download { gid: "abc123".into(), source: "https://example.org/a.zip".into(), is_torrent: false,
            name: "a.zip".into(), status: "complete".into(), completed_length: 30,
            total_length: 30, download_speed: 0, upload_speed: 0,
            path: "/tmp/a.zip".into(), error: None };
        let encoded = serde_json::to_string(&item).unwrap();
        let decoded: Download = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.gid, "abc123");
        assert_eq!(decoded.completed_length, 30);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            app.manage(Engine::start()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_download, add_batch, retry_download, list_downloads, control_download, move_queued_download, get_torrent_files, get_torrent_info, select_torrent_files, forget_download, get_settings, save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Download Manager");
}
