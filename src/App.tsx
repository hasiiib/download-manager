import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowDownToLine, CheckCircle2, Clock3, Download, HardDrive, Link2, Plus, Search, Settings2, ShieldCheck, XCircle, Zap } from "lucide-react";

type Status = "queued" | "downloading" | "completed" | "failed";
type Item = { id: string; url: string; name: string; status: Status; received: number; total?: number; filePath?: string; error?: string };
type Progress = { id: string; received: number; total?: number; status: Status; filePath?: string; error?: string };
const formatSize = (n?: number) => n === undefined ? "Unknown size" : n < 1024 ? n + " B" : n < 1048576 ? (n / 1024).toFixed(1) + " KB" : n < 1073741824 ? (n / 1048576).toFixed(1) + " MB" : (n / 1073741824).toFixed(2) + " GB";
const nameFromUrl = (raw: string) => { try { return decodeURIComponent(new URL(raw).pathname.split("/").pop() || "download"); } catch { return "Download"; } };

export default function App() {
  const [items, setItems] = useState<Item[]>([]);
  const [url, setUrl] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | Status>("all");
  const [message, setMessage] = useState("");
  useEffect(() => {
    let active = true;
    let off: (() => void) | undefined;
    listen<Progress>("download-progress", event => {
      if (!active) return;
      const p = event.payload;
      setItems(prev => prev.map(item => item.id === p.id ? { ...item, status: p.status, received: p.received || item.received, total: p.total ?? item.total, filePath: p.filePath ?? item.filePath, error: p.error ?? item.error } : item));
    }).then(unlisten => { if (active) off = unlisten; else unlisten(); }).catch(console.error);
    return () => { active = false; off?.(); };
  }, []);
  const add = async () => {
    setMessage("");
    let parsed: URL;
    try { parsed = new URL(url.trim()); if (!["http:", "https:"].includes(parsed.protocol)) throw new Error("scheme"); }
    catch { setMessage("Enter a valid HTTP or HTTPS download URL."); return; }
    const id = crypto.randomUUID();
    setItems(prev => [{ id, url: parsed.toString(), name: nameFromUrl(parsed.toString()), status: "queued", received: 0 }, ...prev]);
    setUrl("");
    try { await invoke("start_download", { id, url: parsed.toString() }); }
    catch (error) { setItems(prev => prev.map(item => item.id === id ? { ...item, status: "failed", error: String(error) } : item)); }
  };
  const shown = useMemo(() => items.filter(x => (filter === "all" || x.status === filter) && (x.name.toLowerCase().includes(query.toLowerCase()) || x.url.toLowerCase().includes(query.toLowerCase()))), [items, filter, query]);
  const active = items.filter(x => x.status === "downloading" || x.status === "queued").length;
  const completed = items.filter(x => x.status === "completed").length;
  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-icon"><ArrowDownToLine size={22}/></div><div><strong>Download<span>Manager</span></strong><small>YOUR DOWNLOAD HUB</small></div></div>
      <div className="nav-label">WORKSPACE</div>
      <button className={filter === "all" ? "nav active" : "nav"} onClick={() => setFilter("all")}><HardDrive size={18}/> All downloads <b>{items.length}</b></button>
      <button className={filter === "downloading" ? "nav active" : "nav"} onClick={() => setFilter("downloading")}><Download size={18}/> Downloading <b>{active}</b></button>
      <button className={filter === "completed" ? "nav active" : "nav"} onClick={() => setFilter("completed")}><CheckCircle2 size={18}/> Completed <b>{completed}</b></button>
      <button className={filter === "failed" ? "nav active" : "nav"} onClick={() => setFilter("failed")}><XCircle size={18}/> Failed</button>
      <div className="sidebar-bottom"><div className="side-note"><ShieldCheck size={20}/><div><strong>Local by design</strong><p>Downloads stay on your device.</p></div></div><div className="version">PREVIEW BUILD · v0.1.0</div></div>
    </aside>
    <main className="main">
      <header><div className="crumb">WORKSPACE <span>/</span> DOWNLOADS</div><div className="badge"><span className="dot"/> Ubuntu Preview</div></header>
      <section className="heading"><div><div className="eyebrow"><Zap size={14}/> YOUR DOWNLOAD CONTROL CENTER</div><h1>Downloads<span>.</span></h1><p>Everything you download, in one beautiful place.</p></div></section>
      <section className="stats"><div className="stat"><div className="stat-icon violet"><HardDrive size={21}/></div><div><small>Total downloads</small><strong>{items.length}</strong></div></div><div className="stat"><div className="stat-icon blue"><Download size={21}/></div><div><small>In progress</small><strong>{active}</strong></div></div><div className="stat"><div className="stat-icon green"><CheckCircle2 size={21}/></div><div><small>Completed</small><strong>{completed}</strong></div></div></section>
      <section className="add-box"><div className="section-title"><div><Link2 size={18}/><h2>New download</h2></div><span>HTTP / HTTPS</span></div><div className="input-row"><input value={url} onChange={e => setUrl(e.target.value)} onKeyDown={e => { if (e.key === "Enter") void add(); }} placeholder="Paste a download link here..."/><button onClick={() => void add()}><Plus size={18}/> Add download</button></div>{message && <p className="error">{message}</p>}<p className="hint">Files save to your ~/Downloads folder. This preview supports direct HTTP(S) files.</p></section>
      <section className="list"><div className="list-head"><div><h2>Download activity</h2><p>See what's happening with your files</p></div><div className="search"><Search size={17}/><input placeholder="Search downloads" value={query} onChange={e => setQuery(e.target.value)}/></div></div>
        {shown.length === 0 ? <div className="empty"><div><ArrowDownToLine size={30}/></div><h3>No downloads here yet</h3><p>Paste a direct link above to start your first download.</p></div> : <div className="rows">{shown.map(item => <article className="row" key={item.id}><div className="file-icon"><Download size={20}/></div><div className="file-info"><strong title={item.url}>{item.name}</strong><small title={item.filePath || item.error || item.url}>{item.error || item.filePath || item.url}</small><div className="progress"><span style={{ width: item.status === "completed" ? "100%" : item.total ? Math.min(100, item.received / item.total * 100) + "%" : "0%" }}/></div></div><div className="row-meta"><span className={"status " + item.status}>{item.status === "downloading" ? <Clock3 size={13}/> : item.status === "completed" ? <CheckCircle2 size={13}/> : item.status === "failed" ? <XCircle size={13}/> : <Clock3 size={13}/>} {item.status}</span><small>{formatSize(item.received)}{item.total ? " / " + formatSize(item.total) : ""}</small></div></article>)}</div>}
      </section><footer><span><Settings2 size={15}/> More features on the roadmap: pause/resume, segmentation, torrent & browser capture.</span><span>BUILT FOR LINUX FIRST</span></footer>
    </main>
  </div>;
}
