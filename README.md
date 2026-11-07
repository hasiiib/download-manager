# Download Manager

Ubuntu-first desktop downloader using Tauri 2, Rust, React and TypeScript.

**Status: v0.1.0 development preview.** Only single-connection HTTP/HTTPS downloads are implemented. Pause/resume, multi-connection acceleration, BitTorrent, browser integration and download history persistence are planned, not yet available.

## Requirements (Ubuntu 26.04)

Install Rust using rustup and Node.js 22+ with npm, then install Tauri Linux dependencies:

```bash
sudo apt update && sudo apt install -y build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev libwebkit2gtk-4.1-dev patchelf git
```

## Run

```bash
git clone https://github.com/hasiiib/download-manager.git && cd download-manager && npm install && npm run tauri dev
```

GitHub access is needed since the repository is private.

## Implemented

- Desktop dark UI with search and live transfer progress.
- Direct HTTP(S) download to `~/Downloads`.
- Unique names and `.part` files while transfer is in progress.
- Session-only list of completed/failed downloads.

## Known limitations

- No pause, resume, cancel, scheduling or acceleration yet.
- Downloads may leave `.part` files when interrupted.
- No persistent history or error recovery yet.
- Not yet verified on user's Ubuntu machine.
- This project has no open-source license yet.

## Roadmap

1. Cancellation, resume, persistent download queue (SQLite).
2. Range-request segmentation and acceleration benchmarking.
3. Torrent engine and magnet links.
4. Browser download capture, scheduling and advanced UI.
5. Linux release builds followed by macOS and Windows.

