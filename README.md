# Download Manager

A Linux-first desktop download manager built with **Tauri 2**, **React**, **TypeScript**, **Rust** and the **aria2** transfer engine. Development initially targets **Ubuntu 26.04.1 LTS**. macOS and Windows are planned.

> Development preview. This is not an IDM-compatible release, and several advanced capabilities are still missing.

## Features available in the code

- Custom dark desktop interface with live transfer activity and speed measurements.
- Enter HTTP, HTTPS, FTP and magnet links (SFTP is not supported by the current aria2 engine).
- Import `.torrent`, `.meta4` and `.metalink` files with a native file picker or full path.
- Torrent file selection dialog with file sizes, per-file progress, tracker URLs and connected peer count (once metadata is available).
- Segmented HTTP(S) transfers when the server supports range requests (through aria2).
- Pause/resume/cancel controls and top/bottom priority actions for waiting downloads, subject to protocol/server support.
- Download history stored locally as JSON, plus aria2's session persistence.
- Settings: connections per server (1–16), concurrent downloads (1–10), overall speed limit, target directory and optional torrent seeding time.
- Transfer status filters and filename/source search.
- Batch link input for 1–50 downloads, with individual error reporting.
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
- On Unix, app exit sends SIGTERM and gives aria2 up to two seconds to save its session; a forced shutdown can still lose progress metadata. Session recovery remains under test.

## Known limitations

- The new aria2 integration still needs a successful Ubuntu 26.04 build and end-to-end tests. GitHub Actions now runs React/Rust checks and a local-only aria2 RPC transfer smoke test, but passing results have not been confirmed.
- Some stopped/completed aria2 entries may not resolve after a fresh engine process; persisted history remains visible.
- Torrent seeding is off by default; set a seeding duration in Settings for newly added torrents. Per-file selection and read-only peer count/tracker details are implemented; editing trackers, peer controls and ratio settings remain future work.
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

## Recent reliability fixes

- Replaced the malformed embedded application icon with a valid PNG.
- Added actionable error when the system cannot launch `aria2c`.
- Persisted cancellation, marked stale jobs interrupted, and added retry controls.
- Reduced disk writes when progress has not changed and avoided retrying non-idempotent RPC calls after ambiguous transport errors.
- Added Rust unit tests for settings limits and download serialization.

## Ubuntu preview QA checklist

The following checks must pass before calling this a release:

1. Run `npm run build`, `cargo check --manifest-path src-tauri/Cargo.toml`, and `cargo test --manifest-path src-tauri/Cargo.toml` successfully.
2. Launch `npm run tauri dev` on Ubuntu 26.04 with `aria2c` available in PATH.
3. Download a small HTTPS file and verify its final bytes/checksum outside the app.
4. Test pause/resume on a server that supports HTTP Range; ensure its final checksum matches.
5. Import a local legal test torrent and a magnet link; verify metadata and payload progress.
6. Restart while a download is in progress and verify session recovery. This remains a known risk.
7. Confirm queue priority, cancel, and no duplicate entries after torrent metadata resolution.
8. Test settings persistence, bad URLs, disk-full errors and missing aria2 gracefully.

These are **pending acceptance tests**, not results already achieved.

## Latest work

- Torrent file selection uses aria2's `getFiles` / `changeOption` RPC methods. At least one file must remain selected; options may cause aria2 to internally restart an active transfer.
- Peer count and announce tracker URLs are visible when the engine provides metadata. Peer IP addresses are not displayed.
- Short-lived RPC transport failures no longer automatically mark a job interrupted. This status is reserved for known missing GIDs.
- Added `tests/test_aria2_rpc.py`, which serves a local file, downloads it through aria2's authenticated RPC and verifies SHA-256; CI runs it on Linux.
- Successful local/CI execution is still required before declaring these changes stable.

## Recent batch download improvements

- Added `add_batch` for up to 50 sources with per-line errors; valid links continue even if others fail.
- Desktop UI has a multiline input for batch queue entry.
- Added source input tests and a safer early-exit check for the aria2 child process.
- These changes are committed but still require a passing Linux build and real-world testing.

## Validation and version consistency (November 2026)

- Fixed malformed Rust character literals that prevented compilation in download source validation.
- New source validation checks supported URI schemes (HTTP, HTTPS, FTP, magnet) and accepts local files only for `.torrent`, `.meta4`, or `.metalink` metadata.
- Added source-validation regression tests for unsupported SFTP, local file metadata, and malformed input.
- Aligned the frontend, Cargo and Tauri application versions to `0.2.0`.
- **Status:** changes have been committed; a green Ubuntu build and end-to-end tests are still required. Do not consider this a release-ready build.

## Reliability improvements

- Backend status checks now poll tracked aria2 GIDs concurrently instead of sequentially.
- The frontend skips overlapping status polls, preventing stale UI responses from piling up.
- Retry is a separate backend action: it refuses to duplicate an existing active/queued/paused transfer after session restoration, then replaces failed history entries when re-enqueue succeeds.
- Local aria2 RPC calls bypass proxy environment variables; RPC still requires a private token on loopback.
- `bash scripts/doctor.sh` reports missing Ubuntu prerequisites without installing or changing anything. The Linux CI workflow runs it before attempting compilation.

**Verification:** These changes are source commits. A passing GitHub Actions run and hands-on Ubuntu tests are still pending; they must not be treated as evidence of a completed release.

## November 2026 recovery and performance update

- RPC status polling now has a concurrency limit of eight jobs, so very large queues do not overwhelm the aria2 control server.
- Retrying a failed or interrupted transfer is blocked when the engine status check fails unexpectedly. This avoids duplicate file transfers when RPC is temporarily unavailable.
- Added a Rust regression test to distinguish confirmed missing GIDs from connection errors.
- Active transfers now display a simple estimated time remaining, based on current download speed and reported total size. It is an estimate, not a guarantee.
- Interrupted transfers are visible in the Failed filter.

The changes require real Ubuntu compilation and restart/transfer tests before they are considered release ready.
