import { useCallback, useEffect, useMemo, useState } from "react";
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
};
type Filter = "all" | "active" | "complete" | "error" | "torrent";
const defaults: Settings = { connections: 8, concurrent: 3, speedLimitKib: 0, seedMinutes: 0, downloadDir: "" };
const formatSize = (n = 0) => n < 1024 ? n + " B" : n < 1048576 ? (n / 1024).toFixed(1) + " KB" : n < 1073741824 ? (n / 1048576).toFixed(1) + " MB" : (n / 1073741824).toFixed(2) + " GB";
const isTorrent = (item: DownloadItem) => item.isTorrent || item.source.startsWith("magnet:") || /\.(torrent)(\?|$)/i.test(item.source);
const activeState = (s: string) => ["active", "waiting", "paused"].includes(s);
const filtered = (d: DownloadItem, filter: Filter) => filter === "all" ||
  (filter === "active" && activeState(d.status)) ||
  (filter === "complete" && d.status === "complete") ||
  (filter === "error" && ["error", "removed"].includes(d.status)) ||
  (filter === "torrent" && isTorrent(d));

function StatusIcon({ status }: { status: string }) {
  if (status === "complete") return <CheckCircle2 size={13} />;
  if (status === "error" || status === "removed") return <XCircle size={13} />;
  if (status === "active") return <Activity size={13} />;
  return <Clock3 size={13} />;
}

