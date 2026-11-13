use base64::Engine as _;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, io::{BufRead, BufReader, Write}, net::TcpListener, path::PathBuf, process::{Child, Command, Stdio}, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, time::{Duration, Instant}};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    #[serde(default = "default_min_split_size_mib")]
    min_split_size_mib: u8,
    concurrent: u8,
    speed_limit_kib: u64,
    #[serde(default)]
    seed_minutes: u16,
    download_dir: String,
    #[serde(default = "default_refresh_seconds")]
    refresh_seconds: u8,
    #[serde(default = "default_low_memory_mode")]
    low_memory_mode: bool,
}

fn aria2_server_connections(requested: u8) -> u8 { requested.min(16) }
fn default_min_split_size_mib() -> u8 { 4 }
fn default_refresh_seconds() -> u8 { 3 }
fn default_low_memory_mode() -> bool { true }

impl Settings {
    fn defaults() -> Self {
        let folder = home().join("Downloads");
        Self { connections: 8, min_split_size_mib: default_min_split_size_mib(), concurrent: 3, speed_limit_kib: 0, seed_minutes: 0, download_dir: folder.to_string_lossy().into_owned(),
            refresh_seconds: default_refresh_seconds(), low_memory_mode: default_low_memory_mode() }
    }
    fn validate(&self) -> Result<(), String> {
        if !(1..=32).contains(&self.connections) { return Err("Connections must be 1–32".into()); }
        if !(1..=64).contains(&self.min_split_size_mib) { return Err("Minimum segment size must be 1–64 MiB".into()); }
        if !(1..=10).contains(&self.concurrent) { return Err("Concurrent downloads must be 1–10".into()); }
        if self.download_dir.trim().is_empty() { return Err("Download folder cannot be empty".into()); }
        if !(1..=30).contains(&self.refresh_seconds) { return Err("Refresh interval must be 1–30 seconds".into()); }
        if self.speed_limit_kib > 1_048_576 { return Err("Speed limit must be no more than 1048576 KiB/s".into()); }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    state: String,             // idle, armed, running, power_pending, completed, cancelled, error
    start_at_unix: u64,         // local time converted by UI to Unix seconds
    concurrent: u8,
    after_complete: String,    // none, shutdown, sleep
    gids: Vec<String>,         // exact transfers owned by this schedule
    power_at_unix: u64,        // 60-second grace period for cancellation
    message: String,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            state: "idle".into(), start_at_unix: 0, concurrent: 3,
            after_complete: "none".into(), gids: Vec::new(),
            power_at_unix: 0, message: String::new(),
        }
    }
}

fn recover_schedule_after_restart(mut schedule: Schedule) -> Schedule {
    if matches!(schedule.state.as_str(), "armed" | "running" | "power_pending") {
        schedule.state = "cancelled".into();
        schedule.power_at_unix = 0;
        schedule.message = "App restarted; schedule cancelled for safety. Restored aria2 downloads may resume immediately. Re-arm the schedule if needed.".into();
    }
    schedule
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn validate_schedule_request(start: u64, concurrent: u8, action: &str, now: u64) -> Result<(), String> {
    if start <= now || start > now.saturating_add(366 * 24 * 60 * 60) {
        return Err("Choose a future start time within one year".into());
    }
    if !(1..=10).contains(&concurrent) { return Err("Concurrent transfers must be 1–10".into()); }
    if !matches!(action, "none" | "shutdown" | "sleep") {
        return Err("Invalid completion action".into());
    }
    Ok(())
}

fn scheduled_terminal_state(status: &str) -> bool {
    matches!(status, "complete" | "error" | "removed")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaJob {
    id: String,
    title: String,
    status: String,
    format: String,
    #[serde(default)]
    playlist: bool,
    #[serde(default)]
    playlist_items: Vec<u32>,
    error: Option<String>,
    #[serde(default)]
    progress: Option<f32>,
    #[serde(default)]
    speed: String,
    #[serde(default)]
    eta: String,
    #[serde(default)]
    current_item: String,
    #[serde(default)]
    playlist_index: Option<u32>,
    #[serde(default)]
    overall_progress: Option<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaTools {
    yt_dlp: Option<String>,
    ffmpeg: Option<String>,
}

fn optional_tool_version(exe: &str, option: &str) -> Option<String> {
    let output = Command::new(exe).arg(option)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .output().ok()?;
    if !output.status.success() { return None; }
    String::from_utf8_lossy(&output.stdout)
        .lines().next().map(|line| line.chars().take(120).collect())
}

#[tauri::command]
async fn media_tools_status() -> Result<MediaTools, String> {
    tokio::task::spawn_blocking(|| MediaTools {
        yt_dlp: yt_dlp_command().arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .output().ok().filter(|output| output.status.success())
            .and_then(|output| String::from_utf8_lossy(&output.stdout).lines().next().map(str::to_owned)),
        ffmpeg: optional_tool_version("ffmpeg", "-version"),
    }).await.map_err(|error| error.to_string())
}

fn restore_media_jobs(mut jobs: BTreeMap<String, MediaJob>) -> BTreeMap<String, MediaJob> {
    for job in jobs.values_mut() {
        if matches!(job.status.as_str(), "downloading" | "paused") {
            job.status = "interrupted".into();
            job.error = Some("Download Manager restarted. Retry to continue using yt-dlp's partial files.".into());
            job.speed.clear();
            job.eta.clear();
        }
    }
    jobs
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaPreview {
    title: String,
    duration_seconds: Option<f64>,
    uploader: String,
    webpage_url: String,
    yt_dlp_version: String,
    formats: Vec<MediaFormat>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistPreview {
    title: String,
    entries: Vec<PlaylistEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistEntry {
    index: u32,
    title: String,
    duration_seconds: Option<f64>,
}

fn playlist_entries(info: &Value) -> Vec<PlaylistEntry> {
    info.get("entries").and_then(Value::as_array).into_iter().flatten()
        .take(50).enumerate().filter_map(|(offset, entry)| {
            if entry.is_null() { return None; }
            Some(PlaylistEntry {
                index: (offset + 1) as u32,
                title: entry.get("title").and_then(Value::as_str).unwrap_or("Untitled video")
                    .chars().filter(|ch| !ch.is_control()).take(180).collect(),
                duration_seconds: entry.get("duration").and_then(Value::as_f64),
            })
        }).collect()
}

fn validate_playlist_items(items: &[u32], playlist: bool) -> Result<(), String> {
    if !playlist && !items.is_empty() { return Err("Playlist items require playlist mode".into()); }
    if items.len() > 50 || items.iter().any(|index| !(1..=50).contains(index)) {
        return Err("Playlist item indexes must be between 1 and 50".into());
    }
    let unique: std::collections::BTreeSet<u32> = items.iter().copied().collect();
    if unique.len() != items.len() { return Err("Duplicate playlist indexes are not allowed".into()); }
    Ok(())
}

// A user-managed pipx install is often newer than Ubuntu's apt package.
// Prefer it explicitly because desktop launchers may not include ~/.local/bin in PATH.
fn yt_dlp_command() -> Command {
    let user_binary = home().join(".local/bin/yt-dlp");
    if user_binary.is_file() {
        Command::new(user_binary)
    } else {
        Command::new("yt-dlp")
    }
}

#[tauri::command]
async fn inspect_playlist(source: String) -> Result<PlaylistPreview, String> {
    media_http_url(&source)?;
    tokio::task::spawn_blocking(move || {
        let output = yt_dlp_command()
            .args(["--ignore-config", "--js-runtimes", "node", "--flat-playlist", "--playlist-end", "50",
                "--dump-single-json", "--no-warnings", "--no-call-home"])
            .arg("--").arg(&source)
            .stdin(Stdio::null()).output()
            .map_err(|_| "yt-dlp missing or could not inspect the playlist".to_string())?;
        if !output.status.success() {
            return Err(media_inspect_error(&output.stderr, &source, "Playlist inspection failed"));
        }
        if output.stdout.len() > 2_000_000 { return Err("Playlist metadata is too large".into()); }
        let info: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "yt-dlp returned invalid playlist metadata")?;
        Ok(PlaylistPreview {
            title: info.get("title").and_then(Value::as_str).unwrap_or("Playlist")
                .chars().filter(|ch| !ch.is_control()).take(180).collect(),
            entries: playlist_entries(&info),
        })
    }).await.map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaFormat {
    id: String,
    extension: String,
    resolution: String,
    note: String,
    filesize: Option<u64>,
}

fn media_formats(info: &Value) -> Vec<MediaFormat> {
    let Some(formats) = info.get("formats").and_then(Value::as_array) else { return vec![] };
    formats.iter().filter_map(|entry| {
        let id = entry.get("format_id")?.as_str()?;
        if id.is_empty() || id.len() > 64 || !id.starts_with(|c: char| c.is_ascii_alphanumeric()) ||
            !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
            return None;
        }
        let clean = |field: &str| -> String {
            entry.get(field).and_then(Value::as_str).unwrap_or("")
                .chars().filter(|c| !c.is_control()).take(70).collect()
        };
        let resolution = clean("resolution");
        let height = entry.get("height").and_then(Value::as_u64);
        let resolution = if resolution.is_empty() {
            height.map(|v| format!("{v}p")).unwrap_or_else(|| "Audio / unknown".into())
        } else { resolution };
        Some(MediaFormat {
            id: id.to_string(), extension: clean("ext"),
            resolution, note: clean("format_note"),
            filesize: entry.get("filesize").or_else(|| entry.get("filesize_approx"))
                .and_then(Value::as_u64),
        })
    }).take(80).collect()
}

fn media_http_url(source: &str) -> Result<(), String> {
    if source.len() > 4096 || source.chars().any(|ch| ch.is_control()) {
        return Err("Invalid media URL".into());
    }
    let parsed = url::Url::parse(source).map_err(|_| "Enter a valid media page URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Only HTTP and HTTPS media page URLs are supported".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Embedded URL credentials are not accepted".into());
    }
    Ok(())
}

fn media_format_args(format: &str) -> Result<Vec<String>, String> {
    let known: Option<Vec<&str>> = match format {
        "best" => Some(vec!["-f", "bv*+ba/b", "--merge-output-format", "mkv"]),
        "mp4" => Some(vec!["-f", "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/b", "--merge-output-format", "mp4"]),
        "audio" => Some(vec!["-x", "--audio-format", "mp3"]),
        "720p" => Some(vec!["-f", "bv*[height<=720]+ba/b[height<=720]", "--merge-output-format", "mkv"]),
        "1080p" => Some(vec!["-f", "bv*[height<=1080]+ba/b[height<=1080]", "--merge-output-format", "mkv"]),
        _ => None,
    };
    if let Some(parts) = known {
        return Ok(parts.into_iter().map(str::to_string).collect());
    }
    // Format IDs come from yt-dlp metadata. Permit only narrow identifiers;
    // never allow arbitrary selectors/command arguments from the frontend.
    if let Some(id) = format.strip_prefix("id:") {
        if !id.is_empty() && id.len() <= 64 && id.starts_with(|ch: char| ch.is_ascii_alphanumeric()) &&
            id.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
            return Ok(vec!["-f".into(), id.to_string()]);
        }
    }
    Err("Choose a supported preset or an inspected format ID".into())
}

struct Engine {
    client: reqwest::Client,
    endpoint: String,
    token: String,
    child: Mutex<Child>,
    downloads: Mutex<BTreeMap<String, Download>>,
    settings: Mutex<Settings>,
    schedule: Mutex<Schedule>,
    media_jobs: Arc<Mutex<BTreeMap<String, MediaJob>>>,
    media_cancellations: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
    media_pauses: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
    feed_subscriptions: Mutex<BTreeMap<String, FeedSubscription>>,
    data_dir: PathBuf,
    last_history_save: Mutex<Instant>,
}

/// Persist immediately for lifecycle/identity changes; ordinary byte and speed
/// updates can be checkpointed periodically without excessive disk writes.
fn requires_immediate_history_save(old: &Download, new: &Download) -> bool {
    old.status != new.status
        || old.source != new.source
        || old.name != new.name
        || old.is_torrent != new.is_torrent
        || old.path != new.path
        || old.error != new.error
}

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn load_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<Option<T>, String> {
    // Missing data means first launch; corrupted/unreadable data is never
    // silently replaced with an empty history. Recover from the last good
    // backup when possible, preserving the damaged primary for diagnosis.
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let backup = path.with_extension("bak");
            let bytes = match fs::read(&backup) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(format!("Cannot read backup {}: {error}", backup.display())),
            };
            let recovered = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Primary {} is missing and backup {} is invalid: {error}",
                    path.display(), backup.display()))?;
            eprintln!("Recovered missing {} from {}", path.display(), backup.display());
            return Ok(Some(recovered));
        },
        Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
    };
    match serde_json::from_slice(&raw) {
        Ok(data) => Ok(Some(data)),
        Err(primary_error) => {
            let backup = path.with_extension("bak");
            let bytes = fs::read(&backup).map_err(|backup_error| format!(
                "Invalid JSON in {}: {primary_error}; backup {} unavailable: {backup_error}",
                path.display(), backup.display()
            ))?;
            let restored = serde_json::from_slice(&bytes).map_err(|backup_error| format!(
                "Invalid JSON in {}: {primary_error}; backup {} is also invalid: {backup_error}",
                path.display(), backup.display()
            ))?;
            eprintln!("Recovered {} using backup {}; original is preserved",
                path.display(), backup.display());
            Ok(Some(restored))
        }
    }
}

