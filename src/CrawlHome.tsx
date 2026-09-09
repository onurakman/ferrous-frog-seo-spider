import { ArrowRight, ChevronUp, FolderOpen, GitCompareArrows, Globe, LoaderCircle, RefreshCw, Search, Settings2, Trash2 } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import "./crawl-home.css";

export type SavedCrawl = {
  id: string;
  name: string;
  startUrl: string;
  databasePath: string;
  createdAtMs: number;
  updatedAtMs: number;
  isCurrent: boolean;
  mode?: "spider" | "list";
  status?: string;
  crawled?: number | null;
};

type CrawlHomeProps = {
  startUrl: string;
  mode: "spider" | "list";
  scope: string;
  scopes: Array<{ id: string; label: string }>;
  listUrls: string[];
  listSitemapCount: number;
  sessions: SavedCrawl[];
  loading: boolean;
  error?: string;
  busy: boolean;
  running: boolean;
  desktop: boolean;
  workspaceAvailable: boolean;
  selectedIds: string[];
  onStartUrlChange(value: string): void;
  onModeChange(mode: "spider" | "list"): void;
  onScopeChange(value: string): void;
  onListUrlsChange(urls: string[]): void;
  onStart(): void;
  onOpen(id: string): void;
  onDelete(id: string, origin: HTMLButtonElement): void;
  onSelect(id: string): void;
  onCompare(): void;
  onRefresh(): void;
  onSettings(): void;
  onReturn(): void;
};

const dateFormat = new Intl.DateTimeFormat(undefined, {
  day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit",
});
const historyPreferenceKey = "ferrous-frog-history-open";