export default function App() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [url, setUrl] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [settings, setSettings] = useState<Settings>(defaults);
  const [draft, setDraft] = useState<Settings>(defaults);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [torrentDialog, setTorrentDialog] = useState<DownloadItem | null>(null);
  const [torrentFiles, setTorrentFiles] = useState<TorrentFile[]>([]);
  const [torrentInfo, setTorrentInfo] = useState<TorrentInfo | null>(null);
  const [selectedFiles, setSelectedFiles] = useState<number[]>([]);
  const [torrentError, setTorrentError] = useState("");
  const [torrentBusy, setTorrentBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [working, setWorking] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const [settingsError, setSettingsError] = useState("");

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<DownloadItem[]>("list_downloads");
      setItems(list);
      setRefreshError("");
    } catch (err) { setRefreshError(String(err)); }
  }, []);

  useEffect(() => {
    void refresh();
    invoke<Settings>("get_settings").then(s => { setSettings(s); setDraft(s); }).catch(e => setMessage(String(e)));
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => window.clearInterval(timer);
  }, [refresh]);

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
      setUrl("");
      await refresh();
    } catch (err) { setMessage(String(err)); }
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
    setMessage("");
    try { await invoke("move_queued_download", { gid, direction }); await refresh(); }
    catch (err) { setMessage(String(err)); }
  };

  const retry = async (item: DownloadItem) => {
    setMessage("");
    try { await invoke("add_download", { source: item.source }); await refresh(); }
    catch (err) { setMessage(String(err)); }
  };

  const control = async (gid: string, action: "pause" | "resume" | "cancel") => {
    setMessage("");
    try { await invoke("control_download", { gid, action }); await refresh(); }
    catch (err) { setMessage(String(err)); }
  };

  const forget = async (gid: string) => {
    try { await invoke("forget_download", { gid }); await refresh(); }
    catch (err) { setMessage(String(err)); }
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

  const shown = useMemo(() => items.filter(d => filtered(d, filter) && (d.name.toLowerCase().includes(query.toLowerCase()) || d.source.toLowerCase().includes(query.toLowerCase()))), [items, filter, query]);
  const active = items.filter(d => d.status === "active").length;
  const done = items.filter(d => d.status === "complete").length;
  const speed = items.reduce((sum, item) => sum + (item.status === "active" ? item.downloadSpeed : 0), 0);

  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-icon"><ArrowDownToLine size={22} /></div><div><strong>Download<span>Manager</span></strong><small>YOUR DOWNLOAD HUB</small></div></div>
      <div className="nav-label">WORKSPACE</div>
      <button className={filter === "all" ? "nav active" : "nav"} onClick={() => setFilter("all")}><HardDrive size={18} /> All downloads <b>{items.length}</b></button>
      <button className={filter === "active" ? "nav active" : "nav"} onClick={() => setFilter("active")}><Download size={18} /> In progress <b>{items.filter(d => activeState(d.status)).length}</b></button>
      <button className={filter === "complete" ? "nav active" : "nav"} onClick={() => setFilter("complete")}><CheckCircle2 size={18} /> Completed <b>{done}</b></button>
      <button className={filter === "torrent" ? "nav active" : "nav"} onClick={() => setFilter("torrent")}><Magnet size={18} /> Torrents <b>{items.filter(d => isTorrent(d)).length}</b></button>
      <button className={filter === "error" ? "nav active" : "nav"} onClick={() => setFilter("error")}><XCircle size={18} /> Failed</button>
      <div className="nav-label nav-label-secondary">PREFERENCES</div>
      <button className="nav" onClick={() => { setDraft(settings); setSettingsError(""); setSettingsOpen(true); }}><Settings2 size={18} /> Settings</button>
      <div className="sidebar-bottom"><div className="side-note"><ShieldCheck size={20}/><div><strong>Private RPC engine</strong><p>aria2 runs on localhost with a private token.</p></div></div><div className="version">DEVELOPMENT PREVIEW · v0.2</div></div>
    </aside>
    <main className="main">
      <header><div className="crumb">WORKSPACE <span>/</span> DOWNLOADS</div><div className="badge"><span className="dot" /> Ubuntu / aria2 Engine</div></header>
      <section className="heading"><div><div className="eyebrow"><Zap size={14} /> DOWNLOADS, UNDER CONTROL</div><h1>Downloads<span>.</span></h1><p>Manage direct links, torrents and magnet downloads in one place.</p></div></section>
      <section className="stats">
        <div className="stat"><div className="stat-icon violet"><HardDrive size={21}/></div><div><small>All downloads</small><strong>{items.length}</strong></div></div>
        <div className="stat"><div className="stat-icon blue"><Download size={21}/></div><div><small>Downloading</small><strong>{active}</strong></div></div>
        <div className="stat"><div className="stat-icon green"><CheckCircle2 size={21}/></div><div><small>Completed</small><strong>{done}</strong></div></div>
        <div className="stat stat-speed"><div className="stat-icon violet"><Zap size={21}/></div><div><small>Total download speed</small><strong>{formatSize(speed)}/s</strong></div></div>
      </section>
      <section className="add-box"><div className="section-title"><div><Link2 size={18}/><h2>New download</h2></div><span>HTTP · HTTPS · FTP · MAGNET · TORRENT · METALINK</span></div>
        <div className="input-row"><input aria-label="Download source" value={url} onChange={e => setUrl(e.target.value)} onKeyDown={e => { if (e.key === "Enter") void add(); }} placeholder="Paste a link, magnet:? URL or /path/to/file.torrent" /><button disabled={working} onClick={() => void add()}><Plus size={18} /> {working ? "Working..." : "Add download"}</button></div><div className="picker-row"><button type="button" onClick={() => void pickTorrent()}><FolderOpen size={15}/> Choose torrent / metalink file</button></div>
        {message && <p className="error" role="alert">{message}</p>}
        {refreshError && <p className="error" role="alert">Engine: {refreshError}</p>}
        <p className="hint">For a local torrent or metalink, paste its absolute file path. Files download to {settings.downloadDir || "~/Downloads"}.</p>
      </section>
      <section className="list"><div className="list-head"><div><h2>Download activity</h2><p>Pause, resume and monitor active transfers</p></div><div className="search"><Search size={17}/><input aria-label="Search downloads" placeholder="Search downloads" value={query} onChange={e => setQuery(e.target.value)} /></div></div>
        {shown.length === 0 ? <div className="empty"><div><ArrowDownToLine size={30}/></div><h3>No downloads here yet</h3><p>Add a URL, magnet link or torrent to get started.</p></div> :
          <div className="rows">{shown.map(item => <article className="row" key={item.gid}>
            <div className="file-icon">{isTorrent(item) ? <Magnet size={20}/> : <Download size={20}/>}</div>
            <div className="file-info"><strong title={item.source}>{item.name}</strong><small title={item.error || item.path || item.source}>{item.error || item.path || item.source}</small><div className="progress"><span style={{ width: item.status === "complete" ? "100%" : item.totalLength ? Math.min(100, item.completedLength / item.totalLength * 100) + "%" : "0%" }} /></div></div>
            <div className="row-meta"><span className={"status " + item.status}><StatusIcon status={item.status}/> {item.status}</span>
              <small>{formatSize(item.completedLength)}{item.totalLength ? " / " + formatSize(item.totalLength) : ""}</small>
              {item.status === "active" && <small>{formatSize(item.downloadSpeed)}/s</small>}</div>
            <div className="row-actions">
              {isTorrent(item) && item.status !== "removed" && <button title="Choose files in torrent" aria-label={"Torrent files " + item.name} onClick={() => void showTorrentFiles(item)}><FolderOpen size={17}/></button>}
              {item.status === "waiting" && <><button title="Move to top of queue" aria-label={"Prioritize " + item.name} onClick={() => void moveQueue(item.gid, "top")}><ArrowUpToLine size={17}/></button><button title="Move to bottom of queue" aria-label={"Deprioritize " + item.name} onClick={() => void moveQueue(item.gid, "bottom")}><QueueBottomIcon size={17}/></button></>}
              {["active", "waiting"].includes(item.status) && <button title="Pause" aria-label={"Pause " + item.name} onClick={() => void control(item.gid, "pause")}><Pause size={17}/></button>}
              {item.status === "paused" && <button title="Resume" aria-label={"Resume " + item.name} onClick={() => void control(item.gid, "resume")}><Play size={17}/></button>}
              {activeState(item.status) && <button title="Cancel" aria-label={"Cancel " + item.name} onClick={() => void control(item.gid, "cancel")}><X size={17}/></button>}
              {["error", "removed", "interrupted"].includes(item.status) && <button title="Retry download" aria-label={"Retry " + item.name} onClick={() => void retry(item)}><Play size={17}/></button>}
              {!activeState(item.status) && <button title="Remove from history (does not delete files)" aria-label={"Forget " + item.name} onClick={() => void forget(item.gid)}><Trash2 size={17}/></button>}
            </div>
          </article>)}</div>}
      </section>
      <footer><span><SlidersHorizontal size={15}/> Parallel connections, torrents and magnet links powered by aria2.</span><span>LINUX FIRST · CROSS-PLATFORM ROADMAP</span></footer>
    </main>
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
      <section className="modal" role="dialog" aria-modal="true" aria-label="Download settings">
        <div className="modal-title"><div><Settings2 size={22}/><h2>Download settings</h2></div><button aria-label="Close settings" onClick={() => setSettingsOpen(false)}><X size={18}/></button></div>
        <p>These preferences are saved locally and used for new transfers.</p>
        <label><span><Zap size={16}/> Connections per server</span><input type="number" min="1" max="16" value={draft.connections} onChange={e => setDraft(s => ({ ...s, connections: Number(e.target.value) }))}/></label>
        <label><span><Download size={16}/> Maximum concurrent downloads</span><input type="number" min="1" max="10" value={draft.concurrent} onChange={e => setDraft(s => ({ ...s, concurrent: Number(e.target.value) }))}/></label>
        <label><span><Activity size={16}/> Overall speed limit (KiB/s, 0 = unlimited)</span><input type="number" min="0" value={draft.speedLimitKib} onChange={e => setDraft(s => ({ ...s, speedLimitKib: Number(e.target.value) }))}/></label>
        <label><span><Magnet size={16}/> Torrent seeding time (minutes, 0 = off)</span><input type="number" min="0" max="65535" value={draft.seedMinutes} onChange={e => setDraft(s => ({ ...s, seedMinutes: Number(e.target.value) }))}/></label>
        <label><span><FolderOpen size={16}/> Download directory</span><div className="folder-row"><input value={draft.downloadDir} onChange={e => setDraft(s => ({ ...s, downloadDir: e.target.value }))}/><button type="button" onClick={() => void pickDirectory()}>Browse</button></div></label>
        {settingsError && <p className="error" role="alert">{settingsError}</p>}
        <div className="modal-actions"><button onClick={() => setSettingsOpen(false)}>Cancel</button><button disabled={working} onClick={() => void save()}>Save settings</button></div>
      </section>
    </div>}
  </div>;
}