fn load_validated_settings(path: &std::path::Path) -> Result<Settings, String> {
    let primary = load_json::<Settings>(path)?;
    if let Some(settings) = primary {
        match settings.validate() {
            Ok(()) => return Ok(settings),
            Err(primary_error) => {
                // A syntactically valid settings document may still contain
                // unusable values. Attempt the last known-good backup.
                let backup = path.with_extension("bak");
                let restored = load_json::<Settings>(&backup)?
                    .ok_or_else(|| format!("Invalid settings in {}: {primary_error}; backup is missing",
                        path.display()))?;
                restored.validate().map_err(|backup_error| format!(
                    "Invalid settings in {}: {primary_error}; backup {} is invalid: {backup_error}",
                    path.display(), backup.display()
                ))?;
                eprintln!("Recovered invalid settings from {}; primary is preserved", backup.display());
                return Ok(restored);
            }
        }
    }
    Ok(Settings::defaults())
}

fn save_json<T: Serialize + serde::de::DeserializeOwned>(path: &std::path::Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    // Write a complete temporary document first. Never truncate the last
    // successfully committed history/settings file on a partial write.
    let mut file = fs::OpenOptions::new().create(true).write(true).truncate(true)
        .open(&tmp).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    // Retain the previous valid snapshot before committing a replacement.
    // Never overwrite a known-good backup with a corrupt primary.
    let previous = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot back up {}: {error}", path.display())),
    };
    if let Some(current) = previous {
        // A syntactically valid JSON object might still have the wrong schema.
        // Settings also require semantic validation before replacing the backup.
        let backup_worthy = if path.file_name().and_then(|part| part.to_str()) == Some("settings.json") {
            serde_json::from_slice::<Settings>(&current)
                .is_ok_and(|settings| settings.validate().is_ok())
        } else {
            serde_json::from_slice::<T>(&current).is_ok()
        };
        if backup_worthy {
            let backup = path.with_extension("bak");
            let backup_tmp = path.with_extension("bak.tmp");
            let mut backup_file = fs::OpenOptions::new().create(true).write(true)
                .truncate(true).open(&backup_tmp).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            fs::set_permissions(&backup_tmp, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
            backup_file.write_all(&current).map_err(|e| e.to_string())?;
            backup_file.sync_all().map_err(|e| e.to_string())?;
            drop(backup_file);
            fs::rename(&backup_tmp, &backup).map_err(|e| e.to_string())?;
        }
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        // Sync the rename's directory entry for better durability after a crash.
        fs::File::open(parent).and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Reconciliation after app launch must distinguish "not found in the new
// aria2 session" from "not checked yet". Never allow duplicate retries while
// session restoration is still pending.
fn status_needs_reconciliation(status: &str) -> bool {
    matches!(status, "active" | "waiting" | "paused")
}

impl Engine {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = home().join(".local").join("share").join("download-manager");
        fs::create_dir_all(&data_dir)?;
        // URLs and torrent metadata can include private credentials; keep
        // app-managed state inaccessible to other local Unix users.
        #[cfg(unix)]
        fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o700))?;
        let settings = load_validated_settings(&data_dir.join("settings.json"))?;
        let mut schedule: Schedule = load_json(&data_dir.join("schedule.json"))?.unwrap_or_default();
        // A recovered aria2 session may begin downloading immediately, before
        // an armed start time. Cancel all in-flight schedules on app restart so
        // neither premature queue starts nor delayed power actions are implied
        // to be protected by a schedule that no longer owns the process.
        schedule = recover_schedule_after_restart(schedule);
        settings.validate()?;
        fs::create_dir_all(&settings.download_dir)?;
        let media_history: BTreeMap<String, MediaJob> =
            restore_media_jobs(load_json(&data_dir.join("media_jobs.json"))?.unwrap_or_default());
        let feed_subscriptions: BTreeMap<String, FeedSubscription> =
            load_json(&data_dir.join("feeds.json"))?.unwrap_or_default();
        let mut history: BTreeMap<String, Download> = load_json(&data_dir.join("downloads.json"))?.unwrap_or_default();
        // A process restart may invalidate an old aria2 GID. Avoid showing stale speed.
        for download in history.values_mut() {
            download.download_speed = 0;
            download.upload_speed = 0;
            if status_needs_reconciliation(&download.status) {
                // aria2 may restore this GID from its session. Do not offer a
                // duplicate Retry until a live RPC lookup determines its state.
                download.status = "recovering".into();
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
        // Keep startup failures visible without leaking them into a public log.
        // The containing app-data directory is already mode 0700.
        let stderr_path = data_dir.join("aria2.stderr.log");
        let stderr_file = fs::OpenOptions::new()
            .create(true).write(true).truncate(true).open(&stderr_path)?;
        #[cfg(unix)]
        fs::set_permissions(&stderr_path, fs::Permissions::from_mode(0o600))?;
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
            .arg(format!("--min-split-size={}M", settings.min_split_size_mib))
            .arg(format!("--max-connection-per-server={}", aria2_server_connections(settings.connections)))
            .arg(format!("--max-concurrent-downloads={}", settings.concurrent))
            .arg(format!("--max-overall-download-limit={}K", settings.speed_limit_kib))
            .arg("--bt-save-metadata=true")
            .arg("--seed-time=0")
            .arg("--console-log-level=warn")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(stderr_file))
            .spawn().map_err(|error| std::io::Error::new(error.kind(),
                format!("Could not start aria2c ({error}). Install it with: sudo apt install aria2")))?;
        Ok(Self {
            client,
            endpoint: format!("http://127.0.0.1:{port}/jsonrpc"),
            token, child: Mutex::new(child), downloads: Mutex::new(history),
            settings: Mutex::new(settings), schedule: Mutex::new(schedule), media_jobs: Arc::new(Mutex::new(media_history)), media_cancellations: Arc::new(Mutex::new(BTreeMap::new())), media_pauses: Arc::new(Mutex::new(BTreeMap::new())), feed_subscriptions: Mutex::new(feed_subscriptions), data_dir,
            last_history_save: Mutex::new(Instant::now() - Duration::from_secs(10)),
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

impl Engine {
    fn shutdown(&self) {
        // Request cancellation for every media worker this app started. Each
        // worker owns and reaps its yt-dlp child. Do not target processes by
        // executable name, as they might belong to other applications.
        if let Ok(cancellations) = self.media_cancellations.lock() {
            for flag in cancellations.values() { flag.store(true, Ordering::Release); }
        }
        // Give worker threads a bounded window to reap and checkpoint. Avoid
        // waiting forever on a misbehaving helper during desktop shutdown.
        for _ in 0..15 {
            let pending = self.media_cancellations.lock()
                .map(|entries| !entries.is_empty()).unwrap_or(false);
            if !pending { break; }
            std::thread::sleep(Duration::from_millis(200));
        }
        // Exit callbacks run even when managed Tauri state outlives the app.
        // This is idempotent: Drop may call it after RunEvent::Exit.
        if let Ok(jobs) = self.media_jobs.lock() {
            if let Err(error) = save_json(&self.data_dir.join("media_jobs.json"), &*jobs) {
                eprintln!("Unable to persist media history on exit: {error}");
            }
        }
        if let Ok(entries) = self.downloads.lock() {
            if let Err(error) = save_json(&self.data_dir.join("downloads.json"), &*entries) {
                eprintln!("Unable to save download history during shutdown: {error}");
            }
        }
        if let Ok(mut child) = self.child.lock() {
            // Only signal the child process this Engine instance spawned.
            // Never kill unrelated aria2 processes by name or port.
            if let Ok(Some(_)) = child.try_wait() { return; }
            #[cfg(unix)]
            {
                let _ = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
                for _ in 0..20 {
                    if let Ok(Some(_)) = child.try_wait() { return; }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
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
        "max-connection-per-server": aria2_server_connections(settings.connections).to_string(),
        "min-split-size": format!("{}M", settings.min_split_size_mib),
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

// Only genuinely new child GIDs may be added by a status poll. A GID
// present at poll start but absent at commit time was removed concurrently.
fn may_insert_polled_gid(
    gid: &str,
    existing_at_poll_start: &std::collections::BTreeSet<String>,
) -> bool {
    !existing_at_poll_start.contains(gid)
}

// aria2's waiting list is the source of truth for queue positions.
// Preserve the relative order of all non-waiting entries, and fall back to
// the original history order if a queue RPC fails.
fn sort_by_waiting_queue(items: &mut [Download], waiting: &Value) {
    let mut rank = BTreeMap::new();
    if let Some(queue) = waiting.as_array() {
        for (position, entry) in queue.iter().enumerate() {
            if let Some(gid) = entry.get("gid").and_then(Value::as_str) {
                rank.entry(gid.to_string()).or_insert(position);
            }
        }
    }
    if rank.is_empty() { return; }
    items.sort_by_key(|item| {
        if item.status == "waiting" {
            rank.get(&item.gid).map(|position| (0, *position))
                .unwrap_or((1, usize::MAX))
        } else {
            (1, usize::MAX)
        }
    });
}

#[tauri::command]
async fn list_downloads(engine: State<'_, Engine>) -> Result<Vec<Download>, String> {
    let snapshot: Vec<Download> = engine.downloads.lock().map_err(|e| e.to_string())?.values().cloned().collect();
    let existing_at_poll_start: std::collections::BTreeSet<String> =
        snapshot.iter().map(|item| item.gid.clone()).collect();
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
            if matches!(item.status.as_str(), "active" | "waiting" | "paused" | "recovering") {
                item.status = "interrupted".into();
            }
            item.download_speed = 0;
            item.upload_speed = 0;
        }
        updated.push(item);
    }
    updated.extend(discovered);
    updated.reverse();
    // Only one extra local RPC for the full waiting queue, never per row.
    // Queue moves become visible even if history was inserted in GID order.
    if updated.iter().any(|item| item.status == "waiting") {
        if let Ok(waiting) = engine.rpc("tellWaiting", vec![
            json!(0), json!(1000), json!(["gid"]),
        ]).await {
            sort_by_waiting_queue(&mut updated, &waiting);
        }
    }
    // Store completed state and filenames for display after restarting the application.
    if let Ok(mut entries) = engine.downloads.lock() {
        let mut changed = false;
        let mut urgent = false;
        for item in &updated {
            match entries.get(&item.gid) {
                Some(old) if old == item => {}
                Some(old) => {
                    urgent |= requires_immediate_history_save(old, item);
                    entries.insert(item.gid.clone(), item.clone());
                    changed = true;
                }
                None if may_insert_polled_gid(&item.gid, &existing_at_poll_start) => {
                    // Newly discovered child transfer (e.g. torrent metadata).
                    urgent = true;
                    entries.insert(item.gid.clone(), item.clone());
                    changed = true;
                }
                None => {
                    // A concurrent Forget/Retry removed this GID after the
                    // RPC poll began. Never resurrect it from stale results.
                }
            }
        }
        // Return only entries still present after concurrent Forget/Retry.
        // Otherwise the backend would be correct but a stale UI poll could
        // briefly bring an already-removed row back on screen.
        updated.retain(|item| entries.contains_key(&item.gid));
        if changed {
            if let Ok(mut last_save) = engine.last_history_save.lock() {
                if urgent || last_save.elapsed() >= Duration::from_secs(10) {
                    match save_json(&engine.data_dir.join("downloads.json"), &*entries) {
                        Ok(()) => *last_save = Instant::now(),
                        Err(error) => eprintln!("Unable to persist download history: {error}"),
                    }
                }
            }
        }
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

// Validate against the live engine rather than a cached UI status. In
// particular, aria2 restores paused jobs from a session after restarting.
fn control_method(action: &str, status: &str) -> Result<&'static str, String> {
    match (action, status) {
        ("pause", "active" | "waiting") => Ok("pause"),
        ("resume", "paused") => Ok("unpause"),
        ("cancel", "active" | "waiting" | "paused") => Ok("forceRemove"),
        ("pause", "paused") => Err("This transfer is already paused".into()),
        ("resume", "active" | "waiting") => Err("This transfer is already running or queued".into()),
        ("pause" | "resume" | "cancel", "complete") =>
            Err("This download has already completed".into()),
        ("pause" | "resume" | "cancel", "error" | "removed") =>
            Err("This transfer has stopped. Use Retry where available.".into()),
        ("pause" | "resume" | "cancel", _) => Err("Cannot control a transfer with an unknown state".into()),
        _ => Err("Unsupported action".into()),
    }
}

#[tauri::command]
async fn control_download(engine: State<'_, Engine>, gid: String, action: String) -> Result<(), String> {
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    if !matches!(action.as_str(), "pause" | "resume" | "cancel") {
        return Err("Unsupported action".into());
    }
    let live = engine.rpc("tellStatus", vec![json!(gid.clone())]).await?;
    let status = live.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let method = control_method(&action, status)?;
    engine.rpc(method, vec![json!(gid.clone())]).await?;

    // Reflect the operation immediately rather than waiting for the next poll.
    // Persist state before reporting success, but do not claim RPC failure if
    // the engine already accepted the action.
    {
        let mut entries = engine.downloads.lock().map_err(|e| e.to_string())?;
        if let Some(entry) = entries.get_mut(&gid) {
            entry.status = match action.as_str() {
                "pause" => "paused",
                "resume" => "waiting",
                _ => "removed",
            }.into();
            entry.download_speed = 0;
            entry.upload_speed = 0;
        }
        if let Err(error) = save_json(&engine.data_dir.join("downloads.json"), &*entries) {
            eprintln!("Transfer updated in aria2, but failed to save history: {error}");
        }
    }
    if let Err(error) = engine.rpc("saveSession", vec![]).await {
        eprintln!("Unable to checkpoint aria2 session after transfer control: {error}");
    }
    Ok(())
}


fn bulk_control_method(action: &str) -> Result<&'static str, String> {
    match action {
        "pause" => Ok("pauseAll"),
        "resume" => Ok("unpauseAll"),
        _ => Err("Unsupported bulk action".into()),
    }
}

fn apply_bulk_status(entries: &mut BTreeMap<String, Download>, action: &str) -> usize {
    let mut changed = 0;
    for entry in entries.values_mut() {
        let should_change = match action {
            "pause" => matches!(entry.status.as_str(), "active" | "waiting"),
            "resume" => entry.status == "paused",
            _ => false,
        };
        if should_change {
            entry.status = if action == "pause" { "paused" } else { "waiting" }.into();
            entry.download_speed = 0;
            entry.upload_speed = 0;
            changed += 1;
        }
    }
    changed
}

#[tauri::command]
async fn control_all_downloads(engine: State<'_, Engine>, action: String) -> Result<usize, String> {
    // This process launches and owns its own private aria2 instance; global
    // pause/unpause therefore cannot interfere with another app's downloads.
    let method = bulk_control_method(&action)?;
    engine.rpc(method, vec![]).await?;
    let changed = {
        let mut entries = engine.downloads.lock().map_err(|error| error.to_string())?;
        let changed = apply_bulk_status(&mut entries, &action);
        if changed > 0 {
            if let Err(error) = save_json(&engine.data_dir.join("downloads.json"), &*entries) {
                eprintln!("Bulk control succeeded but history checkpoint failed: {error}");
            }
        }
        changed
    };
    if let Err(error) = engine.rpc("saveSession", vec![]).await {
        eprintln!("Unable to checkpoint aria2 session after bulk control: {error}");
    }
    Ok(changed)
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
async fn move_queued_download(engine: State<'_, Engine>, gid: String, direction: String, position: Option<u32>) -> Result<(), String> {
    let position = match direction.as_str() {
        "top" => 0,
        "bottom" => 0,
        "position" => position.filter(|p| *p <= 100_000).ok_or("Invalid queue position")?,
        _ => return Err("Unknown queue action".into()),
    };
    if !engine.downloads.lock().map_err(|e| e.to_string())?.contains_key(&gid) {
        return Err("Unknown download".into());
    }
    let live = engine.rpc("tellStatus", vec![json!(gid.clone())]).await?;
    if live.get("status").and_then(Value::as_str) != Some("waiting") {
        return Err("Only waiting downloads can be moved in the queue".into());
    }
    let how = if direction == "bottom" { "POS_END" } else { "POS_SET" };
    engine.rpc("changePosition", vec![json!(gid), json!(position), json!(how)]).await?;
    let _ = engine.rpc("saveSession", vec![]).await;
    Ok(())
}

#[tauri::command]
async fn check_engine(engine: State<'_, Engine>) -> Result<String, String> {
    {
        let mut child = engine.child.lock().map_err(|error| error.to_string())?;
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("aria2c exited unexpectedly: {status}. Restart the application."));
        }
    }
    let version = engine.rpc("getVersion", vec![]).await?;
    version.get("version").and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "aria2 returned no engine version".to_string())
}

fn save_schedule(engine: &Engine, schedule: &Schedule) -> Result<(), String> {
    save_json(&engine.data_dir.join("schedule.json"), schedule)
}

#[tauri::command]
fn forget_media_job(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    let mut jobs = engine.media_jobs.lock().map_err(|error| error.to_string())?;
    let job = jobs.get(&id).ok_or("Media job not found")?;
    if matches!(job.status.as_str(), "downloading" | "paused" | "queued") {
        return Err("Stop this media download before removing its history".into());
    }
    let mut updated = jobs.clone();
    updated.remove(&id);
    save_json(&engine.data_dir.join("media_jobs.json"), &updated)?;
    *jobs = updated;
    Ok(())
}

#[tauri::command]
fn clear_media_history(engine: State<'_, Engine>) -> Result<usize, String> {
    let mut jobs = engine.media_jobs.lock().map_err(|error| error.to_string())?;
    let mut updated = jobs.clone();
    updated.retain(|_, job| matches!(job.status.as_str(), "downloading" | "paused" | "queued"));
    let removed = jobs.len() - updated.len();
    save_json(&engine.data_dir.join("media_jobs.json"), &updated)?;
    *jobs = updated;
    Ok(removed)
}

#[tauri::command]
fn list_media_jobs(engine: State<'_, Engine>) -> Result<Vec<MediaJob>, String> {
    Ok(engine.media_jobs.lock().map_err(|error| error.to_string())?.values().cloned().collect())
}

#[tauri::command]
async fn preview_media(source: String) -> Result<MediaPreview, String> {
    media_http_url(&source)?;
    // yt-dlp is an optional system dependency, never silently downloaded.
    let result = tokio::task::spawn_blocking(move || {
        let version = yt_dlp_command().arg("--version")
            .stdin(Stdio::null()).output()
            .map_err(|_| "yt-dlp not found. Install it yourself before using Video downloader.".to_string())?;
        if !version.status.success() { return Err("yt-dlp could not start".into()); }
        let output = yt_dlp_command()
            .args(["--ignore-config", "--js-runtimes", "node", "--no-playlist", "--skip-download", "--dump-single-json", "--no-warnings", "--no-call-home"])
            .arg("--").arg(&source)
            .stdin(Stdio::null()).output()
            .map_err(|error| format!("Could not inspect media: {error}"))?;
        if !output.status.success() {
            return Err(media_inspect_error(&output.stderr, &source, "Media inspection failed"));
        }
        if output.stdout.len() > 2_000_000 {
            return Err("Media metadata is too large".into());
        }
        let info: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "yt-dlp returned invalid media metadata")?;
        Ok(MediaPreview {
            title: info.get("title").and_then(Value::as_str).unwrap_or("Untitled media").chars().take(250).collect(),
            duration_seconds: info.get("duration").and_then(Value::as_f64),
            uploader: info.get("uploader").and_then(Value::as_str).unwrap_or("").chars().take(200).collect(),
            webpage_url: info.get("webpage_url").and_then(Value::as_str).unwrap_or("").chars().take(4096).collect(),
            yt_dlp_version: String::from_utf8_lossy(&version.stdout).trim().chars().take(64).collect(),
            formats: media_formats(&info),
        })
    }).await.map_err(|error| error.to_string())?;
    result
}

// yt-dlp's dedicated progress template is intentionally narrow; no terminal
// escapes, arbitrary log text, or unbounded captured output is kept in memory.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeedSubscription {
    id: String,
    source: String,
    title: String,
    #[serde(default)]
    auto_download: bool,
    #[serde(default)]
    seen_sources: Vec<String>,
    #[serde(default)]
    last_checked_unix: u64,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedPreview {
    title: String,
    entries: Vec<FeedEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedEntry {
    title: String,
    source: String,
    enclosure: bool,
}

fn public_feed_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 || a == 10 || a == 127 || a >= 224 ||
        a == 169 && b == 254 || a == 172 && (16..=31).contains(&b) ||
        a == 192 && b == 168 || a == 100 && (64..=127).contains(&b) ||
        a == 198 && (b == 18 || b == 19 || b == 51 && c == 100) ||
        a == 192 && (b == 0 || b == 88 && c == 99 || b == 0 && c == 2) ||
        a == 203 && b == 0 && c == 113)
}

fn validated_feed_url(value: &str) -> Result<(url::Url, String, u16), String> {
    if value.len() > 2048 || value.chars().any(|ch| ch.is_control()) {
        return Err("Invalid feed URL".into());
    }
    let url = url::Url::parse(value).map_err(|_| "Enter a complete RSS/Atom feed URL")?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("Feed must use HTTPS or HTTP".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Feed URLs must not contain credentials".into());
    }
    let host = url.host_str().ok_or("Feed URL must have a hostname")?.to_string();
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") ||
        host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".test") {
        return Err("Local/private network feeds are not supported".into());
    }
    let port = url.port_or_known_default().ok_or("Invalid feed port")?;
    if (url.scheme() == "https" && port != 443) || (url.scheme() == "http" && port != 80) {
        return Err("Only standard HTTP/HTTPS feed ports are supported".into());
    }
    Ok((url, host, port))
}

fn parsed_feed(bytes: &[u8]) -> Result<FeedPreview, String> {
    let feed = feed_rs::parser::parse(bytes)
        .map_err(|error| format!("Invalid RSS/Atom/JSON feed: {error}"))?;
    let entries = feed.entries.iter().take(50).filter_map(|entry| {
        // feed-rs 3 stores RSS enclosures in MediaObject.content; Atom
        // enclosures usually appear in Entry.links. Consider all supported
        // representations before falling back to an article link.
        let attachment = entry.links.iter()
            .find(|link| link.rel.as_deref() == Some("enclosure"))
            .map(|link| link.href.clone())
            .or_else(|| entry.content.as_ref()
                .and_then(|content| content.src.as_ref())
                .map(|link| link.href.clone()))
            .or_else(|| entry.media.iter()
                .flat_map(|media| media.content.iter())
                .find_map(|content| content.url.as_ref().map(ToString::to_string)));
        let source = attachment.clone().or_else(|| entry.links.iter()
            .find(|link| link.rel.as_deref() == Some("alternate"))
            .or_else(|| entry.links.first()).map(|link| link.href.clone()))?;
        // Enclosure or article links must be explicit HTTP(S); never allow
        // file://, javascript: or other schemes from untrusted feed content.
        media_http_url(&source).ok()?;
        Some(FeedEntry {
            title: entry.title.as_ref().map(|title| title.content.as_str())
                .unwrap_or("Untitled entry").chars().filter(|ch| !ch.is_control()).take(160).collect(),
            source, enclosure: attachment.is_some(),
        })
    }).collect();
    Ok(FeedPreview {
        title: feed.title.map(|title| title.content.chars().filter(|ch| !ch.is_control()).take(160).collect())
            .unwrap_or_else(|| "RSS feed".into()),
        entries,
    })
}

#[tauri::command]
async fn inspect_feed(source: String) -> Result<FeedPreview, String> {
    let (url, host, port) = validated_feed_url(&source)?;
    // Resolve before connecting and pin to a vetted public IPv4 address.
    // Redirects and inherited proxy settings are disabled to prevent fetching
    // another target, including loopback and other private services.
    let addr = tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        (host.as_str(), port).to_socket_addrs()
            .map_err(|error| format!("Cannot resolve feed host: {error}"))?
            .find(|address| match address.ip() {
                std::net::IpAddr::V4(ip) => public_feed_ipv4(ip),
                std::net::IpAddr::V6(_) => false,
            }).ok_or_else(|| "Feed hostname has no permitted public IPv4 address".to_string())
    }).await.map_err(|error| error.to_string())??;
    let host = url.host_str().ok_or("Invalid feed host")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, addr)
        .timeout(Duration::from_secs(15))
        .build().map_err(|error| error.to_string())?;
    let mut response = client.get(url)
        .send().await.map_err(|error| format!("Feed request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Feed request returned HTTP {}", response.status()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len() + chunk.len() > 1_048_576 {
            return Err("Feed exceeds 1 MiB limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    parsed_feed(&body)
}

#[tauri::command]
fn list_feed_subscriptions(engine: State<'_, Engine>) -> Result<Vec<FeedSubscription>, String> {
    Ok(engine.feed_subscriptions.lock().map_err(|error| error.to_string())?
        .values().cloned().collect())
}

#[tauri::command]
async fn subscribe_feed(engine: State<'_, Engine>, source: String) -> Result<FeedSubscription, String> {
    validated_feed_url(&source)?;
    let preview = inspect_feed(source.clone()).await?;
    let mut subscriptions = engine.feed_subscriptions.lock().map_err(|error| error.to_string())?;
    if subscriptions.len() >= 20 { return Err("At most 20 feed subscriptions are supported".into()); }
    if subscriptions.values().any(|item| item.source == source) {
        return Err("This feed is already subscribed".into());
    }
    let entry = FeedSubscription {
        id: uuid::Uuid::new_v4().to_string(), source, title: preview.title,
        auto_download: false,
        seen_sources: preview.entries.iter().map(|item| item.source.clone()).collect(),
        last_checked_unix: unix_now(), error: None,
    };
    let mut next = subscriptions.clone();
    next.insert(entry.id.clone(), entry.clone());
    save_json(&engine.data_dir.join("feeds.json"), &next)?;
    *subscriptions = next;
    Ok(entry)
}

#[tauri::command]
fn set_feed_auto_download(engine: State<'_, Engine>, id: String, enabled: bool) -> Result<(), String> {
    let mut subscriptions = engine.feed_subscriptions.lock().map_err(|error| error.to_string())?;
    let mut next = subscriptions.clone();
    let item = next.get_mut(&id).ok_or("Unknown feed subscription")?;
    item.auto_download = enabled;
    save_json(&engine.data_dir.join("feeds.json"), &next)?;
    *subscriptions = next;
    Ok(())
}

#[tauri::command]
fn remove_feed_subscription(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    let mut subscriptions = engine.feed_subscriptions.lock().map_err(|error| error.to_string())?;
    let mut next = subscriptions.clone();
    if next.remove(&id).is_none() { return Err("Unknown feed subscription".into()); }
    save_json(&engine.data_dir.join("feeds.json"), &next)?;
    *subscriptions = next;
    Ok(())
}

async fn poll_feed_subscriptions(engine: &Engine) {
    let subscriptions = match engine.feed_subscriptions.lock() {
        Ok(entries) => entries.values().filter(|item| item.auto_download &&
            unix_now().saturating_sub(item.last_checked_unix) >= 900)
            .cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };
    for original in subscriptions {
        let result = inspect_feed(original.source.clone()).await;
        let mut seen = original.seen_sources.clone();
        let mut last_error = None;
        match result {
            Ok(feed) => {
                for item in feed.entries {
                    if !item.enclosure || seen.iter().any(|url| url == &item.source) { continue; }
                    // Remote feed data is not equivalent to a user-pasted URL:
                    // reject local hosts and nonstandard ports for auto actions.
                    if validated_feed_url(&item.source).is_err() { continue; }
                    let duplicate = engine.downloads.lock().map(|entries|
                        entries.values().any(|download| download.source == item.source)).unwrap_or(true);
                    if duplicate {
                        seen.push(item.source);
                        continue;
                    }
                    match enqueue(engine, item.source.clone()).await {
                        Ok(_) => seen.push(item.source),
                        Err(error) => last_error = Some(error),
                    }
                }
            }
            Err(error) => last_error = Some(error),
        }
        // Bound persisted dedupe history and apply updates only if the feed
        // is still subscribed and automatic downloads are still enabled.
        let keep = seen.len().saturating_sub(1000);
        seen.drain(..keep);
        if let Ok(mut current) = engine.feed_subscriptions.lock() {
            if let Some(item) = current.get(&original.id) {
                if !item.auto_download || item.source != original.source { continue; }
            } else { continue; }
            let mut next = current.clone();
            if let Some(item) = next.get_mut(&original.id) {
                item.seen_sources = seen;
                item.last_checked_unix = unix_now();
                item.error = last_error;
            }
            if let Err(error) = save_json(&engine.data_dir.join("feeds.json"), &next) {
                eprintln!("Unable to checkpoint RSS feed: {error}");
            } else { *current = next; }
        }
    }
}

fn playlist_overall_progress(selected: &[u32], index: Option<u32>, percent: f32) -> Option<f32> {
    let index = index?;
    if selected.is_empty() || !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return None;
    }
    let position = selected.iter().position(|item| *item == index)?;
    let total = selected.len() as f32;
    Some(((position as f32 + percent / 100.0) / total * 100.0).min(100.0))
}

fn parse_media_progress(line: &str) -> Option<(f32, String, String, String, Option<u32>)> {
    let raw = line.trim().strip_prefix("DM_PROGRESS:")?;
    let mut parts = raw.splitn(5, '|');
    let percent = parts.next()?.trim().trim_end_matches('%').trim().parse::<f32>().ok()?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) { return None; }
    let speed = parts.next()?.trim().chars().take(40).collect();
    let eta = parts.next()?.trim().chars().take(40).collect();
    let item = parts.next().unwrap_or("").trim();
    let item = if !item.is_empty() && item.len() <= 80 &&
        item.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')) {
        item.to_string()
    } else { String::new() };
    let index = parts.next().and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|index| (1..=50).contains(index));
    Some((percent, speed, eta, item, index))
}

