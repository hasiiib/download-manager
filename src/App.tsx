import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Activity, ArrowDownToLine, ArrowUpToLine, ArrowDownToLine as QueueBottomIcon, CheckCircle2, Clock3, Download, FolderOpen, HardDrive,
  Link2, Magnet, Pause, Play, Plus, Search, Settings2, ShieldCheck, SlidersHorizontal,
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
  concurrent: number;
  speedLimitKib: number;
  seedMinutes: number;
  downloadDir: string;
  refreshSeconds: number;
  lowMemoryMode: boolean;
};
type Filter = "all" | "active" | "complete" | "error" | "torrent";
type SortOrder = "default" | "name" | "size" | "progress";
const viewTitles: Record<Filter, string> = {
  all: "All downloads",
  active: "In progress",
  complete: "Completed",
  error: "Failed",
  torrent: "Torrents",
};
const viewDescriptions: Record<Filter, string> = {
  all: "Manage direct links, torrents and magnet downloads in one place.",
  active: "Active, queued and paused transfers.",
  complete: "Successfully finished downloads.",
  error: "Failed, removed or interrupted transfers.",
  torrent: "Your magnet and BitTorrent downloads.",
};
type SettingsTab = "general" | "connection" | "save" | "downloads" | "integration" | "privacy";
const defaults: Settings = { connections: 8, concurrent: 3, speedLimitKib: 0, seedMinutes: 0, downloadDir: "", refreshSeconds: 3, lowMemoryMode: true };
const formatSize = (n = 0) => n < 1024 ? n + " B" : n < 1048576 ? (n / 1024).toFixed(1) + " KB" : n < 1073741824 ? (n / 1048576).toFixed(1) + " MB" : (n / 1073741824).toFixed(2) + " GB";
const formatEta = (seconds: number) => {
  if (!Number.isFinite(seconds) || seconds < 0) return "Calculating...";
  if (seconds < 60) return Math.ceil(seconds) + " sec left";
  if (seconds < 3600) return Math.ceil(seconds / 60) + " min left";
  if (seconds < 86400) return Math.floor(seconds / 3600) + " hr " + Math.ceil(seconds % 3600 / 60) + " min left";
  return "Over a day left";
};
const isTorrent = (item: DownloadItem) => item.isTorrent || item.source.startsWith("magnet:") || /\.(torrent)(\?|$)/i.test(item.source);
const activeState = (s: string) => ["active", "waiting", "paused"].includes(s);
const filtered = (d: DownloadItem, filter: Filter) => filter === "all" ||
  (filter === "active" && activeState(d.status)) ||
  (filter === "complete" && d.status === "complete") ||
  (filter === "error" && ["error", "removed", "interrupted"].includes(d.status)) ||
  (filter === "torrent" && isTorrent(d));

