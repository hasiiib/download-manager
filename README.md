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

## Session recovery hardening

The automated Ubuntu integration suite now includes `tests/test_aria2_session.py`. It launches aria2 twice against a local temporary session file and checks that a paused queued URI is restored under its original GID. This complements the local HTTP download/checksum test.

The desktop engine now explicitly requests `aria2.saveSession` after accepting downloads, pausing/resuming/cancelling and reordering queued jobs, in addition to periodic automatic saves. Save failures are treated as warnings *after* a download has started, rather than reported as a failed enqueue that users might duplicate.

**Still unverified:** resume of an interrupted partially-downloaded HTTP file and real BitTorrent payload recovery; those need additional tests and a successful Ubuntu CI run. The repository connector has not reported a passing CI check yet.

## HTTP pause/resume verification

The test suite now includes `tests/test_aria2_resume.py`, which runs a deliberately slow **localhost-only** HTTP server that supports byte ranges. The test starts aria2, waits for measurable partial progress, pauses the transfer, resumes the same GID, then verifies all 4 MiB using SHA-256. The server implements both HTTP `HEAD` and `GET` probes.

Run all integration tests on an Ubuntu machine with aria2 installed:

```bash
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

The Linux CI workflow also offers a manual `workflow_dispatch` trigger and executes the same suite after TypeScript and Rust checks. **These are committed tests, not proof they have passed.** A confirmed passing GitHub Actions run and actual desktop testing remain necessary. The suite now also includes `tests/test_aria2_restart_resume.py`: it pauses a partially downloaded local file, saves the aria2 session, completely restarts the engine, resumes the same GID, and compares the final file SHA-256. This test has been committed but a passing run has not yet been confirmed.

## Linux validation command

On a machine with the Ubuntu build prerequisites installed, run the consolidated local validation from the repository root:

```bash
bash scripts/validate-linux.sh
```

It runs the environment doctor, TypeScript build, Rust check and unit tests, Python syntax checks, and **four separately reported** aria2 integration tests (local HTTP checksum, queued-session restoration, in-process pause/resume, and partial-file recovery after engine restart).

The GitHub workflow now isolates each integration test in its own time-limited step for more actionable failure diagnosis. The Tauri engine startup returns thread-safe errors for integration with its setup callback.

**Verification caveat:** these checks have been committed, but no passing Linux CI result or user-machine end-to-end test has been confirmed. Tests should not be reported as passing merely because they exist.

## Ubuntu Debian installer (experimental)

The Linux workflow now attempts a Tauri `.deb` build **after** all code and integration tests pass. If packaging succeeds, GitHub Actions uploads the file as the `download-manager-ubuntu-deb` artifact, retained for 14 days. This is an experimental build artifact, not a published release.

The Debian configuration declares the system package `aria2` as a runtime dependency (Tauri's build does not embed the download engine). The application icon configuration points to the PNG generated by `src-tauri/build.rs`.

Until the workflow reports a successful run and the resulting installer is tested on Ubuntu, the download manager should still be treated as a development preview.

## Linux packaging preparation

- Tauri 2 bundle configuration now uses the correct `bundle.icon` field for the PNG generated by the Rust build script.
- Debian package metadata lists `aria2` as a required system dependency.
- The GitHub Actions Linux job builds an experimental `.deb` after all checks pass and uploads it as a downloadable workflow artifact (14-day retention).
- A new configuration regression test (`tests/test_project_config.py`) validates the icon key, package dependency, and consistent versions before the expensive build.

**Verification:** No passing GitHub Actions result has been observed for this change. The installer is not yet a verified downloadable release.

## Installer CI fixes

- Fixed a guaranteed failing configuration test: the icon build script uses `dir.join("icon.png")`, not `folder.join(...)`.
- Added a dependency-free `scripts/generate_icon.py` step **before** Tauri CLI invokes Rust or validates icon assets. The Rust build script still regenerates the preview icon during compilation.
- Both Linux CI and `bash scripts/validate-linux.sh` generate the icon before building.
- Configuration tests now validate PNG signature, dimensions (64×64 RGBA), chunk order and CRC checksums to catch corrupt generated icons early.

This only verifies the icon-generation logic and configuration in source. A full successful Ubuntu build and installer installation test remain outstanding.

## November 2026 startup and installer checks

- Tauri's RPC HTTP client is now initialized before launching aria2, avoiding an orphaned daemon if client setup fails.
- `npm run tauri dev` and `npm run tauri build` first generate the icon through npm's `pretauri` hook. Direct `npx tauri build` in CI is preceded by its own explicit icon step.
- Installer CI verifies that exactly one `.deb` was generated and that the package metadata declares the `aria2` dependency before uploading an artifact.
- Added a configuration regression test to keep the local icon preflight wired up.
- The icon-generator algorithm was checked in isolation and its 64×64 RGBA PNG parsed successfully. This does **not** establish that the full app or installer builds.

**Outstanding:** A green Ubuntu GitHub Actions run, installation on Ubuntu 26.04, and a full desktop download/resume smoke test.

## History polling and retry guard improvements

- Completed, removed and failed history records are now served from local history rather than each generating an aria2 status RPC request every 1.5 seconds. Active, waiting, paused and interrupted jobs are still checked against the engine.
- Retry checks now reject already-complete transfers and unrecognized aria2 states, in addition to running or paused transfers, to reduce duplicate downloads.
- The partial-file restart integration test only sends `unpause` when aria2 actually reports a paused transfer; restored waiting transfers are allowed to run naturally.
- Added Rust unit coverage for the distinction between live and historical status polling.

The changes above have been committed, but the Linux CI connector has not provided a passing build result. The app remains a development preview.

## History checkpointing improvements

The transfer-status screen refreshes roughly every 1.5 seconds, but persisting the entire JSON history for every changed byte counter created excessive write amplification with active downloads. The Rust backend now checkpoints progress-only changes at most once every ten seconds, while immediately saving lifecycle changes (new/removed jobs, status, source, torrent identification, path, filename or errors). The latest in-memory history is flushed again during normal engine teardown. aria2 maintains its own session file separately.

A Rust unit test checks that speed/byte changes are throttled while status and filename changes trigger immediate persistence. Unexpected process termination or power loss may still lose progress metadata from the most recent few seconds; this is not a guarantee of crash-proof persistence.

**Verification status:** These are source changes. Full Ubuntu compilation, download integration tests and installer checks still require a successful CI run.

## Private and durable local state (November 2026)

- On Linux/other Unix systems the application data directory is restricted to mode `0700`. Persisted settings and download-history JSON files are written with mode `0600`, so other local users cannot read private source URLs and metadata through normal file permissions.
- JSON persistence now writes to a temporary file, flushes file contents with `sync_all`, atomically renames it, then syncs the containing directory on Unix. The last committed JSON file is not truncated by an incomplete replacement write.
- A missing settings/history file still means first launch, but malformed or unreadable JSON now produces a visible startup error instead of silently discarding the saved list or restoring default settings. Users should preserve corrupt files for recovery rather than delete them without a backup.
- A Rust regression test checks replacement saves, missing-vs-corrupt reads, and Unix file permissions.

These changes are committed, but runtime validation on Ubuntu and a passing Linux build are still required. Atomic filesystem replacement reduces damage from interrupted writes; it does not guarantee safety against failing storage hardware.

## Recovery from corrupted local JSON state

Each successful overwrite of `downloads.json` and `settings.json` now retains the previously committed, syntactically valid JSON document as a private `.bak` file (for example, `downloads.bak`). Both the backup and the primary file use atomic temporary-file replacement. On startup, if the primary document is malformed or cannot deserialize, the application attempts to load the last valid backup and logs the recovery. The corrupt primary is left untouched until the next successful save, so it can be inspected if necessary.

If both copies are invalid, startup still fails with an explicit error instead of erasing the user's download history. Backup files contain potentially private download URLs and are protected by the same restricted Unix file permissions as the main JSON files. The Rust tests cover valid backup creation, recovery, preservation of a good backup after corruption, and the failure case where both copies are bad.

**Limits:** the backup is a previous snapshot and can be a few seconds behind the newest state. This mechanism does not repair aria2 session files, nor does it replace an off-device backup. The Ubuntu Rust build and tests still require a verified successful run.

## Additional backup recovery safeguards

- If the main settings or download-history JSON file has disappeared but a `.bak` snapshot exists, startup now restores from the backup instead of treating it as a fresh installation. A missing primary plus an invalid backup produces an explicit error.
- A syntactically valid JSON file of the wrong document type (for example, an array where a download-history map is required) is no longer considered eligible to replace the last valid backup. The backup must deserialize as the same document type being saved.
- Rust regression coverage now includes missing-primary recovery, invalid backup contents, and incompatible JSON document structures.

**Verification:** These safeguards are committed but the repository's Rust compilation and Ubuntu installer workflow have not yet returned a confirmed passing status. Do not regard them as proven until CI and real-machine restart tests pass.