fn media_inspect_error(stderr: &[u8], source: &str, fallback: &str) -> String {
    // Metadata inspection can emit untrusted multiline output. Limit the
    // retained diagnostics and redact the original URL before displaying.
    let tail = if stderr.len() > 12_000 { &stderr[stderr.len()-12_000..] } else { stderr };
    let lines = String::from_utf8_lossy(tail);
    let lines: Vec<String> = lines.lines().rev().take(12)
        .map(|line| line.chars().take(350).collect()).collect();
    let summary = media_error_summary(&lines, source);
    if summary.is_empty() { fallback.to_string() }
    else { format!("{fallback}: {summary}") }
}

// Retain just a few yt-dlp diagnostic lines, not unlimited third-party output.
// The source URL may contain private query tokens, so redact it in the UI.
fn media_error_summary(lines: &[String], source: &str) -> String {
    let mut picked: Vec<String> = lines.iter()
        .filter(|line| line.contains("ERROR:") || line.contains("WARNING:"))
        .rev().take(4).cloned().collect();
    if picked.is_empty() {
        picked = lines.iter().rev().take(2).cloned().collect();
    }
    picked.reverse();
    picked.join(" | ").replace(source, "[media URL]")
        .chars().filter(|ch| !ch.is_control()).take(900).collect()
}

