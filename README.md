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

## Invalid settings recovery (November 2026)

Settings can be syntactically valid JSON yet unusable, such as a saved concurrency limit of zero. App startup now checks settings constraints, falls back to a valid `settings.bak` snapshot if the primary fails validation, and preserves the original for diagnosis. If the primary and backup both fail validation, startup reports the issue instead of silently resetting preferences.

When saving repaired settings, the previous file is eligible to replace `settings.bak` only if it passes both JSON schema decoding and settings value validation. A Rust regression test checks recovery and verifies an invalid primary cannot overwrite the last good backup.

These changes are committed, but the Ubuntu build and aria2 integration suite still lack a confirmed passing run.

## Ubuntu package verification (experimental)

The Linux CI job now inspects any generated `.deb` file before publishing it as an artifact. The offline `scripts/check_deb.py` verifier checks the Debian package name, version, declared `aria2` dependency, executable at `/usr/bin/download-manager`, desktop launcher, and application icon. A separate set of unit tests checks that the verifier rejects missing files or dependencies.

If the build and verification **both pass**, CI uploads the installer plus a `SHA256SUMS` file. After obtaining the artifact, verify integrity from the directory containing both files:

```bash
sha256sum -c SHA256SUMS
```

This checksum protects against accidental corruption after build; it is not a digital signature and does not independently establish publisher authenticity. The package is still experimental, and no green end-to-end Ubuntu build or installation has been confirmed.

**Build reproducibility gap:** the repository currently does not track `package-lock.json` or `src-tauri/Cargo.lock`. Those files should be generated and committed with the exact resolved dependencies before a release, allowing `npm ci` and locked Cargo builds.

## Dependency lockfile preparation

The Linux workflow now archives the dependency lockfiles resolved by npm and Cargo (`package-lock.json` and `src-tauri/Cargo.lock`) as a separate `build-dependency-lockfiles` artifact when the build reaches those stages. This allows maintainers to review the exact versions used in CI and commit them for reproducible future builds. The artifact is for review; committing audited lockfiles and switching to `npm ci` / `cargo --locked` remain future release gates.

The Debian payload verifier also fixes regex escaping for expected package versions and `aria2` dependencies. Basic version/dependency regex cases were checked locally, but this does not establish a full `.deb` build or passing GitHub Actions run. This environment could not reach the npm registry to generate a reliable lockfile, so no manually invented or unverified dependency lock was committed.

## CI dependency resolution and package version checks

- The Ubuntu CI job now resolves the npm dependency tree into a lockfile, installs that exact tree using `npm ci`, generates `src-tauri/Cargo.lock`, and runs Rust checks/tests with `--locked`. Both resolved lockfiles are archived as CI artifacts for inspection; they are **not yet committed to the repository**, so different future CI runs may still select different versions until they are reviewed and checked in.
- `bash scripts/validate-linux.sh` mirrors the same sequence on an Ubuntu development machine. Both operations need package registry access and can fail if it is unavailable.
- The Debian payload checker reads the expected app name and version from `package.json` rather than hardcoding `0.2.0`, and new unit tests reject wrong package names or release versions.

**Release gate remains open:** A real passing GitHub Actions run, reviewed committed lockfiles, and Ubuntu installer launch/download smoke tests are still outstanding.

## Debian archive smoke-test coverage

The package-checker test suite now creates a minimal real Debian archive locally with `dpkg-deb --build`, then asks the project verifier to inspect its metadata and archive paths without mocks. This checks the real `dpkg-deb --fsys-tarfile` inspection path in addition to the existing rejection tests. The fixture is not the application's real installer, and the test does not execute the packaged app. CI runs this suite before Rust compilation; systems without `dpkg-deb` skip just this archive-specific test.

Reproducibility remains incomplete until actual npm/Cargo lockfiles are committed. Neither an installer artifact nor a passing Ubuntu CI result has been confirmed here.

## Streaming installer verification

`scripts/check_deb.py` now streams the Debian package's uncompressed tar entries directly from `dpkg-deb --fsys-tarfile` rather than loading the complete package payload into memory. The error handling reports an invalid archive instead of presenting an unhandled traceback. Regression coverage includes a deliberately corrupted archive and an actual locally assembled Debian fixture.