export default function CrawlHome({
  startUrl, mode, scope, scopes, listUrls, listSitemapCount, sessions, loading, error,
  busy, running, desktop, workspaceAvailable, selectedIds, onStartUrlChange, onModeChange,
  onScopeChange, onListUrlsChange, onStart, onOpen, onDelete, onSelect, onCompare, onRefresh,
  onSettings, onReturn,
}: CrawlHomeProps) {
  const [search, setSearch] = useState("");
  const [visibleCount, setVisibleCount] = useState(12);
  const [historyOpen, setHistoryOpen] = useState(() => {
    try { return localStorage.getItem(historyPreferenceKey) === "true"; } catch { return false; }
  });
  const [preferenceError, setPreferenceError] = useState(false);
  const historyToggle = useRef<HTMLButtonElement>(null);
  const changeHistoryOpen = (open: boolean) => {
    if (!open) historyToggle.current?.focus();
    setHistoryOpen(open);
    try { localStorage.setItem(historyPreferenceKey, String(open)); setPreferenceError(false); }
    catch { setPreferenceError(true); }
  };
  const query = search.trim().toLowerCase();
  const filteredSessions = useMemo(() => sessions.filter((session) =>
    [session.name, session.startUrl, session.mode, session.status].join(" ").toLowerCase().includes(query),
  ), [sessions, query]);
  const locked = busy || running;
  const listCount = listUrls.filter((url) => url.trim()).length;
  const hasListInputs = mode === "list" && (listCount > 0 || listSitemapCount > 0);
  const hasTarget = Boolean(startUrl.trim()) || hasListInputs;

  return <section className="crawl-home" aria-label="Crawl library" data-history-open={historyOpen}>
    <div className="crawl-home-content">
      <section className="crawl-launcher" aria-labelledby="crawl-launcher-title">
        <div className="crawl-launcher-heading">
          <img src="/brand/ferrous-frog.png" width={48} height={48} alt="" />
          <h2 id="crawl-launcher-title">Start a new crawl</h2>
        </div>
        <form onSubmit={(event) => {
          event.preventDefault();
          if (!locked && desktop && hasTarget) onStart();
        }}>
          <fieldset className="crawl-launcher-fields" disabled={locked}>
            <legend className="sr-only">New crawl</legend>
            <div className="crawl-launcher-target">
              <label className="crawl-launcher-url">
                <span>{mode === "list" ? "Root URL (optional with a URL list)" : "Website URL"}</span>
                <input
                  aria-label="Crawl URL"
                  type="url"
                  inputMode="url"
                  autoComplete="url"
                  spellCheck={false}
                  placeholder="https://example.com"
                  value={startUrl}
                  required={!hasListInputs}
                  pattern="[Hh][Tt][Tt][Pp][Ss]?://.+"
                  title="Enter a full URL beginning with https:// or http://."
                  onChange={(event) => onStartUrlChange(event.currentTarget.value)}
                />
              </label>
              <label className="crawl-launcher-mode">
                <span>Crawl type</span>
                <select aria-label="Crawl mode" value={mode} onChange={(event) => onModeChange(event.currentTarget.value as "spider" | "list")}>
                  <option value="spider">Spider</option>
                  <option value="list">List</option>
                </select>
              </label>
              <button className="primary crawl-launcher-start" type="submit" data-action="start-new-crawl" disabled={locked || !desktop || !hasTarget}>
                Start crawl <ArrowRight size={16} aria-hidden="true" />
              </button>
            </div>
            {mode === "list" && <label className="crawl-launcher-list">
              <span>URLs to crawl <span className="crawl-home-muted">· one per line</span></span>
              <textarea
                aria-label="List URLs"
                aria-describedby="crawl-list-help"
                rows={4}
                spellCheck={false}
                placeholder={"https://example.com/page-one\nhttps://example.com/page-two"}
                value={listUrls.join("\n")}
                onChange={(event) => onListUrlsChange(event.currentTarget.value.split("\n"))}
              />
              <span id="crawl-list-help" className="crawl-home-muted">
                {listCount.toLocaleString()} URL{listCount === 1 ? "" : "s"}
                {listSitemapCount > 0 ? ` · ${listSitemapCount.toLocaleString()} sitemap source${listSitemapCount === 1 ? "" : "s"} in settings` : " · Import files or add sitemap sources in settings."}
              </span>
            </label>}
            <div className="crawl-launcher-options">
              {mode === "spider" && scopes.length > 0 ? <label className="crawl-launcher-scope">
                <span>Scope</span>
                <select aria-label="Crawl scope" value={scope} onChange={(event) => onScopeChange(event.currentTarget.value)}>
                  {scopes.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}
                </select>
              </label> : <span className="crawl-home-muted">{mode === "list" ? "List mode fetches only the URLs you provide." : "Follow links from your starting URL."}</span>}
              <button className="crawl-settings-button" type="button" onClick={onSettings}>
                <Settings2 size={15} aria-hidden="true" /> Crawl settings
              </button>
            </div>
          </fieldset>
        </form>
        {!desktop && <p className="crawl-launcher-note">Open the desktop app to run crawls and access your saved results.</p>}
        {(workspaceAvailable || running) && <div className="crawl-workspace-return">
          <button type="button" data-action="return-to-workspace" disabled={busy} onClick={onReturn}>
            {running ? "Return to active crawl" : "Return to results"} <ArrowRight size={15} aria-hidden="true" />
          </button>
        </div>}
      </section>
    </div>

      <section className="crawl-history" aria-labelledby="crawl-history-title" aria-busy={loading} onKeyDown={(event) => {
        if (event.key === "Escape" && historyOpen && !event.defaultPrevented) {
          event.preventDefault();
          event.stopPropagation();
          changeHistoryOpen(false);
        }
      }}>
        <h2 id="crawl-history-title" className="crawl-history-heading">
          <button ref={historyToggle} type="button" className="crawl-history-toggle" aria-label="Saved crawls" aria-controls="crawl-history-panel" aria-expanded={historyOpen} onClick={() => changeHistoryOpen(!historyOpen)}>
            <FolderOpen size={17} aria-hidden="true" />
            Saved crawls <span className="crawl-history-count">{sessions.length.toLocaleString()}</span>
            {loading ? <LoaderCircle size={15} className="crawl-home-spinner" aria-hidden="true" /> : <ChevronUp size={17} className="crawl-history-chevron" aria-hidden="true" />}
          </button>
        </h2>
        {preferenceError && <p className="crawl-history-preference-error" role="status">Could not save the panel preference.</p>}
        {error && <div className="crawl-history-error" role="alert">
          <p>{error}</p>
          <button type="button" disabled={loading || locked || !desktop} onClick={onRefresh}>Try again</button>
        </div>}
        <div className="crawl-history-body" id="crawl-history-panel" inert={!historyOpen} aria-hidden={!historyOpen}>
        <div className="crawl-history-clip"><div className="crawl-history-scroll">
        <div className="crawl-history-header">
          <div className="crawl-history-tools">
            <label className="crawl-history-search">
              <Search size={16} aria-hidden="true" />
              <input aria-label="Search saved crawls" type="search" placeholder="Search crawls" value={search} onChange={(event) => {
                setSearch(event.currentTarget.value);
                setVisibleCount(12);
              }} />
            </label>
            <button className="crawl-history-refresh" type="button" aria-label="Refresh saved crawls" title="Refresh saved crawls" disabled={loading || locked || !desktop} onClick={onRefresh}>
              <RefreshCw size={16} className={loading ? "crawl-home-spinner" : undefined} aria-hidden="true" />
            </button>
          </div>
        </div>
        <div className="crawl-history-comparison">
          <p aria-live="polite"><strong>{selectedIds.length} of 2 selected</strong><span>The older crawl is the baseline.</span></p>
          <button type="button" data-action="compare-saved-crawls" disabled={selectedIds.length !== 2 || locked || !desktop} onClick={onCompare}>
            <GitCompareArrows size={16} aria-hidden="true" /> Compare crawls
          </button>
        </div>
        {loading && sessions.length === 0 ? <div className="crawl-history-empty" role="status">
          <LoaderCircle size={22} className="crawl-home-spinner" aria-hidden="true" />
          <p>Loading saved crawls…</p>
        </div> : <>
          {filteredSessions.length > 0 && <ul className="crawl-card-grid" aria-label="Saved crawls">
            {filteredSessions.slice(0, visibleCount).map((session) => {
              let host = "Saved crawl";
              let address = session.startUrl || "No seed URL recorded";
              try {
                const url = new URL(session.startUrl);
                host = url.host;
                address = `${url.host}${url.pathname}${url.search}${url.hash}`;
              } catch { /* Legacy sessions may not have a seed URL. */ }
              const name = session.name || host;
              const selected = selectedIds.includes(session.id);
              const status = session.status || "saved";
              const date = new Date(session.createdAtMs);
              return <li className={`crawl-card${selected ? " is-selected" : ""}`} data-session-id={session.id} key={session.id}>
                <div className="crawl-card-heading">
                  <span className="crawl-card-icon"><Globe size={16} aria-hidden="true" /></span>
                  <div>
                    <h3 title={name}>{name}</h3>
                    <p title={session.startUrl}>{address}</p>
                  </div>
                  <button type="button" className="crawl-card-delete" data-action="delete-saved-crawl" aria-label={`Delete crawl ${name}`} title="Delete crawl" disabled={locked || !desktop} onClick={(event) => onDelete(session.id, event.currentTarget)}>
                    <Trash2 size={14} aria-hidden="true" />
                  </button>
                </div>
                <div className="crawl-card-meta">
                  <div className="crawl-card-tags">
                    <span className="crawl-card-mode">{session.mode === "list" ? "List" : session.mode === "spider" ? "Spider" : "Crawl"}</span>
                    <span className="crawl-card-status" data-status={status}>{status === "running" ? "Crawling" : status.charAt(0).toUpperCase() + status.slice(1)}</span>
                    {session.isCurrent && <span className="crawl-card-current">Current</span>}
                  </div>
                  <p className="crawl-card-results"><strong>{session.crawled == null ? "Saved" : session.crawled.toLocaleString()}</strong> {session.crawled == null ? "results" : "URLs"}</p>
                </div>
                <time className="crawl-card-date" dateTime={date.toISOString()} title={`Created ${date.toLocaleString()}`}>{dateFormat.format(date)}</time>
                <div className="crawl-card-actions">
                  <label className="crawl-card-select">
                    <input
                      type="checkbox"
                      aria-label={`Select ${name} for comparison`}
                      checked={selected}
                      disabled={locked || !desktop || (!selected && selectedIds.length >= 2)}
                      onChange={() => onSelect(session.id)}
                    />
                    Compare
                  </label>
                  <button type="button" data-action="open-saved-crawl" aria-label={`Open crawl ${name}`} disabled={locked || !desktop} onClick={() => onOpen(session.id)}>
                    Open <ArrowRight size={14} aria-hidden="true" />
                  </button>
                </div>
              </li>;
            })}
          </ul>}
          {!loading && !error && filteredSessions.length === 0 && <div className="crawl-history-empty">
            <FolderOpen size={25} aria-hidden="true" />
            <h3>{query ? "No matching crawls" : "No saved crawls yet"}</h3>
            <p>{query ? "Try a different website, crawl name or status." : desktop ? "Start a crawl above. Your results will appear here." : "Your crawl history is available in the desktop app."}</p>
            {query && <button type="button" onClick={() => { setSearch(""); setVisibleCount(12); }}>Clear search</button>}
          </div>}
          {filteredSessions.length > 0 && <div className="crawl-history-pagination">
            <span>{Math.min(visibleCount, filteredSessions.length).toLocaleString()} of {filteredSessions.length.toLocaleString()} {query ? "matching " : ""}crawls</span>
            {filteredSessions.length > visibleCount && <button type="button" onClick={() => setVisibleCount((count) => count + 12)}>Show more</button>}
          </div>}
        </>}
        </div></div>
        </div>
      </section>
  </section>;
}