#[tauri::command]
fn cancel_media_download(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    let cancellations = engine.media_cancellations.lock().map_err(|error| error.to_string())?;
    let cancel = cancellations.get(&id).ok_or("Media job is no longer running")?;
    cancel.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
fn control_media_download(engine: State<'_, Engine>, id: String, action: String) -> Result<(), String> {
    if !matches!(action.as_str(), "pause" | "resume") {
        return Err("Invalid media control action".into());
    }
    #[cfg(not(unix))]
    { return Err("Media pause/resume is currently supported on Linux and other Unix desktops only".into()); }
    #[cfg(unix)]
    {
        let flags = engine.media_pauses.lock().map_err(|error| error.to_string())?;
        let flag = flags.get(&id).ok_or("Media job is not running")?;
        let jobs = engine.media_jobs.lock().map_err(|error| error.to_string())?;
        let job = jobs.get(&id).ok_or("Media job not found")?;
        if action == "pause" && job.status != "downloading" {
            return Err("Only a downloading media job can be paused".into());
        }
        if action == "resume" && job.status != "paused" {
            return Err("Only a paused media job can be resumed".into());
        }
        // Worker is the sole owner of its Child and applies the signal.
        flag.store(action == "pause", Ordering::Release);
        Ok(())
    }
}

#[tauri::command]
fn retry_media_download(engine: State<'_, Engine>, id: String) -> Result<MediaJob, String> {
    let original = {
        let jobs = engine.media_jobs.lock().map_err(|error| error.to_string())?;
        let job = jobs.get(&id).ok_or("Unknown media download")?;
        if !matches!(job.status.as_str(), "error" | "cancelled" | "interrupted") {
            return Err("Only failed, cancelled or interrupted media downloads can be retried".into());
        }
        job.clone()
    };
    // Existing partial files remain on disk, letting yt-dlp resume where the
    // server and media format support it. The old history record is preserved.
    start_media_download(engine, original.title, original.format, original.playlist, original.playlist_items)
}

#[tauri::command]
fn start_media_download(engine: State<'_, Engine>, source: String, format: String, playlist: bool, playlist_items: Vec<u32>) -> Result<MediaJob, String> {
    media_http_url(&source)?;
    validate_playlist_items(&playlist_items, playlist)?;
    if playlist && format.starts_with("id:") {
        return Err("Exact format IDs apply only to one inspected media item, not a playlist".into());
    }
    let args = media_format_args(&format)?;
    let output_folder = engine.settings.lock().map_err(|error| error.to_string())?.download_dir.clone();
    fs::create_dir_all(&output_folder).map_err(|error| error.to_string())?;
    let mut command = yt_dlp_command();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Give the downloader its own process group so spawned FFmpeg helpers
        // are paused and stopped together, not left running in the background.
        command.process_group(0);
    }
    command.args(["--ignore-config", "--js-runtimes", "node", "--newline", "--no-color", "--no-overwrites", "--restrict-filenames"]);
    if playlist {
        // Bound playlist size; downloading arbitrary thousands of remote entries
        // should require separate explicit batches.
        command.args(["--yes-playlist", "--playlist-end", "50"]);
        if !playlist_items.is_empty() {
            command.arg("--playlist-items");
            command.arg(playlist_items.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
        }
    } else {
        command.arg("--no-playlist");
    }
    command.args(["--progress-template", "download:DM_PROGRESS:%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(info.id)s|%(info.playlist_index)s"]);
    command.args(args);
    command.arg("-P").arg(&output_folder);
    command.arg("--").arg(&source);
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|_| "yt-dlp is missing or could not launch. Install yt-dlp and FFmpeg to enable media downloads.".to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let job = MediaJob {
        id: id.clone(), title: source, status: "downloading".into(),
        format, playlist, playlist_items, error: None, progress: None, speed: String::new(), eta: String::new(),
        current_item: String::new(), playlist_index: None, overall_progress: None,
    };
    {
        let mut jobs = engine.media_jobs.lock().map_err(|error| error.to_string())?;
        jobs.insert(id.clone(), job.clone());
        if let Err(error) = save_json(&engine.data_dir.join("media_jobs.json"), &*jobs) {
            eprintln!("Media launched, but its history could not be persisted: {error}");
        }
    }
    let pause = Arc::new(AtomicBool::new(false));
    engine.media_pauses.lock().map_err(|error| error.to_string())?.insert(id.clone(), Arc::clone(&pause));
    let stop = Arc::new(AtomicBool::new(false));
    engine.media_cancellations.lock().map_err(|error| error.to_string())?
        .insert(id.clone(), Arc::clone(&stop));
    let jobs = Arc::clone(&engine.media_jobs);
    let cancellations = Arc::clone(&engine.media_cancellations);
    let pauses = Arc::clone(&engine.media_pauses);
    let media_history_path = engine.data_dir.join("media_jobs.json");
    let reader_jobs = Arc::clone(&jobs);
    let reader_id = id.clone();
    let selected_for_progress = job.playlist_items.clone();
    let stderr_reader = child.stderr.take().map(|output| std::thread::spawn(move || {
        let mut recent: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            let trimmed: String = line.trim().chars().filter(|ch| !ch.is_control())
                .take(350).collect();
            if trimmed.is_empty() { continue; }
            if recent.len() >= 12 { recent.pop_front(); }
            recent.push_back(trimmed);
        }
        recent.into_iter().collect::<Vec<_>>()
    }));
    let progress_reader = child.stdout.take().map(|output| std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if let Some((progress, speed, eta, current_item, playlist_index)) = parse_media_progress(&line) {
                if let Ok(mut entries) = reader_jobs.lock() {
                    if let Some(entry) = entries.get_mut(&reader_id) {
                        if entry.status == "downloading" {
                            entry.progress = Some(progress);
                            entry.speed = speed;
                            entry.eta = eta;
                            entry.current_item = current_item;
                            entry.playlist_index = playlist_index;
                            entry.overall_progress = playlist_overall_progress(&selected_for_progress, playlist_index, progress);
                        }
                    }
                }
            }
        }
    }));
    std::thread::spawn(move || {
        let mut is_paused = false;
        let (result, cancelled) = loop {
            if stop.load(Ordering::Acquire) {
                #[cfg(unix)]
                {
                    // Group contains only this app-owned yt-dlp child and its
                    // helper processes; SIGKILL works even when paused.
                    let _ = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                }
                let _ = child.kill();
                break (child.wait(), true);
            }
            // Applying SIGSTOP/SIGCONT here rather than from the UI avoids
            // signals to a PID that has already exited/recycled.
            #[cfg(unix)]
            {
                let should_pause = pause.load(Ordering::Acquire);
                if should_pause != is_paused {
                    let signal = if should_pause { libc::SIGSTOP } else { libc::SIGCONT };
                    let applied = unsafe { libc::kill(-(child.id() as i32), signal) } == 0;
                    if applied {
                        is_paused = should_pause;
                        if let Ok(mut entries) = jobs.lock() {
                            if let Some(entry) = entries.get_mut(&id) {
                                entry.status = if is_paused { "paused" } else { "downloading" }.into();
                                if is_paused { entry.speed.clear(); entry.eta.clear(); }
                            }
                            if let Err(error) = save_json(&media_history_path, &*entries) {
                                eprintln!("Unable to checkpoint media pause state: {error}");
                            }
                        }
                    } else {
                        eprintln!("Failed to change yt-dlp pause state");
                    }
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => break (Ok(status), false),
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(error) => break (Err(error), false),
            }
        };
        if let Some(reader) = progress_reader { let _ = reader.join(); }
        let diagnostics = stderr_reader
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        if let Ok(mut entries) = jobs.lock() {
            if let Some(entry) = entries.get_mut(&id) {
                if cancelled {
                    entry.status = "cancelled".into();
                    entry.speed.clear();
                    entry.eta.clear();
                } else {
                    match result {
                        Ok(status) if status.success() => {
                            entry.status = "complete".into();
                            entry.progress = Some(100.0);
                            entry.speed.clear();
                            entry.eta.clear();
                        }
                        Ok(status) => {
                            entry.status = "error".into();
                            let detail = media_error_summary(&diagnostics, &entry.title);
                            entry.error = Some(if detail.is_empty() {
                                format!("yt-dlp exited with {status}. Try Inspect media or run yt-dlp in Terminal for details.")
                            } else {
                                format!("yt-dlp exited with {status}: {detail}")
                            });
                        }
                        Err(error) => {
                            entry.status = "error".into();
                            entry.error = Some(format!("Cannot wait for yt-dlp: {error}"));
                        }
                    }
                }
            }
        }
        if let Ok(entries) = jobs.lock() {
            if let Err(error) = save_json(&media_history_path, &*entries) {
                eprintln!("Unable to persist completed media job: {error}");
            }
        }
        if let Ok(mut entries) = cancellations.lock() { entries.remove(&id); }
        if let Ok(mut entries) = pauses.lock() { entries.remove(&id); }
    });
    Ok(job)
}

