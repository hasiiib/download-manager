import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Activity, ArrowDownToLine, ArrowUpToLine, ArrowDownToLine as QueueBottomIcon, CheckCircle2, Clock3, Download, FolderOpen, HardDrive,
  Link2, Magnet, Pause, Play, Plus, Search, Settings2, ShieldCheck, SlidersHorizontal, Video,
  Trash2, X, XCircle, Zap
} from "lucide-react";

type DownloadItem = {
  gid: string;
  source: string;
  isTorrent: boolean;
  name: string;
  status: string;
  completedLength: number;
  totalLength: number;
  downloadSpeed: number;
  uploadSpeed: number;
  path: string;
  error?: string | null;
};
type TorrentFile = { index: number; path: string; length: number; completedLength: number; selected: boolean };
type TorrentInfo = { peers: number; trackers: string[] };
type Settings = {
  connections: number;
  minSplitSizeMib: number;
  concurrent: number;
  speedLimitKib: number;
  seedMinutes: number;
  downloadDir: string;
  refreshSeconds: number;
  lowMemoryMode: boolean;
};
type Filter = "all" | "active" | "complete" | "error" | "torrent";
type SortOrder = "default" | "name" | "size" | "progress";
type MediaFormat = { id: string; extension: string; resolution: string; note: string; filesize: number | null };
type FeedSubscription = { id: string; source: string; title: string; autoDownload: boolean; seenSources: string[]; lastCheckedUnix: number; error: string | null };
type FeedPreview = { title: string; entries: { title: string; source: string; enclosure: boolean }[] };
type PlaylistPreview = { title: string; entries: { index: number; title: string; durationSeconds: number | null }[] };
type MediaPreview = { title: string; durationSeconds: number | null; uploader: string; webpageUrl: string; ytDlpVersion: string; formats: MediaFormat[] };
type MediaJob = { id: string; title: string; status: string; format: string; playlist: boolean; playlistItems: number[]; error: string | null; progress: number | null; speed: string; eta: string; currentItem: string; playlistIndex: number | null; overallProgress: number | null };
type MediaTools = { ytDlp: string | null; ffmpeg: string | null };

type Schedule = {
  state: "idle" | "armed" | "running" | "power_pending" | "completed" | "cancelled" | "error";
  startAtUnix: number;
  concurrent: number;
  afterComplete: "none" | "shutdown" | "sleep";
  gids: string[];
  powerAtUnix: number;
  message: string;
};

const viewTitles: Record<Filter, string> = {
  all: "All downloads",
  active: "In progress",
  complete: "Completed",
  error: "Failed",
  torrent: "Torrents",
};
const viewDescriptions: Record<Filter, string> = {
  all: "Manage files, videos, audio and torrents in one place.",
  active: "Active, queued and paused transfers.",
  complete: "Successfully finished downloads.",
  error: "Failed, removed or interrupted transfers.",
  torrent: "Your magnet and BitTorrent downloads.",
};
type SettingsTab = "general" | "connection" | "save" | "downloads" | "integration" | "privacy";
const defaults: Settings = { connections: 8, minSplitSizeMib: 4, concurrent: 3, speedLimitKib: 0, seedMinutes: 0, downloadDir: "", refreshSeconds: 3, lowMemoryMode: true };
const formatSize = (n = 0) => n < 1024 ? n + " B" : n < 1048576 ? (n / 1024).toFixed(1) + " KB" : n < 1073741824 ? (n / 1048576).toFixed(1) + " MB" : (n / 1073741824).toFixed(2) + " GB";
const formatEta = (seconds: number) => {
  if (!Number.isFinite(seconds) || seconds < 0) return "Calculating...";
  if (seconds < 60) return Math.ceil(seconds) + " sec left";
  if (seconds < 3600) return Math.ceil(seconds / 60) + " min left";
  if (seconds < 86400) return Math.floor(seconds / 3600) + " hr " + Math.ceil(seconds % 3600 / 60) + " min left";
  return "Over a day left";
};
const isTorrent = (item: DownloadItem) => item.isTorrent || item.source.startsWith("magnet:") || /\.(torrent)(\?|$)/i.test(item.source);
const activeState = (s: string) => ["active", "waiting", "paused", "recovering"].includes(s);
const filtered = (d: DownloadItem, filter: Filter) => filter === "all" ||
  (filter === "active" && activeState(d.status)) ||
  (filter === "complete" && d.status === "complete") ||
  (filter === "error" && ["error", "removed", "interrupted"].includes(d.status)) ||
  (filter === "torrent" && isTorrent(d));

function StatusIcon({ status }: { status: string }) {
  if (status === "complete") return <CheckCircle2 size={13} />;
  if (status === "error" || status === "removed" || status === "interrupted") return <XCircle size={13} />;
  if (status === "active") return <Activity size={13} />;
  if (status === "recovering") return <Clock3 size={13} />;
  return <Clock3 size={13} />;
}

