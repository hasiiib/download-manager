# All-in-one Download Manager feature ledger

This is the enduring backlog. **Planned is not implemented.** Existing features remain in scope until they are delivered and verified on a real desktop.

## Priority order (updated)
1. Download acceleration, segmented connection controls, real HTTP range/resume benchmarks and server compatibility.
2. Video/audio/playlist downloading with opt-in yt-dlp and ffmpeg, RSS, media extraction.
3. Queue groups, recurring scheduler, download concurrency profiles, shutdown/sleep correctness.
4. Download categories, authentication/proxies, hash checks, import/export, themes and translations.
5. Installer polish, native desktop notifications, platform and accessibility tests.
6. **Browser integration and interception: deliberately last priority, still in scope.** Firefox/Chromium extensions are not removed from the roadmap.

This ordering is for implementation priority only. No requested feature is dropped.

## Core transfers and reliability
- [x] Direct HTTP/HTTPS/FTP downloads through aria2
- [x] Torrent, magnet and metalink ingestion
- [x] Per-download pause, resume, cancel, retry
- [x] Pause All / Resume All
- [x] Queue top/bottom and live queue ordering
- [x] Batch link submission
- [x] Download history and completed-history cleanup
- [x] Atomic history/settings writes with backup recovery
- [ ] Verify resumed, interrupted and torrent downloads end-to-end after restart on Ubuntu
- [ ] Download accelerator controls with benchmarks: segmented connections and minimum segment-size tuning now committed (unverified); per-host and global limits, adaptive tuning, measured speed gains and reliable range fallback remain to build/test
- [ ] Batch rename, move and duplicate detection with overwrite policies
- [ ] File integrity verification, hashes and optional malware scanning
- [ ] Queue groups, priorities, simultaneous transfer profiles and scheduling
- [ ] Proxy profiles, per-site authentication, headers/cookies and site exclusions

## Scheduling and power
- [x] Arm a download queue for a chosen local time and maximum simultaneous transfers (code committed, Ubuntu verification pending)
- [x] Track scheduled transfer completion and allow schedule cancellation (code committed, Ubuntu verification pending)
- [x] On verified completion, user-selectable None / Shutdown / Sleep with an explicit warning and 60-second cancellation grace period (Linux code committed, not tested)
- [ ] Repeat schedules, scheduled start/stop windows and bandwidth profiles
- [ ] Start-at-login / tray support and recovery if application exits before scheduled time

## Media, RSS and sources (high priority)
- [ ] yt-dlp and ffmpeg integration: optional yt-dlp metadata inspection and single-item MP4/best-video/MP3 job launching implemented provisionally; playlist support, progress, cancellation, FFmpeg detection and persistent media jobs still pending
- [ ] RSS subscriptions, filters and automatic episode/file downloads
- [ ] HTTP link grabber and downloadable asset discovery
- [ ] Cloud/service integrations and optional accounts where feasible

## Desktop and user experience
- [x] Compact sidebar with counts and failure highlighting
- [x] All / In progress / Completed / Torrents / Failed categories
- [x] Sort/search and file-manager open for completed downloads
- [ ] Download detail/progress window, notifications, completion sound
- [ ] Categories by file type and per-category save locations
- [ ] Theme switching (system, light, dark, high contrast), UI scaling and keyboard shortcuts
- [ ] Bengali and other translations
- [ ] Export/import download history and settings
- [ ] Cross-platform installers and platform-specific integrations
- [ ] Automated browser, UI and accessibility testing
- [ ] Measured resource use and performance/throughput comparisons

## Validation rule
Each entry is complete only after working code, tests, security review, and where applicable interactive platform verification. Offline commits are provisional. Never silently drop an earlier request. Third-party repositories are inspiration only; honor their licenses rather than copying GPL code into this project unreviewed.

## Browser integration (last implementation priority, not cancelled)
- [ ] Browser extensions for Firefox, Chromium, Chrome, Edge and Brave via authenticated Native Messaging
- [ ] Download interception / URL handoff with user control and exclusions
- [ ] Browser cookies and authenticated transfers with consent and site exclusions