#[tauri::command]
fn get_schedule(engine: State<'_, Engine>) -> Result<Schedule, String> {
    Ok(engine.schedule.lock().map_err(|error| error.to_string())?.clone())
}

#[tauri::command]
async fn arm_schedule(
    engine: State<'_, Engine>, start_at_unix: u64, concurrent: u8,
    after_complete: String,
) -> Result<Schedule, String> {
    validate_schedule_request(start_at_unix, concurrent, &after_complete, unix_now())?;
    {
        let current = engine.schedule.lock().map_err(|error| error.to_string())?;
        if matches!(current.state.as_str(), "armed" | "running" | "power_pending") {
            return Err("Cancel the existing schedule before creating another".into());
        }
    }
    let gids: Vec<String> = engine.downloads.lock().map_err(|error| error.to_string())?
        .values().filter(|item| matches!(item.status.as_str(), "active" | "waiting" | "paused"))
        .map(|item| item.gid.clone()).collect();
    if gids.is_empty() {
        return Err("Add a download first; there are no transfers to schedule".into());
    }
    // Pause the private engine before arming; even if persisting fails the
    // user can Resume All. Never modify an unrelated aria2 instance.
    engine.rpc("pauseAll", vec![]).await?;
    let schedule = Schedule {
        state: "armed".into(), start_at_unix, concurrent, after_complete,
        gids, power_at_unix: 0,
        message: "Queue paused until the selected time. Keep Download Manager running.".into(),
    };
    let mut current = engine.schedule.lock().map_err(|error| error.to_string())?;
    save_schedule(&engine, &schedule)?;
    *current = schedule.clone();
    Ok(schedule)
}