The streaming inspection and corrupt-archive rejection were exercised locally with `dpkg-deb`. This is **not** a passing Rust compile, Tauri installer build, or successful Ubuntu desktop launch. Real npm and Cargo lockfiles are still missing from the repository, and a green GitHub Actions run is still needed before release.

## Help test the Ubuntu app locally

On an Ubuntu machine with this repository cloned, run:

```bash
cd download-manager
git pull origin main
bash scripts/ubuntu-qa.sh
```

The script requires no `sudo`, installs no operating-system packages, and stops at the first failure. It checks prerequisites, generates the icon, installs frontend npm dependencies, runs the TypeScript/Vite build, checks/tests the Rust backend, and runs the Python configuration and Debian-checker suites. Separate logs are saved under `/tmp/download-manager-qa.*`; the last 25 lines of a failed step appear in the terminal.

Send the failed step's output to the developer, but review it first for passwords, private URLs or tokens. If the script completes, try `npm run tauri -- build --bundles deb`, then share the result and any build error. Running the script alone does not establish that the packaged desktop app launches successfully.

## Ubuntu 26.04 Rust compilation feedback (November 2026)

A contributor's real Ubuntu QA run confirmed that the environment prerequisite, icon-generation, npm dependency, and TypeScript/Vite build steps pass with Node.js 22.22.1 and aria2 1.37.0. The first Rust compile exposed missing reqwest JSON methods and a Tauri setup error-conversion mismatch. The source now enables the `reqwest/json` feature and aligns the `Engine::start` error type with Tauri's setup callback. The QA script prints compiler error summaries when Rust still fails.

The Rust fixes are **not yet confirmed passing**; repeat `git pull origin main && bash scripts/ubuntu-qa.sh` on the Ubuntu machine. If it fails, share the output (after removing any private URLs or credentials). Do not mark the installer ready until Rust compilation, tests, packaging, and desktop launch pass.

## Desktop opens but controls do not work

The desktop UI now actively checks the local aria2 process and its authenticated JSON-RPC connection every ten seconds. The top status indicator shows **aria2 Connected** or **aria2 Disconnected**, and the download form displays the underlying error when the check fails. aria2 stderr is written to `~/.local/share/download-manager/aria2.stderr.log` with owner-only permissions. Do not share the full log without checking it for private URLs, file paths, or tokens.

On Ubuntu, rebuild and install the newest local package after pulling the change:

```bash
git pull origin main
bash scripts/ubuntu-qa.sh
npm run tauri -- build --bundles deb
sudo apt install ./src-tauri/target/release/bundle/deb/*.deb
download-manager
```

If the app still fails, report what happens when pressing **Settings** and **Add download**, the visible error/status indicator, and the output from `tail -n 30 ~/.local/share/download-manager/aria2.stderr.log` after reviewing it for sensitive information. A successful installer build or install does not itself verify downloads or GUI interactions.

## Filter visibility and low-memory preferences

On the first Ubuntu desktop field test, **Completed** was selected with zero completed items. A newly added transfer would therefore stay hidden in this filtered view, even if it had been accepted by aria2. Adding a single or batch download now switches to **All downloads**, clears search, and exposes the new transfer. Empty filtered views explain this and offer **Show all downloads**.

Settings are now organized into six sections modeled on IDM's settings taxonomy: **General**, **Connection**, **Save To**, **Downloads**, **Browser & Files**, and **Proxy & Privacy**. The functional controls are download folder, concurrent jobs, connections per server, speed limit, torrent seed duration, progress refresh interval (1–30 seconds), and Low Memory Mode. Other IDM-inspired categories show clearly marked **Planned** entries rather than misleading non-functional controls. This is not full IDM settings parity.

Low Memory Mode defaults on. It pauses UI status polling when the window/document is hidden, while aria2 continues transfers, and the visible progress refresh defaults to three seconds instead of the old 1.5-second interval. These preferences persist in settings.json and the new fields have backward-compatible defaults. This reduces idle polling overhead but does **not** guarantee a particular RAM usage; that still needs to be measured on Ubuntu.