function StatusIcon({ status }: { status: string }) {
  if (status === "complete") return <CheckCircle2 size={13} />;
  if (status === "error" || status === "removed" || status === "interrupted") return <XCircle size={13} />;
  if (status === "active") return <Activity size={13} />;
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
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [torrentDialog, setTorrentDialog] = useState<DownloadItem | null>(null);
  const [torrentFiles, setTorrentFiles] = useState<TorrentFile[]>([]);
  const [torrentInfo, setTorrentInfo] = useState<TorrentInfo | null>(null);
  const [selectedFiles, setSelectedFiles] = useState<number[]>([]);
  const [torrentError, setTorrentError] = useState("");
  const [torrentBusy, setTorrentBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [actionFeedback, setActionFeedback] = useState("");
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

  const add = async () => {
    if (!url.trim()) { setMessage("Paste a URL, magnet link or full path to a .torrent/.meta4 file."); return; }
    setMessage("");
    setWorking(true);
    try {
      await invoke("add_download", { source: url.trim() });
      setFilter("all");
      setQuery("");
      setMessage("Download added. Showing all downloads.");
      setUrl("");
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

  const save = async () => {
    setWorking(true);
    setSettingsError("");
    try {
      await invoke("save_settings", { settings: { ...draft, connections: Number(draft.connections), concurrent: Number(draft.concurrent), speedLimitKib: Number(draft.speedLimitKib), seedMinutes: Number(draft.seedMinutes) } });
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
  const active = items.filter(d => d.status === "active").length;
  const done = items.filter(d => d.status === "complete").length;
  const speed = items.reduce((sum, item) => sum + (item.status === "active" ? item.downloadSpeed : 0), 0);

  return <div className="shell">
    <aside className={sidebarExpanded ? "sidebar" : "sidebar sidebar-collapsed"} aria-label="Download navigation">
      <button type="button" className="brand brand-toggle" aria-label={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"} aria-expanded={sidebarExpanded} onClick={() => setSidebarExpanded(expanded => !expanded)} title={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"}>
        <span className="brand-icon"><ArrowDownToLine size={22} /></span>
        {sidebarExpanded && <span className="brand-copy"><strong>Download<span>Manager</span></strong><small>YOUR DOWNLOAD HUB</small></span>}
      </button>
      {sidebarExpanded && <div className="nav-label">WORKSPACE</div>}
      {([
        { key: "all", label: "All downloads", icon: <HardDrive size={18} />, count: items.length },
        { key: "active", label: "In progress", icon: <Download size={18} />, count: items.filter(d => activeState(d.status)).length },
        { key: "complete", label: "Completed", icon: <CheckCircle2 size={18} />, count: items.filter(d => d.status === "complete").length },
        { key: "torrent", label: "Torrents", icon: <Magnet size={18} />, count: items.filter(d => isTorrent(d)).length },
        { key: "error", label: "Failed", icon: <XCircle size={18} />, count: items.filter(d => filtered(d, "error")).length },
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
        <div className="stat"><div className="stat-icon violet"><HardDrive size={21}/></div><div><small>All downloads</small><strong>{items.length}</strong></div></div>
        <div className="stat"><div className="stat-icon blue"><Download size={21}/></div><div><small>Downloading</small><strong>{active}</strong></div></div>
        <div className="stat"><div className="stat-icon green"><CheckCircle2 size={21}/></div><div><small>Completed</small><strong>{done}</strong></div></div>
        <div className="stat stat-speed"><div className="stat-icon violet"><Zap size={21}/></div><div><small>Total download speed</small><strong>{formatSize(speed)}/s</strong></div></div>
      </section>}
      {filter === "all" && <section className="add-box"><div className="section-title"><div><Link2 size={18}/><h2>New download</h2></div><span>HTTP · HTTPS · FTP · MAGNET · TORRENT · METALINK</span></div>
        <div className="input-row"><input aria-label="Download source" value={url} onChange={e => setUrl(e.target.value)} onKeyDown={e => { if (e.key === "Enter") void add(); }} placeholder="Paste a link, magnet:? URL or /path/to/file.torrent" /><button disabled={working} onClick={() => void add()}><Plus size={18} /> {working ? "Working..." : "Add download"}</button></div><div className="picker-row"><button type="button" onClick={() => void pickTorrent()}><FolderOpen size={15}/> Choose torrent / metalink file</button><button type="button" onClick={() => { setBatchError(""); setBatchOpen(true); }}><Plus size={15}/> Batch download links</button></div>
        {message && <p className={message.startsWith("Download added.") || message.endsWith("downloads queued.") ? "notice-success" : "error"} role="status">{message}</p>}
        {refreshError && <p className="error" role="alert">Engine: {refreshError}</p>}
        <p className="hint">For a local torrent or metalink, paste its absolute file path. Files download to {settings.downloadDir || "~/Downloads"}.</p>
      </section>}
      {!engineOnline && <p className="engine-notice" role="alert">{engineHealth}</p>}
      {message && filter !== "all" && <p className="engine-notice" role="status">{message}</p>}
      {refreshError && filter !== "all" && <p className="engine-notice" role="alert">Engine: {refreshError}</p>}
      {actionFeedback && <div className="action-feedback" role="status"><span>{actionFeedback}</span><button type="button" aria-label="Dismiss notification" onClick={() => setActionFeedback("")}><X size={14}/></button></div>}
      <section className="list" aria-label={viewTitles[filter]}><div className="list-head"><div><h2>{viewTitles[filter]} <span className="view-count">{shown.length}</span></h2><p>{filter === "all" ? "Pause, resume and monitor active transfers" : viewDescriptions[filter]}</p></div><div className="list-tools"><details className="sort-menu"><summary aria-label={"Sort downloads: " + sortOrder}>Sort: {{ default: "Recent", name: "Name", size: "File size", progress: "Progress" }[sortOrder]} <span aria-hidden="true">▾</span></summary><div className="sort-options">{([["default","Recent"],["name","Name"],["size","File size"],["progress","Progress"]] as const).map(([key,label]) => <button type="button" key={key} aria-pressed={sortOrder === key} onClick={event => { setSortOrder(key); event.currentTarget.closest("details")?.removeAttribute("open"); }}>{label}</button>)}</div></details><div className="search"><Search size={17}/><input aria-label="Search downloads" placeholder="Search downloads" value={query} onChange={e => setQuery(e.target.value)} /></div></div></div>
        {shown.length === 0 ? <div className="empty"><div><ArrowDownToLine size={30}/></div><h3>{query ? "No matching downloads" : filter === "all" ? "No downloads here yet" : "No " + viewTitles[filter].toLowerCase() + " downloads"}</h3><p>{query ? "Try a different search or clear the search field." : filter === "all" ? "Add a URL, magnet link or torrent to get started." : "There are no downloads in this category."}</p>{(filter !== "all" || query) && <button className="clear-filter" onClick={() => { setFilter("all"); setQuery(""); }}>Show all downloads</button>}</div> :
          <div className="rows">{shown.map(item => <article className="row" key={item.gid}>
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
              {activeState(item.status) && <button title="Cancel" aria-label={"Cancel " + item.name} onClick={() => void control(item.gid, "cancel")}><X size={17}/></button>}
              {["error", "removed", "interrupted"].includes(item.status) && <button title="Retry download" aria-label={"Retry " + item.name} onClick={() => void retry(item)}><Play size={17}/></button>}
              {item.status === "complete" && <button title="Open containing folder" aria-label={"Open folder for " + item.name} onClick={() => void openFolder(item.gid)}><FolderOpen size={17}/></button>}
              {!activeState(item.status) && <button title="Remove from history (does not delete files)" aria-label={"Forget " + item.name} onClick={() => void forget(item.gid)}><Trash2 size={17}/></button>}
            </div>
          </article>)}</div>}
      </section>
      <footer><span><SlidersHorizontal size={15}/> Parallel connections, torrents and magnet links powered by aria2.</span><span>LINUX FIRST · CROSS-PLATFORM ROADMAP</span></footer>
    </main>
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
              <label><span>Connections per server</span><input type="number" min="1" max="16" value={draft.connections} onChange={e => setDraft(s => ({ ...s, connections: Number(e.target.value) }))}/></label>
              <label><span>Maximum concurrent downloads</span><input type="number" min="1" max="10" value={draft.concurrent} onChange={e => setDraft(s => ({ ...s, concurrent: Number(e.target.value) }))}/></label>
              <label><span>Overall speed limit (KiB/s, 0 = unlimited)</span><input type="number" min="0" max="1048576" value={draft.speedLimitKib} onChange={e => setDraft(s => ({ ...s, speedLimitKib: Number(e.target.value) }))}/></label>
              <p className="settings-note">The connection-per-server limit applies to newly queued downloads; the overall rate and concurrent limit update immediately.</p>
            </>}
            {settingsTab === "save" && <>
              <h3>Save To</h3><p>Choose where new downloads are stored.</p>
              <label><span>Default download directory</span><div className="folder-row"><input value={draft.downloadDir} onChange={e => setDraft(s => ({ ...s, downloadDir: e.target.value }))}/><button type="button" onClick={() => void pickDirectory()}>Browse</button></div></label>
              <p className="settings-note">Existing downloads keep their original location. Automatic category folders and temporary-folder selection are planned.</p>
            </>}
            {settingsTab === "downloads" && <>
              <h3>Downloads & torrents</h3><p>Manage transfer behavior.</p>
              <label><span>Torrent seeding time (minutes, 0 = off)</span><input type="number" min="0" max="65535" value={draft.seedMinutes} onChange={e => setDraft(s => ({ ...s, seedMinutes: Number(e.target.value) }))}/></label>
              <p className="settings-note">The seeding limit applies to new torrents. Automatic completion actions, download quotas and scheduled queues are planned.</p>
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