export default function App() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [url, setUrl] = useState("");
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState("");
  const [batchError, setBatchError] = useState("");
  const [query, setQuery] = useState("");
  const [sortOrder, setSortOrder] = useState<SortOrder>("default");
  const [filter, setFilter] = useState<Filter>("all");
  const [sidebarExpanded, setSidebarExpanded] = useState(true);
  const [settings, setSettings] = useState<Settings>(defaults);
  const [draft, setDraft] = useState<Settings>(defaults);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mediaOpen, setMediaOpen] = useState(false);
  const [newDownloadOpen, setNewDownloadOpen] = useState(false);
  const [mediaUrl, setMediaUrl] = useState("");
  const [mediaFormat, setMediaFormat] = useState("mp4");
  const [mediaPlaylist, setMediaPlaylist] = useState(false);
  const [feedOpen, setFeedOpen] = useState(false);
  const [feedUrl, setFeedUrl] = useState("");
  const [feedPreview, setFeedPreview] = useState<FeedPreview | null>(null);
  const [feedSubscriptions, setFeedSubscriptions] = useState<FeedSubscription[]>([]);
  const [feedSelection, setFeedSelection] = useState<number[]>([]);
  const [feedBusy, setFeedBusy] = useState(false);
  const [feedError, setFeedError] = useState("");
  const [playlistPreview, setPlaylistPreview] = useState<PlaylistPreview | null>(null);
  const [selectedPlaylistItems, setSelectedPlaylistItems] = useState<number[]>([]);
  const [mediaPreview, setMediaPreview] = useState<MediaPreview | null>(null);
  const [mediaJobs, setMediaJobs] = useState<MediaJob[]>([]);
  const [mediaTools, setMediaTools] = useState<MediaTools | null>(null);
  const [mediaBusy, setMediaBusy] = useState(false);
  const [mediaError, setMediaError] = useState("");
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [schedule, setSchedule] = useState<Schedule | null>(null);
  const [scheduleStart, setScheduleStart] = useState("");
  const [scheduleConcurrency, setScheduleConcurrency] = useState(3);
  const [scheduleAction, setScheduleAction] = useState<Schedule["afterComplete"]>("none");
  const [scheduleError, setScheduleError] = useState("");
  const [queueOrder, setQueueOrder] = useState<string[]>([]);
  const [draggedQueueId, setDraggedQueueId] = useState<string | null>(null);

  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [torrentDialog, setTorrentDialog] = useState<DownloadItem | null>(null);
  const [torrentFiles, setTorrentFiles] = useState<TorrentFile[]>([]);
  const [torrentInfo, setTorrentInfo] = useState<TorrentInfo | null>(null);
  const [selectedFiles, setSelectedFiles] = useState<number[]>([]);
  const [torrentError, setTorrentError] = useState("");
  const [torrentBusy, setTorrentBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [actionFeedback, setActionFeedback] = useState("");
  const [confirmClearCompleted, setConfirmClearCompleted] = useState(false);
  const [working, setWorking] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const [engineHealth, setEngineHealth] = useState("Checking engine connection...");
  const [engineOnline, setEngineOnline] = useState(false);
  const [settingsError, setSettingsError] = useState("");
  const refreshing = useRef(false);
  const selectView = (view: Filter) => {
    setFilter(view);
    setQuery("");
  };

  const refresh = useCallback(async () => {
    // Ignore overlapping polls instead of allowing slower responses to overwrite fresh data.
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const list = await invoke<DownloadItem[]>("list_downloads");
      setItems(list);
      void invoke<Schedule>("get_schedule").then(setSchedule).catch(() => {});
      void invoke<MediaJob[]>("list_media_jobs").then(setMediaJobs).catch(() => {});
      setRefreshError("");
    } catch (err) {
      setRefreshError(String(err));
    } finally {
      refreshing.current = false;
    }
  }, []);

  useEffect(() => {
    void refresh();
    invoke<Settings>("get_settings").then(s => { setSettings(s); setDraft(s); }).catch(e => setMessage(String(e)));
    let mounted = true;
    const check = async () => {
      try {
        const version = await invoke<string>("check_engine");
        if (mounted) { setEngineHealth("aria2 " + version + " connected"); setEngineOnline(true); }
      } catch (error) {
        if (mounted) { setEngineHealth("Engine unavailable: " + String(error)); setEngineOnline(false); }
      }
    };
    void check();
    const intervalMs = Math.max(1000, Math.min(30000, settings.refreshSeconds * 1000));
    const timer = window.setInterval(() => {
      if (!settings.lowMemoryMode || !document.hidden) void refresh();
    }, intervalMs);
    const healthTimer = window.setInterval(() => void check(), 10000);
    return () => { mounted = false; window.clearInterval(timer); window.clearInterval(healthTimer); };
  }, [refresh, settings.refreshSeconds, settings.lowMemoryMode]);

  const pickTorrent = async () => {
    try {
      const file = await open({ multiple: false, directory: false, filters: [{ name: "Torrent or Metalink", extensions: ["torrent", "meta4", "metalink"] }] });
      if (typeof file === "string") setUrl(file);
    } catch (err) { setMessage(String(err)); }
  };

  const pickDirectory = async () => {
    try {
      const folder = await open({ multiple: false, directory: true });
      if (typeof folder === "string") setDraft(s => ({ ...s, downloadDir: folder }));
    } catch (err) { setMessage(String(err)); }
  };

  const looksLikeMedia = /^(https?:\/\/)?(www\.|m\.)?(youtube\.com|youtu\.be|vimeo\.com|tiktok\.com|instagram\.com)\//i.test(url.trim());
  const openMediaFromLink = () => {
    setMediaUrl(url.trim()); setMediaPreview(null); setPlaylistPreview(null);
    setMediaError(""); setMediaOpen(true);
  };

  const add = async () => {
    if (!url.trim()) { setMessage("Paste a URL, magnet link or full path to a .torrent/.meta4 file."); return; }
    if (looksLikeMedia) { openMediaFromLink(); return; }
    setMessage("");
    setWorking(true);
    try {
      await invoke("add_download", { source: url.trim() });
      setFilter("all");
      setQuery("");
      setMessage("Download added. Showing all downloads.");
      setUrl("");
      setNewDownloadOpen(false);
      await refresh();
    } catch (err) { setMessage(String(err)); }
    finally { setWorking(false); }
  };

  const addBatch = async () => {
    const sources = batchText.split(/\r?\n/).map(line => line.trim()).filter(Boolean);
    if (sources.length === 0 || sources.length > 50) {
      setBatchError("Enter between 1 and 50 links, one per line.");
      return;
    }
    setWorking(true);
    setBatchError("");
    try {
      const result = await invoke<{ added: number; errors: string[] }>("add_batch", { sources });
      await refresh();
      if (result.added > 0) { setFilter("all"); setQuery(""); }
      if (result.errors.length) {
        setBatchError(result.added + " added. " + result.errors.join(" | "));
      } else {
        setBatchOpen(false);
        setBatchText("");
        setMessage(result.added + " downloads queued.");
      }
    } catch (err) { setBatchError(String(err)); }
    finally { setWorking(false); }
  };

  const showTorrentFiles = async (item: DownloadItem) => {
    setTorrentDialog(item);
    setTorrentFiles([]);
    setTorrentInfo(null);
    setSelectedFiles([]);
    setTorrentError("");
    setTorrentBusy(true);
    try {
      const files = await invoke<TorrentFile[]>("get_torrent_files", { gid: item.gid });
      setTorrentFiles(files);
      setSelectedFiles(files.filter(file => file.selected).map(file => file.index));
      invoke<TorrentInfo>("get_torrent_info", { gid: item.gid }).then(setTorrentInfo).catch(() => {});
    } catch (err) { setTorrentError(String(err)); }
    finally { setTorrentBusy(false); }
  };

  const saveTorrentFiles = async () => {
    if (!torrentDialog) return;
    setTorrentBusy(true);
    setTorrentError("");
    try {
      await invoke("select_torrent_files", { gid: torrentDialog.gid, indexes: selectedFiles });
      setTorrentDialog(null);
      await refresh();
    } catch (err) { setTorrentError(String(err)); }
    finally { setTorrentBusy(false); }
  };

  const moveQueue = async (gid: string, direction: "top" | "bottom") => {
    setActionFeedback("");
    try { await invoke("move_queued_download", { gid, direction }); await refresh(); setActionFeedback("Queue position updated in aria2."); }
    catch (err) { setActionFeedback(String(err)); }
  };

  const retry = async (item: DownloadItem) => {
    setActionFeedback("");
    try { await invoke("retry_download", { gid: item.gid }); await refresh(); setActionFeedback("Transfer retried."); }
    catch (err) { setActionFeedback(String(err)); }
  };

  const control = async (gid: string, action: "pause" | "resume" | "cancel") => {
    setActionFeedback("");
    try { await invoke("control_download", { gid, action }); await refresh(); setActionFeedback("Transfer " + (action === "cancel" ? "cancelled" : action === "resume" ? "resumed" : "paused") + "."); }
    catch (err) { setActionFeedback(String(err)); }
  };

  const controlAll = async (action: "pause" | "resume") => {
    setWorking(true);
    setActionFeedback("");
    try {
      const count = await invoke<number>("control_all_downloads", { action });
      await refresh();
      setActionFeedback((action === "pause" ? "Pause All" : "Resume All") +
        " completed. " + count + " recorded transfer" + (count === 1 ? "" : "s") + " updated.");
    } catch (error) {
      setActionFeedback(String(error));
    } finally {
      setWorking(false);
    }
  };

  const openFolder = async (gid: string) => {
    setActionFeedback("");
    try {
      await invoke("open_download_folder", { gid });
    } catch (error) {
      setActionFeedback(String(error));
    }
  };

  const forget = async (gid: string) => {
    setActionFeedback("");
    try { await invoke("forget_download", { gid }); await refresh(); setActionFeedback("Download removed from history."); }
    catch (err) { setActionFeedback(String(err)); }
  };

  const forgetMedia = async (id: string) => {
    setActionFeedback("");
    try {
      await invoke("forget_media_job", { id });
      setMediaJobs(jobs => jobs.filter(job => job.id !== id));
      setActionFeedback("Media history entry removed. Downloaded file was kept.");
    } catch (error) { setActionFeedback(String(error)); }
  };

  const clearAllHistory = async () => {
    setWorking(true);
    setActionFeedback("");
    try {
      const removed = await invoke<number>("clear_media_history");
      const media = await invoke<MediaJob[]>("list_media_jobs");
      setMediaJobs(media);
      const completed = await invoke<number>("clear_completed_downloads");
      await refresh();
      setActionFeedback(`${removed + completed} history entries cleared. Downloaded files were kept. Active media downloads were not removed.`);
    } catch (error) { setActionFeedback(String(error)); }
    finally { setWorking(false); }
  };

  const clearCompleted = async () => {
    setWorking(true);
    setActionFeedback("");
    try {
      const removed = await invoke<number>("clear_completed_downloads");
      setConfirmClearCompleted(false);
      await refresh();
      setActionFeedback(removed + " completed history entr" + (removed === 1 ? "y" : "ies") + " cleared. Files were not deleted.");
    } catch (error) {
      setActionFeedback(String(error));
      setConfirmClearCompleted(false);
    } finally {
      setWorking(false);
    }
  };

  useEffect(() => {
    if (!mediaOpen) return;
    let active = true;
    void invoke<MediaTools>("media_tools_status")
      .then(status => { if (active) setMediaTools(status); })
      .catch(() => { if (active) setMediaTools(null); });
    return () => { active = false; };
  }, [mediaOpen]);

  useEffect(() => {
    let mounted = true;
    const poll = () => {
      void invoke<MediaJob[]>("list_media_jobs")
        .then(jobs => { if (mounted) setMediaJobs(jobs); })
        .catch(() => {});
    };
    poll();
    const timer = window.setInterval(poll, 1500);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  const retryMedia = async (id: string) => {
    setMediaError("");
    try {
      const result = await invoke<MediaJob>("retry_media_download", { id });
      setMediaJobs(jobs => [...jobs, result]);
      setMediaError("Retry started. Existing partial files may be reused.");
    } catch (error) {
      setMediaError(String(error));
    }
  };

  const controlMedia = async (id: string, action: "pause" | "resume") => {
    setMediaError("");
    try {
      await invoke("control_media_download", { id, action });
      setMediaError(action === "pause" ? "Pause requested." : "Resume requested.");
    } catch (error) {
      setMediaError(String(error));
    }
  };

  const cancelMedia = async (id: string) => {
    setMediaError("");
    try {
      await invoke("cancel_media_download", { id });
      setMediaError("Cancellation requested. The media process will stop shortly.");
    } catch (error) {
      setMediaError(String(error));
    }
  };

  useEffect(() => {
    if (!feedOpen) return;
    void invoke<FeedSubscription[]>("list_feed_subscriptions")
      .then(setFeedSubscriptions).catch(() => {});
  }, [feedOpen]);

  const subscribeFeed = async () => {
    setFeedBusy(true);
    setFeedError("");
    try {
      const result = await invoke<FeedSubscription>("subscribe_feed", { source: feedUrl.trim() });
      setFeedSubscriptions(items => [...items, result]);
      setFeedError("Feed saved. Automatic downloading is off until you enable it below.");
    } catch (error) { setFeedError(String(error)); }
    finally { setFeedBusy(false); }
  };

  const changeFeedAuto = async (id: string, enabled: boolean) => {
    setFeedBusy(true);
    setFeedError("");
    try {
      await invoke("set_feed_auto_download", { id, enabled });
      setFeedSubscriptions(items => items.map(item =>
        item.id === id ? { ...item, autoDownload: enabled } : item));
      setFeedError(enabled ? "Automatic enclosure downloads enabled. Checks run roughly every 15 minutes while the app is open." :
        "Automatic enclosure downloads disabled.");
    } catch (error) { setFeedError(String(error)); }
    finally { setFeedBusy(false); }
  };

  const removeFeedSubscription = async (id: string) => {
    setFeedBusy(true);
    setFeedError("");
    try {
      await invoke("remove_feed_subscription", { id });
      setFeedSubscriptions(items => items.filter(item => item.id !== id));
      setFeedError("Feed subscription removed. Downloaded files were not deleted.");
    } catch (error) { setFeedError(String(error)); }
    finally { setFeedBusy(false); }
  };

  const inspectFeed = async () => {
    setFeedBusy(true);
    setFeedError("");
    setFeedPreview(null);
    setFeedSelection([]);
    try {
      const result = await invoke<FeedPreview>("inspect_feed", { source: feedUrl.trim() });
      setFeedPreview(result);
      setFeedSelection(result.entries.map((_, i) => i));
    } catch (error) { setFeedError(String(error)); }
    finally { setFeedBusy(false); }
  };

  const addFeedEntries = async () => {
    if (!feedPreview || feedSelection.length === 0) {
      setFeedError("Select at least one entry.");
      return;
    }
    setFeedBusy(true);
    setFeedError("");
    try {
      const sources = feedPreview.entries
        .filter((_, i) => feedSelection.includes(i)).map(item => item.source);
      const result = await invoke<{ added: number; errors: string[] }>("add_batch", { sources });
      await refresh();
      setFeedError(result.added + " link(s) queued." +
        (result.errors.length ? " " + result.errors.join(" | ") : ""));
    } catch (error) { setFeedError(String(error)); }
    finally { setFeedBusy(false); }
  };

  const inspectPlaylist = async () => {
    setMediaBusy(true);
    setMediaError("");
    setPlaylistPreview(null);
    setSelectedPlaylistItems([]);
    try {
      const info = await invoke<PlaylistPreview>("inspect_playlist", { source: mediaUrl.trim() });
      setPlaylistPreview(info);
      setSelectedPlaylistItems(info.entries.map(item => item.index));
      if (info.entries.length === 0) setMediaError("No selectable items found in the first 50 playlist entries.");
    } catch (error) { setMediaError(String(error)); }
    finally { setMediaBusy(false); }
  };

  const inspectMedia = async () => {
    setMediaBusy(true);
    setMediaError("");
    setMediaPreview(null);
    try {
      const info = await invoke<MediaPreview>("preview_media", { source: mediaUrl.trim() });
      setMediaPreview(info);
      setMediaFormat("mp4");
    } catch (error) { setMediaError(String(error)); }
    finally { setMediaBusy(false); }
  };

  const startMedia = async () => {
    setMediaBusy(true);
    setMediaError("");
    try {
      if (mediaPlaylist && playlistPreview && selectedPlaylistItems.length === 0) {
        setMediaError("Select at least one playlist item to download.");
        return;
      }
      if (mediaPlaylist && mediaFormat.startsWith("id:")) {
        setMediaError("Exact format IDs apply to one inspected video. Choose a quality preset for playlists.");
        return;
      }
      const job = await invoke<MediaJob>("start_media_download", {
        source: mediaUrl.trim(), format: mediaFormat, playlist: mediaPlaylist,
        playlistItems: mediaPlaylist && playlistPreview ? selectedPlaylistItems : [],
      });
      setMediaJobs(jobs => [...jobs, job]);
      setMediaHistoryOpen(true);
      setMediaOpen(false);
      setNewDownloadOpen(false);
      setUrl("");
      setMediaPreview(null);
      setMediaError("Media download started. Files will be saved in " + (settings.downloadDir || "your downloads folder") + ".");
    } catch (error) { setMediaError(String(error)); }
    finally { setMediaBusy(false); }
  };

  const waitingQueue = items.filter(item => item.status === "waiting");
  const orderedQueue = [...waitingQueue].sort((a, b) => {
    const ai = queueOrder.indexOf(a.gid), bi = queueOrder.indexOf(b.gid);
    return (ai < 0 ? Number.MAX_SAFE_INTEGER : ai) - (bi < 0 ? Number.MAX_SAFE_INTEGER : bi);
  });
  const reorderWaiting = async (gid: string, target: number) => {
    const oldIndex = orderedQueue.findIndex(item => item.gid === gid);
    if (oldIndex < 0 || target < 0 || target >= orderedQueue.length || oldIndex === target) return;
    setScheduleError("");
    try {
      await invoke("move_queued_download", { gid, direction: "position", position: target });
      const ids = orderedQueue.map(item => item.gid);
      ids.splice(oldIndex, 1); ids.splice(target, 0, gid);
      setQueueOrder(ids);
      await refresh();
    } catch (error) { setScheduleError(String(error)); }
  };

  const armSchedule = async () => {
    const start = new Date(scheduleStart).getTime();
    if (!Number.isFinite(start) || start <= Date.now()) {
      setScheduleError("Choose a future local date and time.");
      return;
    }
    setWorking(true);
    setScheduleError("");
    try {
      const result = await invoke<Schedule>("arm_schedule", {
        startAtUnix: Math.floor(start / 1000),
        concurrent: scheduleConcurrency,
        afterComplete: scheduleAction,
      });
      setSchedule(result);
      setScheduleOpen(false);
      await refresh();
      setActionFeedback("Schedule armed. Transfers are paused until the selected time.");
    } catch (error) { setScheduleError(String(error)); }
    finally { setWorking(false); }
  };

  const cancelSchedule = async () => {
    setWorking(true);
    setScheduleError("");
    try {
      const result = await invoke<Schedule>("cancel_schedule");
      setSchedule(result);
      setScheduleOpen(false);
      setActionFeedback("Schedule cancelled. Existing transfers can be resumed manually.");
      await refresh();
    } catch (error) {
      setScheduleError(String(error));
      setActionFeedback(String(error));
    } finally { setWorking(false); }
  };

  const save = async () => {
    setWorking(true);
    setSettingsError("");
    try {
      await invoke("save_settings", { settings: { ...draft, connections: Number(draft.connections), minSplitSizeMib: Number(draft.minSplitSizeMib), concurrent: Number(draft.concurrent), speedLimitKib: Number(draft.speedLimitKib), seedMinutes: Number(draft.seedMinutes) } });
      setSettings(draft); setSettingsOpen(false);
    } catch (err) { setSettingsError(String(err)); }
    finally { setWorking(false); }
  };

  const shown = useMemo(() => {
    const needle = query.toLowerCase();
    const visible = items.filter(d => filtered(d, filter) &&
      (d.name.toLowerCase().includes(needle) || d.source.toLowerCase().includes(needle)));
    if (sortOrder === "name") visible.sort((a, b) => a.name.localeCompare(b.name));
    if (sortOrder === "size") visible.sort((a, b) => b.totalLength - a.totalLength);
    if (sortOrder === "progress") visible.sort((a, b) => {
      const progress = (item: DownloadItem) => item.status === "complete" ? 1 :
        item.totalLength > 0 ? item.completedLength / item.totalLength : 0;
      return progress(b) - progress(a);
    });
    return visible;
  }, [items, filter, query, sortOrder]);
  const visibleMedia = mediaJobs.filter(job =>
    (filter === "all" || filter === "active" && ["downloading","paused","queued"].includes(job.status) ||
     filter === "complete" && ["complete","completed"].includes(job.status) ||
     filter === "error" && ["error","cancelled","interrupted"].includes(job.status)) &&
    (job.title.toLowerCase().includes(query.toLowerCase()) || job.id.toLowerCase().includes(query.toLowerCase())));
  const active = items.filter(d => d.status === "active").length;
  const done = items.filter(d => d.status === "complete").length + mediaJobs.filter(j => ["complete","completed"].includes(j.status)).length;
  const speed = items.reduce((sum, item) => sum + (item.status === "active" ? item.downloadSpeed : 0), 0);

  return <div className="shell">
    <aside className={sidebarExpanded ? "sidebar" : "sidebar sidebar-collapsed"} aria-label="Download navigation">
      <button type="button" className="brand brand-toggle" aria-label={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"} aria-expanded={sidebarExpanded} onClick={() => setSidebarExpanded(expanded => !expanded)} title={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"}>
        <span className="brand-icon"><ArrowDownToLine size={22} /></span>
        {sidebarExpanded && <span className="brand-copy"><strong>Download<span>Manager</span></strong><small>YOUR DOWNLOAD HUB</small></span>}
      </button>
      <button type="button" className="nav nav-new-download" title="New download" onClick={() => { setFilter("all"); setNewDownloadOpen(true); }}>
        <span className="nav-icon"><Plus size={18}/></span>{sidebarExpanded && <span className="nav-name">New download</span>}
      </button>
      {sidebarExpanded && <div className="nav-label">WORKSPACE</div>}
      {([
        { key: "all", label: "All downloads", icon: <HardDrive size={18} />, count: items.length + mediaJobs.length },
        { key: "active", label: "In progress", icon: <Download size={18} />, count: items.filter(d => activeState(d.status)).length + mediaJobs.filter(j => ["downloading","paused","queued"].includes(j.status)).length },
        { key: "complete", label: "Completed", icon: <CheckCircle2 size={18} />, count: items.filter(d => d.status === "complete").length + mediaJobs.filter(j => ["complete","completed"].includes(j.status)).length },
        { key: "torrent", label: "Torrents", icon: <Magnet size={18} />, count: items.filter(d => isTorrent(d)).length },
        { key: "error", label: "Failed", icon: <XCircle size={18} />, count: items.filter(d => filtered(d, "error")).length + mediaJobs.filter(j => ["error","cancelled","interrupted"].includes(j.status)).length },
      ] as const).map(({ key, label, icon, count }) =>
        <button type="button" key={key} title={label + ": " + count} aria-label={label + ": " + count}
          aria-current={filter === key ? "page" : undefined}
          className={["nav", filter === key ? "active" : "", key === "error" && count > 0 ? "nav-failed" : ""].filter(Boolean).join(" ")}
          onClick={() => selectView(key)}>
          <span className="nav-icon">{icon}{!sidebarExpanded && <span className="nav-icon-count">{count}</span>}</span>
          {sidebarExpanded && <><span className="nav-name">{label}</span><b>{count}</b></>}
        </button>
      )}
      {sidebarExpanded && <div className="nav-label nav-label-secondary">PREFERENCES</div>}
      <button type="button" className="nav nav-settings" title="Settings" aria-label="Settings"
        onClick={() => { setDraft(settings); setSettingsError(""); setSettingsOpen(true); }}>
        <span className="nav-icon"><Settings2 size={18} /></span>
        {sidebarExpanded && <span className="nav-name">Settings</span>}
      </button>
      {sidebarExpanded && <div className="sidebar-bottom"><div className="side-note"><ShieldCheck size={20}/><div><strong>Private RPC engine</strong><p>aria2 runs on localhost with a private token.</p></div></div><div className="version">DEVELOPMENT PREVIEW · v0.2</div></div>}
    </aside>
    <main className="main" onPointerDown={() => { if (sidebarExpanded) setSidebarExpanded(false); }}>

      <section className="heading"><div><div className="eyebrow"><Zap size={14} /> DOWNLOADS, UNDER CONTROL</div><h1>{viewTitles[filter]}<span>.</span></h1><p>{viewDescriptions[filter]}</p></div></section>
      {filter === "all" && <section className="stats">
        <div className="stat"><div className="stat-icon violet"><HardDrive size={21}/></div><div><small>All downloads</small><strong>{items.length + mediaJobs.length}</strong></div></div>
        <div className="stat"><div className="stat-icon blue"><Download size={21}/></div><div><small>Downloading</small><strong>{active + mediaJobs.filter(j => j.status === "downloading").length}</strong></div></div>
        <div className="stat"><div className="stat-icon green"><CheckCircle2 size={21}/></div><div><small>Completed</small><strong>{done}</strong></div></div>
        <div className="stat stat-speed"><div className="stat-icon violet"><Zap size={21}/></div><div><small>Total download speed</small><strong>{formatSize(speed)}/s</strong></div></div>
      </section>}
       {!engineOnline && <p className="engine-notice" role="alert">{engineHealth}</p>}
      {message && filter !== "all" && <p className="engine-notice" role="status">{message}</p>}
      {refreshError && filter !== "all" && <p className="engine-notice" role="alert">Engine: {refreshError}</p>}
      {schedule && ["armed","running","power_pending"].includes(schedule.state) && <div className="schedule-banner" role="status"><div><strong>{schedule.state === "armed" ? "Scheduled" : schedule.state === "running" ? "Schedule running" : "Power action pending"}</strong><span>{schedule.message}</span><small>{schedule.state === "armed" ? "Starts " + new Date(schedule.startAtUnix * 1000).toLocaleString() : schedule.state === "power_pending" ? "Power action at " + new Date(schedule.powerAtUnix * 1000).toLocaleTimeString() : "Monitoring " + schedule.gids.length + " transfers"}</small></div><button type="button" disabled={working} onClick={() => void cancelSchedule()}>Cancel schedule</button></div>}
      {actionFeedback && <div className="action-feedback" role="status"><span>{actionFeedback}</span><button type="button" aria-label="Dismiss notification" onClick={() => setActionFeedback("")}><X size={14}/></button></div>}
      <section className="list" aria-label={viewTitles[filter]}><div className="list-head"><div><h2>{viewTitles[filter]} <span className="view-count">{shown.length + visibleMedia.length}</span></h2><p>{filter === "all" ? "Pause, resume and monitor active transfers" : viewDescriptions[filter]}</p></div><div className="list-tools"><button type="button" className="bulk-control" onClick={() => { setFeedError(""); setFeedOpen(true); }} title="Inspect RSS/Atom feed and queue links"><Link2 size={14}/> RSS feed</button><button type="button" className="bulk-control" onClick={() => { setScheduleError(""); setScheduleConcurrency(settings.concurrent); setScheduleOpen(true); }} title="Schedule existing downloads"><Clock3 size={14}/> Scheduler</button>{items.some(item => item.status === "active" || item.status === "waiting") && <button type="button" className="bulk-control" disabled={working} onClick={() => void controlAll("pause")} title="Pause all active and queued downloads"><Pause size={14}/> Pause all</button>}{items.some(item => item.status === "paused") && <button type="button" className="bulk-control" disabled={working} onClick={() => void controlAll("resume")} title="Resume all paused downloads"><Play size={14}/> Resume all</button>}{(filter === "all" || filter === "error") && (mediaJobs.length > 0 || items.length > 0) && <button type="button" className="clear-completed" disabled={working} onClick={() => void clearAllHistory()} title="Clear completed and stopped media history plus completed direct downloads; keep files"><Trash2 size={14}/> Clear history</button>}{done > 0 && (filter === "all" || filter === "complete") && <button type="button" className="clear-completed" onClick={() => setConfirmClearCompleted(true)} title="Remove completed downloads from history; keep downloaded files"><Trash2 size={14}/> Clear completed</button>}<details className="sort-menu"><summary aria-label={"Sort downloads: " + sortOrder}>Sort: {{ default: "Recent", name: "Name", size: "File size", progress: "Progress" }[sortOrder]} <span aria-hidden="true">▾</span></summary><div className="sort-options">{([["default","Recent"],["name","Name"],["size","File size"],["progress","Progress"]] as const).map(([key,label]) => <button type="button" key={key} aria-pressed={sortOrder === key} onClick={event => { setSortOrder(key); event.currentTarget.closest("details")?.removeAttribute("open"); }}>{label}</button>)}</div></details><div className="search"><Search size={17}/><input aria-label="Search downloads" placeholder="Search downloads" value={query} onChange={e => setQuery(e.target.value)} /></div></div></div>
        {shown.length === 0 && visibleMedia.length === 0 ? <div className="empty"><div><ArrowDownToLine size={30}/></div><h3>{query ? "No matching downloads" : filter === "all" ? "No downloads here yet" : "No " + viewTitles[filter].toLowerCase() + " downloads"}</h3><p>{query ? "Try a different search or clear the search field." : filter === "all" ? "Add a URL, magnet link or torrent to get started." : "There are no downloads in this category."}</p>{(filter !== "all" || query) && <button className="clear-filter" onClick={() => { setFilter("all"); setQuery(""); }}>Show all downloads</button>}</div> :
          <div className="rows">{visibleMedia.map(job => <article className="row" key={"media-" + job.id}>
            <div className="file-icon"><Video size={20}/></div>
            <div className="file-info"><strong title={job.title}>{job.title}</strong>
              <small>{job.format.toUpperCase()} · Video / audio{job.error ? " · " + job.error : ""}</small>
              <div className="progress"><span style={{width: (["complete","completed"].includes(job.status) ? 100 : job.progress || 0) + "%"}} /></div>
            </div>
            <div className="row-meta"><span className={"status " + job.status}><StatusIcon status={job.status === "downloading" ? "active" : job.status === "completed" ? "complete" : job.status}/>{job.status}</span>
              <small>{job.progress == null ? "" : Math.round(job.progress) + "%"} {job.speed}</small></div>
            <div className="row-actions">
              {job.status === "downloading" && <button type="button" onClick={() => void controlMedia(job.id,"pause")}>Pause</button>}
              {job.status === "paused" && <button type="button" onClick={() => void controlMedia(job.id,"resume")}>Resume</button>}
              {["downloading","paused"].includes(job.status) && <button type="button" onClick={() => void cancelMedia(job.id)}>Cancel</button>}
              {["error","cancelled","interrupted"].includes(job.status) && <button type="button" onClick={() => void retryMedia(job.id)}>Retry</button>}
              {!["downloading","paused","queued"].includes(job.status) && <button type="button" title="Remove history entry (keep file)" aria-label={"Remove " + job.title + " from history"} onClick={() => void forgetMedia(job.id)}><Trash2 size={15}/></button>}
            </div>
          </article>)}{shown.map(item => <article className="row" key={item.gid}>
            <div className="file-icon">{isTorrent(item) ? <Magnet size={20}/> : <Download size={20}/>}</div>
            <div className="file-info"><strong title={item.source}>{item.name}</strong><small title={item.error || item.path || item.source}>{item.error || item.path || item.source}</small><div className="progress"><span style={{ width: item.status === "complete" ? "100%" : item.totalLength ? Math.min(100, item.completedLength / item.totalLength * 100) + "%" : "0%" }} /></div></div>
            <div className="row-meta"><span className={"status " + item.status}><StatusIcon status={item.status}/> {item.status}</span>
              <small>{formatSize(item.completedLength)}{item.totalLength ? " / " + formatSize(item.totalLength) : ""}</small>
              {item.status === "active" && <><small>{formatSize(item.downloadSpeed)}/s</small>{item.totalLength > item.completedLength && item.downloadSpeed > 0 && <small>{formatEta((item.totalLength - item.completedLength) / item.downloadSpeed)}</small>}</>}</div>
            <div className="row-actions">
              {isTorrent(item) && item.status !== "removed" && <button title="Choose files in torrent" aria-label={"Torrent files " + item.name} onClick={() => void showTorrentFiles(item)}><FolderOpen size={17}/></button>}
              {item.status === "waiting" && <><button title="Move to top of queue" aria-label={"Prioritize " + item.name} onClick={() => void moveQueue(item.gid, "top")}><ArrowUpToLine size={17}/></button><button title="Move to bottom of queue" aria-label={"Deprioritize " + item.name} onClick={() => void moveQueue(item.gid, "bottom")}><QueueBottomIcon size={17}/></button></>}
              {["active", "waiting"].includes(item.status) && <button title="Pause" aria-label={"Pause " + item.name} onClick={() => void control(item.gid, "pause")}><Pause size={17}/></button>}
              {item.status === "paused" && <button title="Resume" aria-label={"Resume " + item.name} onClick={() => void control(item.gid, "resume")}><Play size={17}/></button>}
              {["active", "waiting", "paused"].includes(item.status) && <button title="Cancel" aria-label={"Cancel " + item.name} onClick={() => void control(item.gid, "cancel")}><X size={17}/></button>}
              {["error", "removed", "interrupted"].includes(item.status) && <button title="Retry download" aria-label={"Retry " + item.name} onClick={() => void retry(item)}><Play size={17}/></button>}
              {item.status === "complete" && <button title="Open containing folder" aria-label={"Open folder for " + item.name} onClick={() => void openFolder(item.gid)}><FolderOpen size={17}/></button>}
              {!activeState(item.status) && <button title="Remove from history (does not delete files)" aria-label={"Forget " + item.name} onClick={() => void forget(item.gid)}><Trash2 size={17}/></button>}
            </div>
          </article>)}</div>}
      </section>
      <footer><span><SlidersHorizontal size={15}/> Parallel connections, torrents and magnet links powered by aria2.</span><span>LINUX FIRST · CROSS-PLATFORM ROADMAP</span></footer>
    </main>
    {feedOpen && <div className="modal-overlay" onMouseDown={event => { if (event.target === event.currentTarget) setFeedOpen(false); }}>
      <section className="modal media-modal" role="dialog" aria-modal="true" aria-label="RSS feed inspector">
        <div className="modal-title"><div><Link2 size={22}/><h2>RSS / Atom feeds</h2></div><button aria-label="Close RSS inspector" onClick={() => setFeedOpen(false)}><X size={18}/></button></div>
        <p>Inspect an RSS, Atom or JSON feed and select links to add to the aria2 queue. Enclosures are preferred. Article links may download an HTML page rather than a media file.</p>
        <label><span>Public HTTP/HTTPS feed URL</span><input type="url" value={feedUrl} onChange={event => { setFeedUrl(event.target.value); setFeedPreview(null); setFeedSelection([]); }} placeholder="https://example.org/feed.xml"/></label>
        <div className="media-actions"><button type="button" disabled={feedBusy || !feedUrl.trim()} onClick={() => void subscribeFeed()}>Save subscription</button><button type="button" disabled={feedBusy || !feedUrl.trim()} onClick={() => void inspectFeed()}>{feedBusy ? "Checking..." : "Inspect feed"}</button></div>
        {feedPreview && <div className="playlist-inspection"><strong>{feedPreview.title} · {feedSelection.length} selected</strong><div className="playlist-select-actions"><button type="button" onClick={() => setFeedSelection(feedPreview.entries.map((_, i) => i))}>Select all</button><button type="button" onClick={() => setFeedSelection([])}>Select none</button><button type="button" onClick={() => setFeedSelection(feedPreview.entries.map((entry, i) => entry.enclosure ? i : -1).filter(i => i >= 0))}>Enclosures only</button></div>
          <div className="playlist-inspection-rows">{feedPreview.entries.map((entry, i) => <label key={i}><input type="checkbox" checked={feedSelection.includes(i)} onChange={event => setFeedSelection(indices => event.target.checked ? [...indices, i] : indices.filter(n => n !== i))}/><span>{entry.enclosure ? "📎 " : "🔗 "}{entry.title}<small className="feed-entry-source">{entry.source}</small></span></label>)}</div>
        </div>}
        {feedSubscriptions.length > 0 && <div className="feed-subscriptions"><h3>Saved feeds</h3><p>Auto-download is off by default. Enabling it queues new enclosure files when the app is running; existing entries are marked seen when you subscribe.</p>{feedSubscriptions.map(item => <div key={item.id} className="feed-subscription"><div><strong>{item.title}</strong><small title={item.source}>{item.source}</small>{item.error && <small className="error">{item.error}</small>}</div><label><input type="checkbox" disabled={feedBusy} checked={item.autoDownload} onChange={event => void changeFeedAuto(item.id, event.target.checked)}/>Auto</label><button type="button" disabled={feedBusy} onClick={() => void removeFeedSubscription(item.id)}>Remove</button></div>)}</div>}
        {feedError && <p className="settings-note" role="status">{feedError}</p>}
        <div className="modal-actions"><button type="button" onClick={() => setFeedOpen(false)}>Close</button><button type="button" disabled={feedBusy || !feedPreview || feedSelection.length === 0} onClick={() => void addFeedEntries()}>{feedBusy ? "Queueing..." : "Queue selected links"}</button></div>
      </section>
    </div>}
      {newDownloadOpen && <div className="modal-overlay" onMouseDown={event => { if (event.target === event.currentTarget) setNewDownloadOpen(false); }}><section className="modal media-modal add-box" role="dialog" aria-modal="true" aria-label="New download"><div className="modal-title"><div><Plus size={20}/><h2>New download</h2></div><button type="button" aria-label="Close new download" onClick={() => setNewDownloadOpen(false)}><X size={18}/></button></div><p className="hint">HTTP · HTTPS · VIDEO · AUDIO · MAGNET · TORRENT</p>
        <div className="input-row"><input aria-label="Download source" value={url} onChange={e => setUrl(e.target.value)} onKeyDown={e => { if (e.key === "Enter") void add(); }} placeholder="Paste a download or video link" /><button disabled={working || mediaBusy} onClick={() => void add()}><Plus size={18} /> {working ? "Working..." : looksLikeMedia ? "Choose video format" : "Add download"}</button></div><div className="picker-row"><button type="button" onClick={() => void pickTorrent()}><FolderOpen size={15}/> Choose torrent / metalink file</button><button type="button" onClick={() => { setBatchError(""); setBatchOpen(true); }}><Plus size={15}/> Batch download links</button></div>
        {message && <p className={message.startsWith("Download added.") || message.endsWith("downloads queued.") ? "notice-success" : "error"} role="status">{message}</p>}
        {refreshError && <p className="error" role="alert">Engine: {refreshError}</p>}
        <p className="hint">For a local torrent or metalink, paste its absolute file path. Files download to {settings.downloadDir || "~/Downloads"}.</p>
      </section></div>}

    {mediaOpen && <div className="modal-overlay" onMouseDown={event => { if (event.target === event.currentTarget) setMediaOpen(false); }}>
      <section className="modal media-modal" role="dialog" aria-modal="true" aria-label="Video and audio downloader">
        <div className="modal-title"><div><Video size={22}/><h2>Video & audio downloader</h2></div><button aria-label="Close media downloader" onClick={() => setMediaOpen(false)}><X size={18}/></button></div>
        <p>Uses your installed yt-dlp. MP3 audio and video merging may require FFmpeg. No software is auto-installed.</p>{mediaTools && <div className="media-tool-status" role="status"><span>yt-dlp: <b>{mediaTools.ytDlp ? "Installed (" + mediaTools.ytDlp + ")" : "Missing"}</b></span><span>FFmpeg: <b>{mediaTools.ffmpeg ? "Installed" : "Missing (needed for merging / MP3)"}</b></span></div>}
        <label><span>Video or audio page URL (HTTP/HTTPS)</span><input type="url" value={mediaUrl} onChange={event => { setMediaUrl(event.target.value); setMediaPreview(null); setPlaylistPreview(null); setSelectedPlaylistItems([]); setMediaFormat("mp4"); }} placeholder="https://example.com/video"/></label>
        <div className="media-actions"><button type="button" disabled={mediaBusy || !mediaUrl.trim()} onClick={() => void inspectMedia()}>{mediaBusy ? "Working..." : "Inspect media"}</button></div>
        {mediaPreview && <div className="media-preview"><strong>{mediaPreview.title}</strong>{mediaPreview.uploader && <small>{mediaPreview.uploader}</small>}{mediaPreview.durationSeconds != null && <small>Duration: {Math.floor(mediaPreview.durationSeconds / 60)}m {Math.floor(mediaPreview.durationSeconds % 60)}s</small>}<small>yt-dlp {mediaPreview.ytDlpVersion}</small></div>}
        <label><span>Download format</span><select value={mediaFormat} onChange={event => setMediaFormat(event.target.value)}><option value="mp4">MP4 video (compatible)</option><option value="best">Best available video</option><option value="1080p">Up to 1080p video</option><option value="720p">Up to 720p video</option><option value="audio">MP3 audio</option>{mediaPreview && mediaPreview.formats.length > 0 && <optgroup disabled={mediaPlaylist} label={mediaPlaylist ? "Exact formats unavailable for playlists" : "Available formats from inspected media"}>{mediaPreview.formats.map(fmt => <option key={fmt.id} value={"id:" + fmt.id}>{fmt.id} · {fmt.extension || "unknown"} · {fmt.resolution}{fmt.note ? " · " + fmt.note : ""}{fmt.filesize != null ? " · " + formatSize(fmt.filesize) : ""}</option>)}</optgroup>}</select></label>
        {mediaPreview && mediaPreview.formats.length > 0 && <p className="media-format-note">{mediaPreview.formats.length} available format(s) found. Selecting an exact format may download video-only or audio-only; use a preset for automatic merging.</p>}
        <label className="media-playlist-toggle"><input type="checkbox" checked={mediaPlaylist} onChange={event => { setMediaPlaylist(event.target.checked); if (event.target.checked && mediaFormat.startsWith("id:")) setMediaFormat("mp4"); }}/><span>Download playlist (up to 50 items, one job)</span></label>
        {mediaPlaylist && <div className="media-playlist-selection">
          <button type="button" className="bulk-control" disabled={mediaBusy || !mediaUrl.trim()} onClick={() => void inspectPlaylist()}>Inspect playlist items</button>
          {playlistPreview && <div className="playlist-inspection"><strong>{playlistPreview.title} · {selectedPlaylistItems.length} selected</strong>
            <div className="playlist-select-actions"><button type="button" onClick={() => setSelectedPlaylistItems(playlistPreview.entries.map(item => item.index))}>Select all</button><button type="button" onClick={() => setSelectedPlaylistItems([])}>Select none</button></div>
            <div className="playlist-inspection-rows">{playlistPreview.entries.map(item => <label key={item.index}><input type="checkbox" checked={selectedPlaylistItems.includes(item.index)} onChange={event => setSelectedPlaylistItems(indices => event.target.checked ? [...indices, item.index].sort((a, b) => a-b) : indices.filter(n => n !== item.index))}/><span>{item.index}. {item.title}</span></label>)}</div>
          </div>}
          {!playlistPreview && <small>Optional: inspect and choose specific entries. Without inspection, the first 50 entries are used.</small>}
        </div>}
        {mediaJobs.length > 0 && <div className="media-job-list" aria-label="Media download jobs">{mediaJobs.map(job => <div key={job.id}><span title={job.title}>{job.title}</span><strong className={job.status === "error" ? "media-job-error" : ""}>{job.status}{job.progress != null ? " · " + Math.round(job.progress) + "%" : ""}</strong>{job.status === "downloading" && <button type="button" className="media-job-cancel" onClick={() => void controlMedia(job.id, "pause")} aria-label={"Pause media download " + job.title}>Pause</button>}{job.status === "paused" && <button type="button" className="media-job-cancel" onClick={() => void controlMedia(job.id, "resume")} aria-label={"Resume media download " + job.title}>Resume</button>}{["downloading","paused"].includes(job.status) && <button type="button" className="media-job-cancel" onClick={() => void cancelMedia(job.id)} aria-label={"Cancel media download " + job.title}>Cancel</button>}{["error","cancelled","interrupted"].includes(job.status) && <button type="button" className="media-job-cancel" onClick={() => void retryMedia(job.id)} aria-label={"Retry media download " + job.title}>Retry</button>}{job.playlist && job.playlistIndex != null && <small className="media-job-stats">Playlist item {job.playlistIndex} of up to 50{job.currentItem ? " · " + job.currentItem : ""} · item progress</small>}{job.playlist && job.overallProgress != null && <div className="playlist-overall" aria-label={"Estimated overall playlist progress for " + job.title}><span>Selected playlist progress (estimate): {Math.round(job.overallProgress)}%</span><progress max={100} value={job.overallProgress}/></div>}{job.progress != null && <progress className="media-progress" max={100} value={job.progress} aria-label={"Progress for " + job.title}/>}{job.status === "downloading" && (job.speed || job.eta) && <small className="media-job-stats">{job.speed}{job.speed && job.eta ? " · " : ""}{job.eta && "ETA " + job.eta}</small>}{job.error && <small>{job.error}</small>}</div>)}</div>}
        {mediaError && <p className="settings-note" role="status">{mediaError}</p>}
        <div className="modal-actions"><button type="button" onClick={() => setMediaOpen(false)}>Close</button><button type="button" disabled={mediaBusy || !mediaUrl.trim()} onClick={() => void startMedia()}>{mediaBusy ? "Starting..." : "Start media download"}</button></div>
      </section>
    </div>}
    {scheduleOpen && <div className="modal-overlay" onMouseDown={event => { if (event.target === event.currentTarget) setScheduleOpen(false); }}>
      <section className="modal schedule-modal" role="dialog" aria-modal="true" aria-label="Schedule downloads">
        <div className="modal-title"><div><Clock3 size={22}/><h2>Download scheduler</h2></div><button aria-label="Close scheduler" onClick={() => setScheduleOpen(false)}><X size={18}/></button></div>
        <p>Schedule the active, queued and paused downloads already in your list. Keep Download Manager running until the jobs complete.</p>
        <div className="scheduler-priority" aria-label="Waiting download priority">
           <strong>Download priority</strong><p>Drag waiting downloads into order, or use the arrows. First item starts first when a slot is available. Active and paused transfers cannot be reordered here.</p>
           {orderedQueue.length === 0 && <small>No waiting downloads to prioritize.</small>}
           {orderedQueue.map((item, index) => <div key={item.gid} className="priority-item" draggable
             onDragStart={event => { setDraggedQueueId(item.gid); event.dataTransfer.effectAllowed = "move"; }}
             onDragOver={event => event.preventDefault()}
             onDrop={event => { event.preventDefault(); if (draggedQueueId) void reorderWaiting(draggedQueueId, index); setDraggedQueueId(null); }}
             onDragEnd={() => setDraggedQueueId(null)}>
             <span>☰ {index + 1}. {item.name}</span>
             <button type="button" disabled={index === 0} aria-label={"Move " + item.name + " up"} onClick={() => void reorderWaiting(item.gid, index - 1)}>↑</button>
             <button type="button" disabled={index === orderedQueue.length - 1} aria-label={"Move " + item.name + " down"} onClick={() => void reorderWaiting(item.gid, index + 1)}>↓</button>
           </div>)}
         </div>
         <label><span>Start time (your local timezone)</span><input type="datetime-local" value={scheduleStart} onChange={event => setScheduleStart(event.target.value)}/></label>
        <label><span>Maximum simultaneous downloads</span><input type="number" min="1" max="10" value={scheduleConcurrency} onChange={event => setScheduleConcurrency(Number(event.target.value))}/></label>
        <label><span>After all scheduled downloads finish successfully</span><select value={scheduleAction} onChange={event => setScheduleAction(event.target.value as Schedule["afterComplete"])}><option value="none">Do nothing</option><option value="shutdown">Shut down computer</option><option value="sleep">Put computer to sleep</option></select></label>
        {scheduleAction !== "none" && <div className="schedule-warning">This can interrupt unsaved work in other applications. Power action is attempted only if every scheduled transfer completes successfully, with a 60-second cancellation window. Keep the app running. Your desktop may require permission.</div>}
        {schedule && ["armed","running","power_pending"].includes(schedule.state) && <p className="settings-note">Current: {schedule.message}</p>}
        {scheduleError && <p className="error" role="alert">{scheduleError}</p>}
        <div className="modal-actions"><button type="button" onClick={() => setScheduleOpen(false)}>Close</button>{schedule && ["armed","running","power_pending"].includes(schedule.state) ? <button type="button" disabled={working} onClick={() => void cancelSchedule()}>Cancel schedule</button> : <button type="button" disabled={working || !scheduleStart} onClick={() => void armSchedule()}>{working ? "Saving..." : "Arm schedule"}</button>}</div>
      </section>
    </div>}
    {confirmClearCompleted && <div className="modal-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setConfirmClearCompleted(false); }}>
      <section className="modal confirm-modal" role="alertdialog" aria-modal="true" aria-label="Clear completed download history" aria-describedby="clear-completed-explanation">
        <div className="modal-title"><div><Trash2 size={21}/><h2>Clear completed history?</h2></div><button aria-label="Close confirmation" type="button" onClick={() => setConfirmClearCompleted(false)}><X size={18}/></button></div>
        <p id="clear-completed-explanation">Remove {done} completed download{done === 1 ? "" : "s"} from the list? Downloaded files on disk will stay exactly where they are. This cannot be undone from the app.</p>
        <div className="modal-actions"><button type="button" onClick={() => setConfirmClearCompleted(false)}>Cancel</button><button type="button" disabled={working} onClick={() => void clearCompleted()}>{working ? "Clearing..." : "Clear history only"}</button></div>
      </section>
    </div>}
    {batchOpen && <div className="modal-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setBatchOpen(false); }}>
      <section className="modal batch-modal" role="dialog" aria-modal="true" aria-label="Add multiple downloads">
        <div className="modal-title"><div><Link2 size={22}/><h2>Batch downloads</h2></div><button aria-label="Close batch downloads" onClick={() => setBatchOpen(false)}><X size={18}/></button></div>
        <p>Add up to 50 direct links or magnet URLs. Put one link on each line.</p>
        <textarea aria-label="Batch download links" className="batch-text" spellCheck={false} placeholder={"https://example.org/file1.zip\\nhttps://example.org/file2.iso"} value={batchText} onChange={e => setBatchText(e.target.value)} rows={9}/>
        {batchError && <p className="error" role="alert">{batchError}</p>}
        <div className="modal-actions"><button onClick={() => setBatchOpen(false)}>Cancel</button><button disabled={working || !batchText.trim()} onClick={() => void addBatch()}>Add to queue</button></div>
      </section>
    </div>}
    {torrentDialog && <div className="modal-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setTorrentDialog(null); }}>
      <section className="modal torrent-modal" role="dialog" aria-modal="true" aria-label="Torrent file selection">
        <div className="modal-title"><div><Magnet size={22}/><h2>Select torrent files</h2></div><button aria-label="Close file selection" onClick={() => setTorrentDialog(null)}><X size={18}/></button></div>
        <p title={torrentDialog.source}>Choose which files to download in {torrentDialog.name}. Changing file selection can restart the transfer internally.</p>
        {torrentInfo && <div className="torrent-metadata"><span>Connected peers: <strong>{torrentInfo.peers}</strong></span><span>Trackers: <strong>{torrentInfo.trackers.length}</strong></span>{torrentInfo.trackers.length > 0 && <details><summary>Show trackers</summary><ul>{torrentInfo.trackers.map((tracker, index) => <li key={index}>{tracker}</li>)}</ul></details>}</div>}
        {torrentError && <p className="error" role="alert">{torrentError}</p>}
        {torrentBusy && <p>Loading torrent metadata...</p>}
        {!torrentBusy && torrentFiles.length === 0 && !torrentError && <p>File details are not available yet.</p>}
        {torrentFiles.length > 0 && <div className="torrent-file-list">{torrentFiles.map(file => <label key={file.index} className="torrent-file">
          <input type="checkbox" checked={selectedFiles.includes(file.index)}
            onChange={event => setSelectedFiles(prev => event.target.checked ? [...prev, file.index] : prev.filter(i => i !== file.index))}/>
          <span><strong title={file.path}>{file.path.split("/").pop() || "File " + file.index}</strong><small title={file.path}>{file.path} · {formatSize(file.length)} · {formatSize(file.completedLength)} complete</small></span>
        </label>)}</div>}
        <div className="modal-actions"><button onClick={() => setTorrentDialog(null)}>Cancel</button><button disabled={torrentBusy || selectedFiles.length === 0 || torrentFiles.length === 0} onClick={() => void saveTorrentFiles()}>Apply selection</button></div>
      </section>
    </div>}
    {settingsOpen && <div className="modal-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setSettingsOpen(false); }}>
      <section className="modal settings-modal" role="dialog" aria-modal="true" aria-label="Download settings">
        <div className="modal-title"><div><Settings2 size={22}/><h2>Preferences</h2></div><button aria-label="Close settings" onClick={() => setSettingsOpen(false)}><X size={18}/></button></div>
        <div className="settings-layout">
          <nav className="settings-tabs" aria-label="Settings categories">
            {([["general","General"],["connection","Connection"],["save","Save To"],["downloads","Downloads"],["integration","Browser & Files"],["privacy","Proxy & Privacy"]] as const).map(([key,label]) =>
              <button key={key} type="button" className={settingsTab === key ? "selected" : ""} onClick={() => setSettingsTab(key)}>{label}</button>
            )}
          </nav>
          <div className="settings-content">
            {settingsTab === "general" && <>
              <h3>General & performance</h3><p>Control background activity and how frequently progress is updated.</p>
              <label><span>Refresh interval (seconds)</span><input type="number" min="1" max="30" value={draft.refreshSeconds} onChange={e => setDraft(s => ({ ...s, refreshSeconds: Number(e.target.value) }))}/></label>
              <label className="settings-toggle"><input type="checkbox" checked={draft.lowMemoryMode} onChange={e => setDraft(s => ({ ...s, lowMemoryMode: e.target.checked }))}/><span><strong>Low Memory Mode</strong><small>Pause status polling while the app is in the background. Recommended.</small></span></label>
              <p className="settings-note">Uses lightweight aria2 status queries instead of reloading the entire list on every render. Download transfers continue in the background.</p>
            </>}
            {settingsTab === "connection" && <>
              <h3>Connection & speed</h3><p>These settings control the aria2 download engine.</p>
              <label><span>Requested parallel segments (1–32)</span><input type="number" min="1" max="32" value={draft.connections} onChange={e => setDraft(s => ({ ...s, connections: Number(e.target.value) }))}/></label>
              <label><span>Minimum segment size (MiB)</span><input type="number" min="1" max="64" value={draft.minSplitSizeMib} onChange={e => setDraft(s => ({ ...s, minSplitSizeMib: Number(e.target.value) }))}/></label>
              <label><span>Maximum concurrent downloads</span><input type="number" min="1" max="10" value={draft.concurrent} onChange={e => setDraft(s => ({ ...s, concurrent: Number(e.target.value) }))}/></label>
              <label><span>Overall speed limit (KiB/s, 0 = unlimited)</span><input type="number" min="0" max="1048576" value={draft.speedLimitKib} onChange={e => setDraft(s => ({ ...s, speedLimitKib: Number(e.target.value) }))}/></label>
              <p className="settings-note">Allows up to 32 requested segments per download. Stock aria2 caps connections to a single server at 16, so 32 segments do not guarantee 32 same-server connections. Settings affect newly queued downloads; results depend on HTTP range support and server limits.</p>
            </>}
            {settingsTab === "save" && <>
              <h3>Save To</h3><p>Choose where new downloads are stored.</p>
              <label><span>Default download directory</span><div className="folder-row"><input value={draft.downloadDir} onChange={e => setDraft(s => ({ ...s, downloadDir: e.target.value }))}/><button type="button" onClick={() => void pickDirectory()}>Browse</button></div></label>
              <p className="settings-note">Existing downloads keep their original location. Automatic category folders and temporary-folder selection are planned.</p>
            </>}
            {settingsTab === "downloads" && <>
              <h3>Downloads & torrents</h3><p>Manage transfer behavior.</p>
              <label><span>Torrent seeding time (minutes, 0 = off)</span><input type="number" min="0" max="65535" value={draft.seedMinutes} onChange={e => setDraft(s => ({ ...s, seedMinutes: Number(e.target.value) }))}/></label>
              <p className="settings-note">The seeding limit applies to new torrents. Use the Scheduler button above the download list to start a queued batch at a chosen time with a concurrency limit and optional completion action. Repeating schedules are planned.</p>
            </>}
            {settingsTab === "integration" && <>
              <h3>Browser integration & file types</h3><p>Planned features, not enabled in this preview.</p>
              <div className="planned-row">Browser extension integration <span>Planned</span></div>
              <div className="planned-row">Clipboard URL monitoring <span>Planned</span></div>
              <div className="planned-row">File extension takeover lists <span>Planned</span></div>
              <div className="planned-row">Site exclusions & keyboard shortcuts <span>Planned</span></div>
            </>}
            {settingsTab === "privacy" && <>
              <h3>Proxy, accounts & privacy</h3><p>Planned features, not enabled in this preview.</p>
              <div className="planned-row">HTTP / SOCKS proxy rules <span>Planned</span></div>
              <div className="planned-row">Site logins & credentials <span>Planned</span></div>
              <div className="planned-row">Per-site connection exceptions <span>Planned</span></div>
              <div className="planned-row">Completion sounds & notifications <span>Planned</span></div>
              <p className="settings-note">The local aria2 RPC remains bound to loopback with a private token.</p>
            </>}
          </div>
        </div>
        {settingsError && <p className="error" role="alert">{settingsError}</p>}
        <div className="modal-actions"><button onClick={() => setSettingsOpen(false)}>Cancel</button><button disabled={working} onClick={() => void save()}>Save settings</button></div>
      </section>
    </div>}
  </div>;
}