#[tauri::command]
fn cancel_schedule(engine: State<'_, Engine>) -> Result<Schedule, String> {
    let mut schedule = engine.schedule.lock().map_err(|error| error.to_string())?;
    if !matches!(schedule.state.as_str(), "armed" | "running" | "power_pending") {
        return Err("There is no active schedule to cancel".into());
    }
    let mut next = schedule.clone();
    next.state = "cancelled".into();
    next.power_at_unix = 0;
    next.message = "Schedule cancelled. Downloads remain in their current state.".into();
    save_schedule(&engine, &next)?;
    *schedule = next.clone();
    Ok(next)
}

async fn engine_has_unfinished_transfers(engine: &Engine) -> Result<bool, String> {
    // Include follow-up payload downloads created by torrent/metalink metadata,
    // and jobs added after the schedule was armed. Never power off while work
    // is still active or queued.
    let active = engine.rpc("tellActive", vec![json!(["gid"])]).await?;
    if active.as_array().is_none_or(|list| !list.is_empty()) { return Ok(true); }
    let waiting = engine.rpc("tellWaiting", vec![
        json!(0), json!(1000), json!(["gid", "status"]),
    ]).await?;
    Ok(waiting.as_array().is_none_or(|list| !list.is_empty()))
}

// Runs only while the desktop app is open; the engine is a managed Tauri
// resource. Scheduler state is checkpointed across ordinary UI navigation.
async fn schedule_tick(engine: &Engine) {
    let schedule = match engine.schedule.lock() {
        Ok(lock) => lock.clone(), Err(_) => return,
    };
    let now = unix_now();
    if schedule.state == "armed" && now >= schedule.start_at_unix {
        let options = json!({"max-concurrent-downloads": schedule.concurrent.to_string()});
        if let Err(error) = engine.rpc("changeGlobalOption", vec![options]).await {
            eprintln!("Scheduler could not set concurrency: {error}");
            return;
        }
        if let Err(error) = engine.rpc("unpauseAll", vec![]).await {
            eprintln!("Scheduler could not resume transfers: {error}");
            return;
        }
        if let Ok(mut lock) = engine.schedule.lock() {
            if lock.state != "armed" { return; }
            let mut next = lock.clone();
            next.state = "running".into();
            next.message = "Scheduled downloads started.".into();
            if save_schedule(engine, &next).is_ok() { *lock = next; }
        }
        return;
    }
    if schedule.state == "running" {
        if schedule.gids.is_empty() { return; }
        let mut completed = 0usize;
        let mut failed = 0usize;
        for gid in &schedule.gids {
            // An RPC failure is unknown, never evidence that all files finished.
            let status = match engine.rpc("tellStatus", vec![json!(gid)]).await {
                Ok(value) => value,
                Err(_) => return,
            };
            match status.get("status").and_then(Value::as_str) {
                Some("complete") => completed += 1,
                Some("error" | "removed") => failed += 1,
                _ => {}
            }
        }
        if completed + failed != schedule.gids.len() { return; }
        if failed == 0 && schedule.after_complete != "none" {
            // A completed torrent metadata GID may have spawned a real payload
            // transfer. Delay power actions until the private engine is idle.
            match engine_has_unfinished_transfers(engine).await {
                Ok(false) => {},
                _ => return,
            }
        }
        if let Ok(mut lock) = engine.schedule.lock() {
            if lock.state != "running" { return; }
            let mut next = lock.clone();
            if failed > 0 || schedule.after_complete == "none" {
                next.state = "completed".into();
                next.message = if failed > 0 {
                    format!("{failed} scheduled transfer(s) failed; power action skipped.")
                } else {
                    "All scheduled transfers completed.".into()
                };
            } else {
                next.state = "power_pending".into();
                next.power_at_unix = now.saturating_add(60);
                next.message = format!("All scheduled transfers complete. {} in 60 seconds. Cancel the schedule to stop it.",
                    if next.after_complete == "sleep" { "Sleep" } else { "Shutdown" });
            }
            if save_schedule(engine, &next).is_ok() { *lock = next; }
        }
        return;
    }
    if schedule.state == "power_pending" && now >= schedule.power_at_unix {
        // A new download may have started during the 60-second grace period.
        // Refuse to power off if any aria2 transfer remains.
        match engine_has_unfinished_transfers(engine).await {
            Ok(false) => {},
            _ => return,
        }
        // Re-check before acting, including after the grace period. Mark
        // completed before launch to avoid issuing a second power command.
        let action = if schedule.after_complete == "sleep" { "suspend" } else { "poweroff" };
        let mut lock = match engine.schedule.lock() { Ok(lock) => lock, Err(_) => return };
        if lock.state != "power_pending" || lock.power_at_unix > unix_now() { return; }
        let mut next = lock.clone();
        next.state = "completed".into();
        next.message = format!("Requested system {action} after scheduled completion.");
        if save_schedule(engine, &next).is_err() { return; }
        *lock = next;
        #[cfg(target_os = "linux")]
        {
            // Explicitly user-selected system action. No shell is invoked.
            let result = Command::new("systemctl").arg(action)
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            if let Err(error) = result {
                eprintln!("Scheduled power action could not launch: {error}");
            }
        }
        #[cfg(not(target_os = "linux"))]
        eprintln!("Scheduled power action is not implemented on this OS");
    }
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

fn completed_download_folder(item: &Download) -> Result<PathBuf, String> {
    if item.status != "complete" {
        return Err("The download must be complete before opening its folder".into());
    }
    if item.path.is_empty() {
        return Err("No saved file path is available for this download".into());
    }
    let file = std::path::Path::new(&item.path);
    if !file.is_file() {
        return Err(format!("Downloaded file was not found: {}", file.display()));
    }
    let folder = file.parent().ok_or("Cannot determine downloaded file folder")?;
    if !folder.is_dir() {
        return Err("Downloaded file folder no longer exists".into());
    }
    Ok(folder.to_path_buf())
}

#[tauri::command]
fn open_download_folder(engine: State<'_, Engine>, gid: String) -> Result<(), String> {
    let folder = {
        let entries = engine.downloads.lock().map_err(|error| error.to_string())?;
        let item = entries.get(&gid).ok_or("Unknown download")?;
        completed_download_folder(item)?
    };
    // Pass the folder as a separate process argument, never via a shell.
    // Only invoke the local desktop file manager for a file verified on disk.
    #[cfg(target_os = "linux")]
    {
        let mut child = Command::new("xdg-open")
            .arg(&folder).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|error| format!("Could not open file manager: {error}"))?;
        std::thread::spawn(move || { let _ = child.wait(); });
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = folder;
        Err("Open download folder is currently supported on Linux only".into())
    }
}

// Commit the history change on disk before publishing it in memory. If the
// filesystem is full or not writable, a failed removal must remain visible.
fn forget_history_entry(
    entries: &mut BTreeMap<String, Download>,
    gid: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let item = entries.get(gid).ok_or("Unknown download")?;
    if matches!(item.status.as_str(), "active" | "waiting" | "paused") {
        return Err("Stop the download before removing it from history".into());
    }
    let mut next = entries.clone();
    next.remove(gid);
    save_json(path, &next)?;
    *entries = next;
    Ok(())
}

// Remove completed history records atomically; downloaded files are untouched.
fn clear_completed_history(
    entries: &mut BTreeMap<String, Download>,
    path: &std::path::Path,
) -> Result<usize, String> {
    let completed = entries.values().filter(|item| item.status == "complete").count();
    if completed == 0 {
        return Ok(0);
    }
    let mut next = entries.clone();
    next.retain(|_, item| item.status != "complete");
    save_json(path, &next)?;
    *entries = next;
    Ok(completed)
}

// Clear only terminal direct-download records. Preserve active, queued and paused transfers.
fn clear_finished_history(
    entries: &mut BTreeMap<String, Download>,
    path: &std::path::Path,
) -> Result<usize, String> {
    let mut next = entries.clone();
    next.retain(|_, item| !matches!(item.status.as_str(), "complete" | "error" | "removed" | "interrupted"));
    let removed = entries.len() - next.len();
    if removed == 0 { return Ok(0); }
    save_json(path, &next)?;
    *entries = next;
    Ok(removed)
}

#[tauri::command]
fn clear_finished_downloads(engine: State<'_, Engine>) -> Result<usize, String> {
    let mut entries = engine.downloads.lock().map_err(|error| error.to_string())?;
    clear_finished_history(&mut entries, &engine.data_dir.join("downloads.json"))
}

#[tauri::command]
fn clear_completed_downloads(engine: State<'_, Engine>) -> Result<usize, String> {
    let mut entries = engine.downloads.lock().map_err(|error| error.to_string())?;
    clear_completed_history(&mut entries, &engine.data_dir.join("downloads.json"))
}

