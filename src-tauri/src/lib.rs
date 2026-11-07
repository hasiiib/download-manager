use futures_util::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    id: String,
    received: u64,
    total: Option<u64>,
    status: String,
    file_path: Option<String>,
    error: Option<String>,
}

fn report(app: &AppHandle, id: &str, received: u64, total: Option<u64>, status: &str, path: Option<&str>, error: Option<&str>) {
    let _ = app.emit("download-progress", DownloadProgress {
        id: id.to_string(), received, total, status: status.to_string(),
        file_path: path.map(str::to_string), error: error.map(str::to_string),
    });
}

fn safe_name(url: &url::Url) -> String {
    let candidate = url.path_segments().and_then(|mut x| x.next_back()).unwrap_or("download");
    let decoded = candidate.split('?').next().unwrap_or("download");
    let cleaned: String = decoded.chars().map(|c| {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }
    }).take(120).collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." { "download".to_string() } else { cleaned }
}

async fn run_download(app: AppHandle, id: String, raw_url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&raw_url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS links are supported in this version".into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME environment variable is not set")?;
    let folder = PathBuf::from(home).join("Downloads");
    tokio::fs::create_dir_all(&folder).await.map_err(|e| e.to_string())?;
    let filename = safe_name(&parsed);
    let unique = format!("{}-{}", &id[..id.len().min(8)], filename);
    let final_path = folder.join(unique);
    let partial_path = final_path.with_extension(format!("{}.part", final_path.extension().and_then(|x| x.to_str()).unwrap_or("download")));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3600)).build().map_err(|e| e.to_string())?;
    let response = client.get(parsed).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&partial_path).await.map_err(|e| e.to_string())?;
    use tokio::io::AsyncWriteExt;
    let mut stream = response.bytes_stream();
    let mut received = 0u64;
    report(&app, &id, 0, total, "downloading", None, None);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        report(&app, &id, received, total, "downloading", None, None);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if let Some(expected) = total {
        if expected != received { return Err(format!("Incomplete download: {received}/{expected} bytes")); }
    }
    tokio::fs::rename(&partial_path, &final_path).await.map_err(|e| e.to_string())?;
    report(&app, &id, received, total, "completed", Some(&final_path.display().to_string()), None);
    Ok(())
}

#[tauri::command]
async fn start_download(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let progress_id = id.clone();
    let progress_app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_download(app, id, url).await {
            report(&progress_app, &progress_id, 0, None, "failed", None, Some(&error));
        }
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![start_download])
        .run(tauri::generate_context!())
        .expect("error while running Download Manager");
}
