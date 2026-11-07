# Download Manager

A Linux-first desktop download manager built with **Tauri 2**, **React**, **TypeScript**, **Rust** and the **aria2** transfer engine. Development initially targets **Ubuntu 26.04.1 LTS**. macOS and Windows are planned.

> Development preview. This is not an IDM-compatible release, and several advanced capabilities are still missing.

## Features available in the code

- Custom dark desktop interface with live transfer activity and speed measurements.
- Enter HTTP, HTTPS, FTP, SFTP and magnet links.
- Import `.torrent`, `.meta4` and `.metalink` files with a native file picker or full path.
- Segmented HTTP(S) transfers when the server supports range requests (through aria2).
- Pause/resume/cancel controls, subject to protocol/server support.
- Download history stored locally as JSON, plus aria2's session persistence.
- Settings: connections per server (1–16), concurrent downloads (1–10), overall speed limit, target directory.
- Transfer status filters and filename/source search.
- aria2 RPC bound to localhost with a random per-launch authentication token.

## Ubuntu requirements

This app **requires the `aria2c` executable** on your PATH.

```bash
sudo apt update && sudo apt install -y git aria2 build-essential curl wget file pkg-config libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

Install Node.js 22+ and npm, plus a modern Rust toolchain using rustup. The Tauri dialog plugin requires Rust 1.90 or newer; check with `rustc --version`.

## Development

```bash
git clone https://github.com/hasiiib/download-manager.git
cd download-manager
npm install
npm run tauri dev
```

If you have cloned this private repository previously, use `git pull origin main` rather than cloning again.

## Storage

- Downloads: configured directory, initially `~/Downloads`.
- Settings/history/aria2 session: `~/.local/share/download-manager`.
- This preview uses an external `aria2c` process. A packaged release must ship or install an engine appropriately.
- aria2 stops when the desktop app exits. Its session is periodically saved; interrupted jobs should be resumable depending on server and protocol capabilities.

## Known limitations

- Not yet verified by a successful Ubuntu 26.04 build of the new aria2 integration.
- Some stopped/completed aria2 entries may not resolve after a fresh engine process; persisted history remains visible.
- Torrent seeding is disabled for now (`--seed-time=0`); file selection, peer details, tracker management and ratio settings remain future work.
- No browser extensions, automatic video detection, site grabber, scheduler, checksum verification, tray controls, or installed packages yet.
- DRM-protected streams are not supported.
- No automatic link extraction from arbitrary web pages. A stream URL (e.g. a manifest requiring cookies or auth) is not necessarily a direct download.
- The aria2 executable is a separate GPL-licensed project. Packaging and distribution will require a license review.

## Roadmap

1. Engine reliability: build verification, automatic reconnect, shutdown persistence, queue controls and tests.
2. Torrent details and options: selected files, trackers, metadata progress and seeding controls.
3. Categories, batch URLs, schedules and automatic file organization.
4. Browser integration through extensions and native messaging.
5. Video detection for supported non-DRM content, proxy/cookies, authentication and speed benchmarks.
6. Ubuntu packaging/release workflow; then macOS and Windows.

## Acknowledgements

IDM's public feature documentation is used only as a functional reference. This project does not reuse IDM branding or proprietary code. aria2 is a separate open-source download engine.

## Licensing

No license has yet been selected for this application. Review library and engine licenses before any distribution.
