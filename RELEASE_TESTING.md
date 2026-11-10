# Ubuntu release-night acceptance checklist

This document is a manual test plan. **A passing CI does not prove the desktop GUI, external services or power actions work on the user's laptop.** Test as a normal desktop user.

## Install & launch
1. Close the application.
2. From `~/download-manager`, run `bash scripts/update-ubuntu.sh`.
3. Confirm all QA steps pass, one .deb is verified and installed, and the Download Manager window opens.
4. If the updater fails, keep the full terminal text and the printed build log path. Do not repeatedly reinstall packages without reading the failure.
5. Check Settings > Connection & speed. Try requested segments **8** first, with 3 concurrent downloads and no speed limit. The per-server cap of stock aria2 remains **16**.

## Essential download flow
1. Add a small, public, non-sensitive HTTPS file you are allowed to download. Verify its size, destination and completed status.
2. Add a larger legitimate file, Pause, then Resume. Verify that progress continues.
3. Queue several files, move one in the waiting list, then test Pause All / Resume All.
4. Close and reopen the app. Confirm the in-progress/finished history reconciles, and no duplicate download starts.
5. Test retry on a *failed* download and Clear Completed History (should preserve actual downloaded files).

## Torrents and metadata
1. Test a legal/public torrent or magnet (for example a distribution's officially published Linux ISO). Do not use private trackers or personal credentials.
2. Check metadata, file selection, peers and trackers; pause/resume and cancel safely.
3. A metadata-only torrent completion may create separate payload transfers. Confirm the app does not consider that an idle queue while payloads are active.

## Video / audio
1. Install/check `yt-dlp` and `ffmpeg` yourself; opening the Video dialog should report whether both are available.
2. Inspect a permitted public single-video URL; select MP4 or 720p and download.
3. Observe progress, ETA, Cancel, Pause/Resume; note the actual result if an FFmpeg merge is needed.
4. Inspect a public playlist (up to first 50 entries), select two entries, and check only those files download. Overall progress is an estimate, not a confirmed byte-weighted total.
5. Close and reopen the app; an interrupted media entry should be labelled Interrupted, with Retry available. A formerly running child process must not be left behind.

## RSS
1. Open RSS feed dialog; inspect a public RSS/Atom feed that has downloadable enclosures.
2. Manually queue an enclosure and confirm it appears in the download list.
3. Save one feed, confirm Auto is **off** by default. Enabling Auto checks for new enclosures around every 15 minutes while the app stays open, not in background.
4. Remove the saved subscription without deleting downloaded files.

## Scheduler: start with zero risk
1. Queue a harmless, small file and test a future schedule (e.g., 5 minutes ahead) with After Complete = **Do nothing**.
2. Confirm transfers pause while armed, resume near the chosen time, and eventually show Completed.
3. Test Cancel on a second schedule. Close/reopen after arming one and confirm it is cancelled for safety. Recovered aria2 sessions might resume immediately.
4. **Do not initially test Shutdown or Sleep.** They could interrupt unsaved work or other system activity. Only test after completing all preceding steps, saving work, and intentionally choosing that action.

## Reporting
Share the first failing step, terminal output (last 30–40 lines), exact error text from the app and your Ubuntu version. Avoid sending tokens, passwords, private download URLs or sensitive contents. If the UI freezes, report what was clicked immediately beforehand.

This checklist is not evidence that any test has passed. Keep unresolved results in FEATURES.md and record failures before describing a release as stable.