#[tauri::command]
fn forget_download(engine: State<'_, Engine>, gid: String) -> Result<(), String> {
    let mut entries = engine.downloads.lock().map_err(|e| e.to_string())?;
    forget_history_entry(&mut entries, &gid, &engine.data_dir.join("downloads.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_save_replaces_document_without_leaving_partial_file() {
        let folder = std::env::temp_dir().join(format!("download-manager-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("history.json");
        let mut first = BTreeMap::new();
        first.insert("old".to_string(), 1_u32);
        save_json(&path, &first).unwrap();
        let mut second = BTreeMap::new();
        second.insert("new".to_string(), 2_u32);
        save_json(&path, &second).unwrap();
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path).unwrap().unwrap(), second);
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path.with_extension("bak"))
            .unwrap().unwrap(), first);
        assert!(!path.with_extension("tmp").exists());
        fs::remove_file(&path).unwrap();
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path).unwrap().unwrap(), first);
        save_json(&path, &second).unwrap();
        // Valid JSON with an incompatible document schema must not replace backup.
        fs::write(&path, b"[]").unwrap();
        save_json(&path, &second).unwrap();
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path.with_extension("bak"))
            .unwrap().unwrap(), first);
        // Missing data is expected on first launch, corrupt data is not.
        assert!(load_json::<BTreeMap<String, u32>>(&folder.join("missing.json")).unwrap().is_none());
        fs::write(&path, b"{not-json").unwrap();
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path).unwrap().unwrap(), first);
        // Saving the recovered snapshot must not destroy the good backup.
        save_json(&path, &second).unwrap();
        assert_eq!(load_json::<BTreeMap<String, u32>>(&path.with_extension("bak"))
            .unwrap().unwrap(), first);
        // If the primary is absent and the remaining backup is unusable,
        // starting with empty history would hide an existing recovery problem.
        fs::remove_file(&path).unwrap();
        fs::write(path.with_extension("bak"), b"[]").unwrap();
        assert!(load_json::<BTreeMap<String, u32>>(&path).is_err());
        save_json(&path, &second).unwrap();
        fs::write(path.with_extension("bak"), serde_json::to_vec(&first).unwrap()).unwrap();
        fs::write(&path, b"{still-broken").unwrap();
        fs::write(path.with_extension("bak"), b"{also-broken").unwrap();
        assert!(load_json::<BTreeMap<String, u32>>(&path).is_err());
        save_json(&path, &second).unwrap();
        #[cfg(unix)]
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn restores_semantically_invalid_settings_from_valid_backup() {
        let folder = std::env::temp_dir().join(format!("dm-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("settings.json");
        let settings = Settings::defaults();
        save_json(&path, &settings).unwrap();
        save_json(&path, &settings).unwrap();
        let mut invalid = settings.clone();
        invalid.concurrent = 0;
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert_eq!(load_validated_settings(&path).unwrap().concurrent, settings.concurrent);
        // Persisting a repaired settings document must not replace the good
        // backup with the invalid-but-parseable old primary.
        save_json(&path, &settings).unwrap();
        assert!(load_json::<Settings>(&path.with_extension("bak")).unwrap()
            .unwrap().validate().is_ok());
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        fs::write(&path.with_extension("bak"), serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(load_validated_settings(&path).is_err());
        fs::remove_dir_all(&folder).unwrap();
    }
    #[test]
    fn waiting_queue_uses_actual_aria2_positions() {
        let make = |gid: &str, status: &str| Download {
            gid: gid.into(), source: "https://example.org/file".into(),
            is_torrent: false, name: gid.into(), status: status.into(),
            completed_length: 0, total_length: 10, download_speed: 0,
            upload_speed: 0, path: String::new(), error: None,
        };
        let mut items = vec![
            make("done", "complete"),
            make("later", "waiting"),
            make("active", "active"),
            make("first", "waiting"),
        ];
        sort_by_waiting_queue(&mut items, &json!([
            {"gid":"first"}, {"gid":"later"},
        ]));
        let order: Vec<_> = items.iter().map(|item| item.gid.clone()).collect();
        assert_eq!(order, vec!["first", "later", "done", "active"]);
        // Invalid or unavailable queue responses leave ordering untouched.
        sort_by_waiting_queue(&mut items, &Value::Null);
        let unchanged: Vec<_> = items.iter().map(|item| item.gid.clone()).collect();
        assert_eq!(unchanged, order);
    }

    #[test]
    fn restart_statuses_are_reconciled_before_retry() {
        assert!(status_needs_reconciliation("active"));
        assert!(status_needs_reconciliation("waiting"));
        assert!(status_needs_reconciliation("paused"));
        assert!(!status_needs_reconciliation("complete"));
        assert!(!status_needs_reconciliation("error"));
        assert!(!status_needs_reconciliation("removed"));
        assert!(!is_historical_status("recovering"));
        assert!(!matches!("recovering", "error" | "removed" | "interrupted"));
    }

    #[test]
    fn stale_polls_must_not_resurrect_removed_downloads() {
        let original = std::collections::BTreeSet::from([
            "removed-during-poll".to_string(),
            "still-live".to_string(),
        ]);
        // A removed or retried entry is no longer in the live map, but the
        // stale RPC result must not be allowed to recreate it.
        assert!(!may_insert_polled_gid("removed-during-poll", &original));
        assert!(!may_insert_polled_gid("still-live", &original));
        // aria2 follow-up GIDs discovered by the poll are still accepted.
        assert!(may_insert_polled_gid("new-torrent-child", &original));
    }

    #[test]
    fn clear_completed_only_removes_history_and_is_atomic() {
        let folder = std::env::temp_dir().join(format!("dm-clear-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("downloads.json");
        let make = |gid: &str, status: &str| Download {
            gid: gid.into(), source: "https://example.org/file".into(),
            is_torrent: false, name: gid.into(), status: status.into(),
            completed_length: 0, total_length: 10, download_speed: 0,
            upload_speed: 0, path: String::new(), error: None,
        };
        let mut entries = BTreeMap::from([
            ("done".into(), make("done", "complete")),
            ("paused".into(), make("paused", "paused")),
            ("error".into(), make("error", "error")),
        ]);
        save_json(&path, &entries).unwrap();
        let unwritable = folder.join("missing").join("downloads.json");
        assert!(clear_completed_history(&mut entries, &unwritable).is_err());
        assert!(entries.contains_key("done"));
        assert!(load_json::<BTreeMap<String, Download>>(&path).unwrap()
            .unwrap().contains_key("done"));
        assert_eq!(clear_completed_history(&mut entries, &path).unwrap(), 1);
        assert!(!entries.contains_key("done"));
        assert!(entries.contains_key("paused"));
        assert!(entries.contains_key("error"));
        assert_eq!(clear_completed_history(&mut entries, &path).unwrap(), 0);
        assert_eq!(load_json::<BTreeMap<String, Download>>(&path).unwrap()
            .unwrap().len(), 2);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn failed_history_removal_preserves_download_and_success_persists() {
        let folder = std::env::temp_dir().join(format!("dm-forget-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("downloads.json");
        let item = Download {
            gid: "done".into(), source: "https://example.org/test.txt".into(),
            is_torrent: false, name: "test.txt".into(), status: "complete".into(),
            completed_length: 4, total_length: 4, download_speed: 0,
            upload_speed: 0, path: String::new(), error: None,
        };
        let mut entries = BTreeMap::from([("done".to_string(), item.clone())]);
        save_json(&path, &entries).unwrap();

        // If writing the new snapshot fails, the in-memory and on-disk
        // history must both retain the entry, including on app shutdown.
        let unwritable = folder.join("missing-parent").join("downloads.json");
        assert!(forget_history_entry(&mut entries, "done", &unwritable).is_err());
        assert_eq!(entries.get("done"), Some(&item));
        assert!(load_json::<BTreeMap<String, Download>>(&path).unwrap()
            .unwrap().contains_key("done"));

        // Active transfers must never be removed from history.
        entries.get_mut("done").unwrap().status = "paused".into();
        assert!(forget_history_entry(&mut entries, "done", &path).is_err());
        entries.get_mut("done").unwrap().status = "complete".into();

        forget_history_entry(&mut entries, "done", &path).unwrap();
        assert!(!entries.contains_key("done"));
        assert!(load_json::<BTreeMap<String, Download>>(&path).unwrap()
            .unwrap().is_empty());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn folder_open_requires_completed_existing_file() {
        let folder = std::env::temp_dir().join(format!("dm-folder-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("fixture.txt");
        fs::write(&file, b"fixture").unwrap();
        let mut item = Download {
            gid: "one".into(), source: "https://example.org/fixture.txt".into(),
            is_torrent: false, name: "fixture.txt".into(), status: "active".into(),
            completed_length: 7, total_length: 7, download_speed: 0,
            upload_speed: 0, path: file.to_string_lossy().into_owned(), error: None,
        };
        assert!(completed_download_folder(&item).is_err());
        item.status = "complete".into();
        assert_eq!(completed_download_folder(&item).unwrap(), folder);
        fs::remove_file(&file).unwrap();
        assert!(completed_download_folder(&item).is_err());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn calculates_overall_progress_only_when_playlist_selection_is_known() {
        assert_eq!(playlist_overall_progress(&[1,3,8], Some(3), 50.0), Some(50.0));
        assert_eq!(playlist_overall_progress(&[1,3,8], Some(8), 100.0), Some(100.0));
        assert_eq!(playlist_overall_progress(&[], Some(1), 50.0), None);
        assert_eq!(playlist_overall_progress(&[1,3], Some(2), 10.0), None);
        assert_eq!(playlist_overall_progress(&[1,3], None, 10.0), None);
    }

    #[test]
    fn feed_subscription_deserialization_keeps_safe_defaults() {
        let feed: FeedSubscription = serde_json::from_str(
            r#"{"id":"abc","source":"https://example.org/rss","title":"Example"}"#).unwrap();
        assert!(!feed.auto_download);
        assert!(feed.seen_sources.is_empty());
        assert_eq!(feed.last_checked_unix, 0);
    }

    #[test]
    fn rss_parser_exposes_enclosures_and_blocks_private_feeds() {
        let xml = br#"<rss version="2.0"><channel><title>Updates</title>
            <item><title>File 1</title><enclosure url="https://example.org/file.zip" length="1234" type="application/zip" /></item>
            <item><title>Web link</title><link>https://example.org/post</link></item>
            </channel></rss>"#;
        let feed = parsed_feed(xml).unwrap();
        assert_eq!(feed.title, "Updates");
        assert_eq!(feed.entries.len(), 2);
        assert!(feed.entries[0].enclosure);
        assert!(!feed.entries[1].enclosure);
        assert!(validated_feed_url("https://example.org/feed").is_ok());
        assert!(validated_feed_url("http://localhost/feed").is_err());
        assert!(validated_feed_url("file:///etc/passwd").is_err());
        assert!(validated_feed_url("https://example.org:8080/feed").is_err());
        assert!(!public_feed_ipv4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!public_feed_ipv4(std::net::Ipv4Addr::new(192, 168, 1, 2)));
        assert!(public_feed_ipv4(std::net::Ipv4Addr::new(1, 1, 1, 1)));
    }

    #[test]
    fn playlist_selection_has_unique_bounded_one_based_indexes() {
        assert!(validate_playlist_items(&[], false).is_ok());
        assert!(validate_playlist_items(&[1, 3, 50], true).is_ok());
        assert!(validate_playlist_items(&[0], true).is_err());
        assert!(validate_playlist_items(&[51], true).is_err());
        assert!(validate_playlist_items(&[1, 1], true).is_err());
        assert!(validate_playlist_items(&[2], false).is_err());
        let entries = playlist_entries(&json!({"entries":[
            {"title":"First"},{"title":"Second"},null,{"title":"Fourth"}]}));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].index, 4);
    }

    #[test]
    fn paused_media_is_interrupted_on_restart() {
        let jobs = BTreeMap::from([("pause".into(), MediaJob {
            id: "pause".into(), title: "https://example.org/watch".into(),
            status: "paused".into(), format: "mp4".into(), playlist: false,
            playlist_items: vec![], error: None, progress: Some(12.0), speed: String::new(), eta: String::new(),
            current_item: String::new(), playlist_index: None, overall_progress: None,
        })]);
        let resumed = restore_media_jobs(jobs);
        assert_eq!(resumed["pause"].status, "interrupted");
    }

    #[test]
    fn media_restart_marks_in_flight_jobs_interrupted() {
        let job = MediaJob {
            id: "one".into(), title: "https://example.org/video".into(),
            status: "downloading".into(), format: "mp4".into(), playlist: true,
            playlist_items: vec![1, 3], error: None, progress: Some(20.0), speed: "4MiB/s".into(), eta: "00:30".into(),
            current_item: String::new(), playlist_index: None, overall_progress: None,
        };
        let jobs = restore_media_jobs(BTreeMap::from([("one".into(), job)]));
        assert_eq!(jobs["one"].status, "interrupted");
        assert!(jobs["one"].speed.is_empty());
        let serialized = serde_json::to_string(&jobs).unwrap();
        let parsed: BTreeMap<String, MediaJob> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed["one"].format, "mp4");
        assert!(parsed["one"].playlist);
    }

    #[test]
    fn media_inspection_error_explains_unavailable_videos() {
        let source = "https://www.youtube.com/watch?v=secret123";
        let error = media_inspect_error(
            format!("ERROR: [youtube] This video is unavailable: {source}").as_bytes(),
            source, "Media inspection failed");
        assert!(error.contains("video is unavailable"));
        assert!(!error.contains("secret123"));
        assert!(error.contains("[media URL]"));
    }

    #[test]
    fn media_diagnostics_show_error_and_redact_source_url() {
        let source = "https://example.com/watch?private=token";
        let lines = vec![
            "WARNING: older note".into(),
            format!("ERROR: Unsupported URL: {source}"),
        ];
        let summary = media_error_summary(&lines, source);
        assert!(summary.contains("ERROR: Unsupported URL"));
        assert!(summary.contains("[media URL]"));
        assert!(!summary.contains("private=token"));
        assert!(media_error_summary(&[], source).is_empty());
    }

    #[test]
    fn parses_only_bounded_media_progress_lines() {
        let (percentage, speed, eta, item, index) = parse_media_progress("DM_PROGRESS: 42.5%|2.3MiB/s|00:12|video123|7").unwrap();
        assert_eq!(percentage, 42.5);
        assert_eq!(speed, "2.3MiB/s");
        assert_eq!(eta, "00:12");
        assert_eq!(item, "video123");
        assert_eq!(index, Some(7));
        assert_eq!(parse_media_progress("DM_PROGRESS: 50%|2MiB/s|12|id|999").unwrap().4, None);
        assert!(parse_media_progress("[download] 40%").is_none());
        assert!(parse_media_progress("DM_PROGRESS:120%|1MiB/s|01:00").is_none());
        assert!(parse_media_progress("DM_PROGRESS:NaN%|1MiB/s|01:00").is_none());
        assert!(parse_media_progress("DM_PROGRESS:missing|1MiB/s|01:00").is_none());
    }

    #[test]
    fn preview_formats_filter_untrusted_ids_and_cap_result_count() {
        let info = json!({"formats": [
            {"format_id": "137", "ext": "mp4", "height": 1080, "filesize": 12345},
            {"format_id": "-exec", "ext": "mp4"},
            {"format_id": "best+audio", "ext": "mp4"},
            {"format_id": "251", "ext": "webm", "resolution": "audio only"}
        ]});
        let found = media_formats(&info);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "137");
        assert_eq!(found[0].filesize, Some(12345));
        assert_eq!(found[1].resolution, "audio only");
    }

    #[test]
    fn media_helper_rejects_unsafe_sources_and_formats() {
        assert!(media_http_url("https://example.org/watch?v=1").is_ok());
        assert!(media_http_url("http://example.org/video").is_ok());
        assert!(media_http_url("file:///etc/passwd").is_err());
        assert!(media_http_url("ftp://example.org/video").is_err());
        assert!(media_http_url("https://user:pass@example.org/watch").is_err());
        assert!(media_http_url("--exec=bad").is_err());
        assert!(media_http_url("https://example.org/\nmalicious").is_err());
        assert!(media_format_args("best").is_ok());
        assert!(media_format_args("mp4").is_ok());
        assert!(media_format_args("audio").is_ok());
        assert!(media_format_args("720p").is_ok());
        assert!(media_format_args("1080p").is_ok());
        assert_eq!(media_format_args("id:137").unwrap(), vec!["-f", "137"]);
        assert!(media_format_args("id:bestvideo+bestaudio").is_err());
        assert!(media_format_args("id:--exec").is_err());
        assert!(media_format_args("id:").is_err());
        assert!(media_format_args("--exec").is_err());
    }

    #[test]
    fn standard_aria2_server_connection_limit_does_not_break_startup() {
        assert_eq!(aria2_server_connections(1), 1);
        assert_eq!(aria2_server_connections(16), 16);
        assert_eq!(aria2_server_connections(32), 16);
    }

    #[test]
    fn restart_cancels_armed_and_power_pending_schedules() {
        for state in ["armed", "running", "power_pending"] {
            let schedule = Schedule { state: state.into(), power_at_unix: 44, ..Schedule::default() };
            let restored = recover_schedule_after_restart(schedule);
            assert_eq!(restored.state, "cancelled");
            assert_eq!(restored.power_at_unix, 0);
            assert!(restored.message.contains("restarted"));
        }
        let completed = Schedule { state: "completed".into(), ..Schedule::default() };
        assert_eq!(recover_schedule_after_restart(completed).state, "completed");
    }

    #[test]
    fn scheduler_validates_time_limit_and_power_action() {
        let now = 1_700_000_000;
        assert!(validate_schedule_request(now + 120, 3, "none", now).is_ok());
        assert!(validate_schedule_request(now + 120, 10, "shutdown", now).is_ok());
        assert!(validate_schedule_request(now + 120, 1, "sleep", now).is_ok());
        assert!(validate_schedule_request(now, 3, "none", now).is_err());
        assert!(validate_schedule_request(now - 60, 3, "none", now).is_err());
        assert!(validate_schedule_request(now + 10, 0, "none", now).is_err());
        assert!(validate_schedule_request(now + 10, 11, "none", now).is_err());
        assert!(validate_schedule_request(now + 10, 3, "delete", now).is_err());
        assert!(scheduled_terminal_state("complete"));
        assert!(scheduled_terminal_state("error"));
        assert!(!scheduled_terminal_state("paused"));
    }

    #[test]
    fn bulk_pause_resume_updates_only_eligible_history() {
        let make = |gid: &str, status: &str| Download {
            gid: gid.into(), source: "https://example.org/file".into(),
            is_torrent: false, name: gid.into(), status: status.into(),
            completed_length: 0, total_length: 10, download_speed: 120,
            upload_speed: 0, path: String::new(), error: None,
        };
        let mut entries = BTreeMap::from([
            ("active".into(), make("active", "active")),
            ("waiting".into(), make("waiting", "waiting")),
            ("paused".into(), make("paused", "paused")),
            ("done".into(), make("done", "complete")),
            ("recovering".into(), make("recovering", "recovering")),
        ]);
        assert_eq!(bulk_control_method("pause").unwrap(), "pauseAll");
        assert_eq!(bulk_control_method("resume").unwrap(), "unpauseAll");
        assert!(bulk_control_method("cancel").is_err());
        assert_eq!(apply_bulk_status(&mut entries, "pause"), 2);
        assert_eq!(entries["active"].status, "paused");
        assert_eq!(entries["active"].download_speed, 0);
        assert_eq!(entries["done"].status, "complete");
        assert_eq!(entries["recovering"].status, "recovering");
        assert_eq!(apply_bulk_status(&mut entries, "resume"), 3);
        assert_eq!(entries["waiting"].status, "waiting");
        assert_eq!(entries["done"].status, "complete");
        assert_eq!(apply_bulk_status(&mut entries, "unknown"), 0);
    }

    #[test]
    fn transfer_controls_reject_stale_or_invalid_states() {
        assert_eq!(control_method("pause", "active").unwrap(), "pause");
        assert_eq!(control_method("pause", "waiting").unwrap(), "pause");
        assert_eq!(control_method("resume", "paused").unwrap(), "unpause");
        assert_eq!(control_method("cancel", "paused").unwrap(), "forceRemove");
        assert!(control_method("resume", "active").is_err());
        assert!(control_method("resume", "waiting").is_err());
        assert!(control_method("pause", "paused").is_err());
        assert!(control_method("cancel", "complete").is_err());
        assert!(control_method("pause", "unknown").is_err());
        assert!(control_method("invalid", "active").is_err());
    }
    #[test]
    fn validates_connection_limits() {
        let mut settings = Settings::defaults();
        assert!(settings.validate().is_ok());
        settings.connections = 0;
        assert!(settings.validate().is_err());
        settings.connections = 32;
        assert!(settings.validate().is_ok());
        settings.connections = 33;
        assert!(settings.validate().is_err());
        settings.connections = 8;
        assert!(settings.validate().is_ok());
        settings.min_split_size_mib = 0;
        assert!(settings.validate().is_err());
        settings.min_split_size_mib = 64;
        assert!(settings.validate().is_ok());
        settings.min_split_size_mib = 4;
        settings.concurrent = 11;
        assert!(settings.validate().is_err());
        settings.concurrent = 3;
        settings.speed_limit_kib = 1_048_577;
        assert!(settings.validate().is_err());
        settings.speed_limit_kib = 0;
        settings.refresh_seconds = 0;
        assert!(settings.validate().is_err());
        settings.refresh_seconds = 30;
        assert!(settings.validate().is_ok());
    }
    #[test]
    fn history_checkpoint_prioritizes_state_changes_over_speed_updates() {
        let item = Download {
            gid: "one".into(), source: "https://example.org/file".into(),
            is_torrent: false, name: "file".into(), status: "active".into(),
            completed_length: 100, total_length: 200, download_speed: 10,
            upload_speed: 0, path: "/tmp/file".into(), error: None,
        };
        let mut changed = item.clone();
        changed.download_speed = 99;
        changed.completed_length = 150;
        assert!(!requires_immediate_history_save(&item, &changed));
        changed.status = "complete".into();
        assert!(requires_immediate_history_save(&item, &changed));
        changed.status = "active".into();
        changed.name = "renamed".into();
        assert!(requires_immediate_history_save(&item, &changed));
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
            let rss_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Manager;
                let mut timer = tokio::time::interval(Duration::from_secs(900));
                timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    timer.tick().await;
                    if let Some(engine) = rss_handle.try_state::<Engine>() {
                        poll_feed_subscriptions(&engine).await;
                    } else { break; }
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Manager;
                let mut timer = tokio::time::interval(Duration::from_secs(5));
                timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    timer.tick().await;
                    if let Some(engine) = handle.try_state::<Engine>() {
                        schedule_tick(&engine).await;
                    } else {
                        break;
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_download, add_batch, retry_download, list_downloads, control_download, control_all_downloads, move_queued_download, get_torrent_files, get_torrent_info, select_torrent_files, forget_download, get_settings, save_settings, check_engine, open_download_folder, clear_completed_downloads, clear_finished_downloads, get_schedule, arm_schedule, cancel_schedule, preview_media, start_media_download, retry_media_download, cancel_media_download, control_media_download, media_tools_status, inspect_playlist, inspect_feed, list_feed_subscriptions, subscribe_feed, set_feed_auto_download, remove_feed_subscription, list_media_jobs, forget_media_job, clear_media_history
        ])
        .build(tauri::generate_context!())
        .expect("error while building Download Manager")
        .run(|app_handle, event| {
            // Managed state is not guaranteed to be dropped before process
            // termination. Explicitly reap our aria2 child on normal app exit.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                if let Some(engine) = app_handle.try_state::<Engine>() {
                    engine.shutdown();
                }
            }
        });
}