After pulling the change, rerun `bash scripts/ubuntu-qa.sh`, rebuild the .deb, reinstall, and test **Add download** with the **Completed** filter selected. The next step is to verify actual download progress, history persistence, and basic settings on the Ubuntu machine.

## Real Ubuntu process/RAM findings (November 2026)

An Ubuntu 26.04.1 desktop run passed all eight QA stages, compiled and installed the .deb, and reported approximately **193,636 KiB RSS** for the Tauri/WebKit application and **four aria2c processes**, each at about 15–17 MiB RSS. This is one observed machine, not a controlled memory benchmark, and the processes may include earlier leaked sessions. The app previously relied on Rust managed-state `Drop` to terminate aria2; the desktop now also explicitly calls shutdown on Tauri `RunEvent::Exit`, flushing history and terminating only the child it spawned. Forced termination, crashes, and previously orphaned children are still outside this normal-exit cleanup.

To investigate without exposing aria2 RPC secrets, inspect process names, memory and parent PIDs **without printing full command lines**:

```bash
ps -C download-manager -o pid,ppid,rss,etime,comm
ps -C aria2c -o pid,ppid,rss,etime,comm
```

Close all Download Manager windows normally and rerun both commands. If aria2c processes remain, report their PID/PPID and elapsed times before terminating anything. Do **not** run `pkill aria2c` indiscriminately, since other apps may own some aria2 processes. The app process RSS includes WebKit overhead, which UI polling changes cannot eliminate; meaningful memory optimization requires measuring idle and active cases separately.

## Filter navigation fixed after Ubuntu feedback (November 2026)

The sidebar filters now switch between distinct download views. **All downloads** contains the dashboard totals, add-download form and all transfer rows. **In progress**, **Completed**, **Torrents** and **Failed** display their own heading, description, result count and filtered rows immediately, without making users scroll past dashboard totals. Sidebar selection clears search; empty categories explicitly describe the current view. The totals previously remained visible even after choosing Completed, creating the misleading impression that the click did nothing. Successful download notifications no longer use error styling.

Reference review: [maxuanquang/idm](https://github.com/maxuanquang/idm) is a Go-based HTTP-server-oriented download manager, useful as a conceptual example of download tasks and resume management but not a directly compatible Tauri UI component. [animatlittlechild8/Internet-Download-Manager-Engine](https://github.com/animatlittlechild8/Internet-Download-Manager-Engine) describes a Windows C++20 accelerator and browser integration architecture; its claims and linked external builds were not validated, and no third-party code or executable was imported. We retain our Ubuntu-first Rust/aria2 architecture and evaluate ideas separately.

This navigation patch needs a fresh `bash scripts/ubuntu-qa.sh` and Ubuntu desktop check before marking it verified. Confirm that each sidebar category displays the matching title/count, and that the existing completed test download appears only under **All downloads** and **Completed**.

## Compact dashboard and download sorting

The four dashboard counters now remain in **one compact row** at supported Ubuntu desktop window widths, including widths that previously triggered the two-column rule. Their icons, padding and type scale were reduced. The transfer list also has lightweight in-memory sorting by Recent (original order), Name, File size, and Progress, without extra backend calls or dependencies.

## Single-command Ubuntu developer testing and installation

After pulling the version containing this helper once, close the installed Download Manager window, then use:

```bash
bash ~/download-manager/scripts/update-ubuntu.sh
```

For a machine that has **not yet pulled the helper**, run this **single shell line** for the first use:

```bash
git -C ~/download-manager pull --ff-only origin main && bash ~/download-manager/scripts/update-ubuntu.sh
```

The script checks that the application is closed, pulls `main` with fast-forward-only semantics, runs all the existing Ubuntu QA checks, builds and verifies exactly one `.deb`, reinstalls it via `sudo apt install --reinstall`, and launches the application. Installation may request the local sudo password. Failures print the last 40 lines of that stage and the private temporary log location; no full command-line arguments from aria2 processes are printed. This helper does not automatically close running apps or terminate unrelated aria2 instances. A script syntax check is included in `scripts/ubuntu-qa.sh`.

Release testing still requires checking the new layout, filter navigation and sorting on a real Ubuntu desktop. The helper's full end-to-end workflow has not yet been confirmed on a user's machine.

## Auto-collapsing navigation and dark sorting control

The Ubuntu desktop screenshot showed a bright native GTK select for sorting that clashed with the dark interface. Sort now uses a lightweight HTML details menu styled within the app rather than the native select popup. Recent, Name, File size and Progress remain available without adding a UI library.

The sidebar now starts expanded. Clicking outside it, in the main content area, collapses it into a narrow icon-only rail. Clicking the Download Manager logo/button expands it again. Every category icon in collapsed mode has a numeric badge above its icon, including zero counts. **Failed** turns red when there are failed, interrupted or removed transfers; this indicates the app's recorded status, not just live aria2 errors. Icons retain accessible names and hover titles. Settings remains available while collapsed.

No new timers or background listeners were introduced for the sidebar; the toggle is local React state. This is a UI-only change, so it does not promise lower WebKit process RSS. Use the existing single-command `bash ~/download-manager/scripts/update-ubuntu.sh` (close the app before running) to pull, QA-test, build, verify, reinstall and launch the latest version. Verify the collapsed icon badges and sort menu on Ubuntu before declaring them tested.

## Cleaner desktop workspace and open-folder action

The top breadcrumb/status strip (previously `WORKSPACE / ALL DOWNLOADS` with an aria2 Connected badge) has been removed entirely. The main view starts with its category heading, giving more vertical room to downloads. Backend health is still polled; errors are displayed as an inline warning rather than an always-visible badge.

Completed download rows now include **Open containing folder**, which invokes a new Rust command. On Ubuntu the command checks that the download is marked complete and the file exists, then opens its parent directory with `xdg-open` (no shell interpolation). It never auto-opens downloaded executables or arbitrary unverified URLs. The Rust unit test covers incomplete status, present files, and missing files. Other operating systems currently return an explicit unsupported-platform error.

Close the running app and use `bash ~/download-manager/scripts/update-ubuntu.sh` to pull, run QA, build/verify the installer, reinstall and launch in one command. Verify that the strip is gone, the Completed view shows the new folder icon, and clicking it opens the Ubuntu file manager. The updated backend and full installer are **not yet verified** on the Ubuntu testing machine.

## Live-state download controls (November 2026)

Pause, Resume and Cancel now check each transfer's current aria2 `tellStatus` value before issuing the action. Pausing an already-paused transfer, resuming an already-running transfer, or trying to control a finished/error transfer returns a specific error rather than blindly sending RPC commands. Successful actions immediately update the in-memory status, clear stale transfer speed, and persist history; aria2 session checkpoint failures are logged without falsely reporting that the already-applied control command failed. A Rust unit test covers permitted and rejected transitions.

**Move to top/bottom** now verifies that the transfer is actually `waiting` in aria2. Successful queue actions are reported as changes to aria2 queue position; a separate future change is needed to display aria2's full queue order instead of the current history ordering. Transfer-control feedback and errors now appear beside the download rows, instead of only in the new-download form at the top of the page.

These changes do not introduce new background polling or additional dependencies. Close the installed app and use `bash ~/download-manager/scripts/update-ubuntu.sh` to pull, QA-test, build, verify and reinstall in one command. Real pause/resume, queue and restart behavior still need to be tested on Ubuntu with a slow, resumable download.

## Offline reliability work before the next Ubuntu test (November 2026)

While desktop access is unavailable, history operations were hardened without changing the application dependencies or adding background polling:

- **Forget from history** is now transactional: the app validates the entry, persists a cloned history snapshot with the entry removed, and only then swaps the live in-memory map. If saving fails (for example due to an unwritable directory or full disk), the download remains in the UI and in the previous history file rather than disappearing until restart.
- **Concurrent UI polling** now remembers which GIDs were present when the poll began. If Forget or Retry removes one while aria2 is replying, an old result cannot add the removed GID back into persisted history. Newly discovered child GIDs for magnet/torrent metadata are still eligible for insertion.
- Rust regression tests cover a simulated failed write, active/paused removal rejection, successful history removal, and the distinction between stale and newly discovered GIDs.

The Ubuntu test suite previously passed before these changes, but **the new Rust code has not yet been compiled or run on Ubuntu**. When a laptop is available, close the app and use the existing one-command `bash ~/download-manager/scripts/update-ubuntu.sh` to pull, run QA, build/verify the Debian package, reinstall, and launch. Then verify Forget from history and Retry alongside normal download flows.

## Queue ordering and restart reconciliation (offline development)

- Downloads that were active, waiting or paused before app restart now enter a temporary **recovering** state until the new aria2 session answers `tellStatus`. Missing GIDs become **interrupted** only after an explicit missing-GID response. A transient RPC failure leaves the transfer recovering rather than claiming it is gone, preventing premature duplicate retries.
- The **In progress** filter includes recovering transfers, but avoids displaying transfer controls before the current aria2 state is known.
- Waiting transfers are presented according to aria2's live `tellWaiting` queue order rather than the download-history map's lexical GID order. The list uses **one additional local RPC when waiting transfers exist**, not an RPC per row. If queue lookup fails, the existing display order remains in use. The existing Recent sort label remains the default ordering, with waiting transfers prioritized by queue position.
- Concurrent history removal/retry is reflected in the current list response: a slow status poll can no longer momentarily display a row that has since been deleted from the live history map.
- Rust unit tests were added for queue ordering and recovering-state eligibility.

This development was committed without a local Cargo toolchain or laptop-accessible Ubuntu test. **Passing compilation and real-world queue/session behavior are not yet confirmed.** When desktop access returns, close the app and run `bash ~/download-manager/scripts/update-ubuntu.sh` for the full update/QA/build/installation sequence, then test queued transfers and restart recovery.

## Feature inspiration: Mister Download Manager (November 2026)

Reviewed [anefzaoui/misterdownloadmanager](https://github.com/anefzaoui/misterdownloadmanager), a Rust download manager with published Linux support. Its documented feature set includes a scheduler, browser integration, video downloading via optional tools, RSS feeds, queue operations, themes, language choices, and **Delete All Completed**. These are reference features, not evidence that our implementation supports them. That upstream repository is **GPL-3.0**; no upstream source code, artwork or bundled binary was copied.

As an immediate, low-overhead addition, this project now provides **Clear completed** from the All downloads and Completed views. It asks for confirmation and atomically removes only completed records from our download history JSON; **downloaded files on disk are never deleted**. The action is disabled/hidden when no completed records exist, keeps failed/paused entries, and reports the number removed. Rust regression coverage checks the successful update, no-op behavior and preservation of in-memory/on-disk history after a failed save. No new library, file watcher, process or polling timer is required.

Future candidates from that feature comparison: Pause All/Resume All, a scheduler, browser hand-off, RSS queues, and optional video downloader integration. Each needs separate security, UX and resource-use design and must not be advertised as implemented yet. The new feature is committed but **not yet tested in a full Ubuntu build**, since the tester's laptop is currently unavailable. On return, close the app then run `bash ~/download-manager/scripts/update-ubuntu.sh`.

## Bulk transfer controls

The download list now offers **Pause all** whenever an active/queued transfer is present and **Resume all** whenever a paused transfer is present. Both use aria2's `pauseAll`/`unpauseAll` operations on the app's own authenticated, local aria2 instance. They do not affect unrelated download managers. After a successful RPC, eligible persisted download statuses are immediately updated, speeds reset and history checkpointed. A single `saveSession` request follows each bulk action; errors after an accepted action are logged rather than misleadingly reported as a failed transfer operation. The UI reports the count of history records changed. Completed, failed and recovering records are not overwritten by these bulk history updates.

Tests cover allowed RPC method names, rejected actions, eligible status transitions and preservation of other records. Laptop testing is currently unavailable, so this build is committed but **not yet confirmed with a new Rust compile or interactive Ubuntu test**. When testing resumes, close the installed app and run `bash ~/download-manager/scripts/update-ubuntu.sh`.

## Full feature direction and scheduled downloads (November 2026)

The complete, ongoing all-in-one feature backlog is maintained in [FEATURES.md](FEATURES.md). Requested features are not quietly dropped: acceleration/segmentation benchmarking, queues, browser integration, videos (yt-dlp/ffmpeg), RSS, scheduler, optional system actions, authentication/proxies, categories, desktop notifications, themes/translations, import/export and platform testing remain tracked until genuinely implemented.

### Scheduler, first implementation

The **Scheduler** button in the transfer toolbar now opens a dialog for a local start date/time, **1–10 maximum concurrent downloads**, and one of **Do nothing**, **Shut down computer**, or **Put computer to sleep** after successful completion. Arming it snapshots the currently active/queued/paused downloads into a specific batch, pauses the private aria2 engine and persists the schedule to `~/.local/share/download-manager/schedule.json`. While the app remains open, a backend task checks the schedule every five seconds, updates aria2's concurrency and resumes the queue at the requested time. It monitors the selected GIDs via aria2 and refuses power action if one fails, is unconfirmed, or another active/waiting transfer is still present.

A user-selected power action is delayed **60 seconds** after successful completion, with a visible cancellation option. During the grace period the app re-checks for unfinished transfers. On Linux the action invokes `systemctl poweroff` or `systemctl suspend` as a direct process without a shell. A running or power-pending schedule is marked cancelled in memory after app restart so a reboot/crash cannot silently trigger an old pending power action. The scheduler cannot run when the desktop app is not running; OS-wide scheduling and repeat schedules remain planned. The app's downloads and files are not deleted by cancellation.

**Important:** This is an initial implementation, not a verified release. The UI, Rust compilation, scheduled execution, Linux permissions, desktop shutdown/sleep and reboot/recovery behavior have **not** yet been tested on the user's Ubuntu laptop. Scheduled system actions can interrupt unsaved documents in other applications. They default to **Do nothing** and require explicit selection when arming.

When the Ubuntu machine becomes available, close Download Manager and run the existing single command `bash ~/download-manager/scripts/update-ubuntu.sh` to pull, QA-test, build, reinstall and launch. First test a small schedule with **Do nothing**; only test sleep/shutdown after saving all work and confirming there are no unrelated system tasks.

## Priority update and segmented download tuning

Browser interception and extensions have been moved to the **last implementation priority** by request, without removing them from [FEATURES.md](FEATURES.md). The active priorities are acceleration, video downloads and RSS, scheduler reliability and richer queue management, followed by the remaining desktop features.

### Segmented HTTP acceleration

Connection settings now include **Minimum segment size (MiB)**, configurable from **1 to 64 MiB** (default 4 MiB) alongside **Connections per server** (1–16). The Rust backend validates and persists the setting with backward-compatible defaults for existing settings files, passes it to the app-owned aria2 engine at startup, and applies it to newly enqueued transfers. The frontend exposes it under Settings → Connection & speed. Reducing the minimum split threshold may let aria2 parallelize smaller range-capable downloads; actual speed depends on server support, network and disk, and some hosts are slower with many connections. Existing active jobs are not silently reconfigured.

This is not a proven speed multiplier. Automated range/resume benchmarks and desktop tests remain outstanding; the code must pass CI before being considered validated. No third-party code was copied.

## Connections up to 32 and first optional media download workflow

Settings → Connection & speed now accepts **1–32 connections per server** (previously 1–16). Rust validation rejects 0 or 33+, frontend input range matches, and tests cover the new bounds. It is still not guaranteed that 32 is faster: host limits, rate limiting and HTTP range support determine results.

A first **Video** toolbar action has been implemented using an **optional, separately installed** `yt-dlp`. Users can enter an HTTP/HTTPS page URL, inspect the title/uploader/duration, then start a best-quality video, compatible MP4 or MP3 audio job. The native Rust code invokes the installed tool with argument vectors, not a shell, with URLs separated using `--`. Output defaults to the configured download directory, no software is silently installed, and FFmpeg is needed for some merging/audio jobs. Job status is tracked within the running app and exposed in the media dialog. URL and format validation unit tests are included.

This is the initial media integration, **not yet a full video download manager**: playlist selection, format-specific preview lists, download percentages and speed, job cancellation, surviving app restarts, and automated yt-dlp/FFmpeg checks remain pending in `FEATURES.md`. Users should only download media where permitted. **The newly modified application has not yet been verified by a successful GitHub Actions run or by the Ubuntu laptop.** Close the app and use `bash ~/download-manager/scripts/update-ubuntu.sh` when interactive testing is available.
