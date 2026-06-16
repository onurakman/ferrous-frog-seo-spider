import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Download,
  Pause,
  Play,
  RefreshCcw,
  Search,
  Square,
} from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { create } from "zustand";

type UrlClassification = "internal" | "external";
type SortDirection = "asc" | "desc";
type IssueView =
  | "all"
  | "internal"
  | "external"
  | "status2xx"
  | "status3xx"
  | "status4xx"
  | "status5xx"
  | "noResponse"
  | "titleMissing"
  | "titleDuplicate"
  | "titleTooShort"
  | "titleTooLong"
  | "metaMissing"
  | "metaDuplicate"
  | "metaTooShort"
  | "metaTooLong"
  | "brokenLinks";

type CrawlRecord = {
  id: number;
  url: string;
  finalUrl: string;
  classification: UrlClassification;
  statusCode?: number | null;
  statusText: string;
  contentType?: string | null;
  indexability: string;
  indexabilityStatus: string;
  responseTimeMs: number;
  sizeBytes: number;
  depth: number;
  redirectTarget?: string | null;
  title?: string | null;
  titleLen: number;
  metaDescription?: string | null;
  metaDescriptionLen: number;
  h1?: string | null;
  h1Len: number;
  canonical?: string | null;
  inlinkCount: number;
  outlinkCount: number;
  internalOutlinkCount: number;
  externalOutlinkCount: number;
  error?: string | null;
};

type CrawlSummary = {
  total: number;
  internal: number;
  external: number;
  success: number;
  redirects: number;
  clientErrors: number;
  serverErrors: number;
  noResponse: number;
  broken: number;
};

type CrawlProgress = {
  status: string;
  crawled: number;
  queued: number;
  discovered: number;
  elapsedMs: number;
  pagesPerSecond: number;
  summary: CrawlSummary;
};

type CrawlerEvent = {
  kind: string;
  record?: CrawlRecord | null;
  progress?: CrawlProgress | null;
  message?: string | null;
};

type GridResponse = {
  rows: CrawlRecord[];
  total: number;
  summary: CrawlSummary;
};

type CrawlConfig = {
  startUrl: string;
  maxUrls: number;
  maxDepth: number;
  concurrency: number;
  requestDelayMs: number;
  respectRobots: boolean;
  userAgent: string;
  timeoutSecs: number;
  maxRedirects: number;
};

type AppState = {
  config: CrawlConfig;
  rows: CrawlRecord[];
  selected?: CrawlRecord;
  selectedView: IssueView;
  globalSearch: string;
  sortBy?: string;
  sortDir: SortDirection;
  total: number;
  summary: CrawlSummary;
  progress?: CrawlProgress;
  running: boolean;
  error?: string;
  setConfig: (config: Partial<CrawlConfig>) => void;
  setRows: (response: GridResponse) => void;
  setSelected: (record?: CrawlRecord) => void;
  setView: (view: IssueView) => void;
  setSearch: (search: string) => void;
  setSort: (sortBy: string) => void;
  setProgress: (progress?: CrawlProgress) => void;
  setRunning: (running: boolean) => void;
  setError: (error?: string) => void;
};

const emptySummary: CrawlSummary = {
  total: 0,
  internal: 0,
  external: 0,
  success: 0,
  redirects: 0,
  clientErrors: 0,
  serverErrors: 0,
  noResponse: 0,
  broken: 0,
};

const defaultConfig: CrawlConfig = {
  startUrl: "https://example.com/",
  maxUrls: 250,
  maxDepth: 3,
  concurrency: 4,
  requestDelayMs: 250,
  respectRobots: true,
  userAgent: "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)",
  timeoutSecs: 20,
  maxRedirects: 10,
};

const useAppStore = create<AppState>((set, get) => ({
  config: defaultConfig,
  rows: [],
  selectedView: "all",
  globalSearch: "",
  sortDir: "asc",
  total: 0,
  summary: emptySummary,
  running: false,
  setConfig: (config) =>
    set((state) => ({ config: { ...state.config, ...config } })),
  setRows: (response) =>
    set((state) => ({
      rows: response.rows,
      total: response.total,
      summary: response.summary,
      selected:
        state.selected &&
        response.rows.some((row) => row.id === state.selected?.id)
          ? state.selected
          : response.rows[0],
    })),
  setSelected: (record) => set({ selected: record }),
  setView: (view) => set({ selectedView: view }),
  setSearch: (search) => set({ globalSearch: search }),
  setSort: (sortBy) =>
    set((state) => ({
      sortBy,
      sortDir:
        state.sortBy === sortBy && state.sortDir === "asc" ? "desc" : "asc",
    })),
  setProgress: (progress) =>
    set({
      progress,
      summary: progress?.summary ?? get().summary,
    }),
  setRunning: (running) => set({ running }),
  setError: (error) => set({ error }),
}));

const views: Array<{ id: IssueView; label: string }> = [
  { id: "all", label: "All URLs" },
  { id: "internal", label: "Internal" },
  { id: "external", label: "External" },
  { id: "status2xx", label: "2xx Success" },
  { id: "status3xx", label: "Redirects" },
  { id: "status4xx", label: "4xx Errors" },
  { id: "status5xx", label: "5xx Errors" },
  { id: "noResponse", label: "No Response" },
  { id: "titleMissing", label: "Missing Titles" },
  { id: "titleDuplicate", label: "Duplicate Titles" },
  { id: "titleTooShort", label: "Short Titles" },
  { id: "titleTooLong", label: "Long Titles" },
  { id: "metaMissing", label: "Missing Meta" },
  { id: "metaDuplicate", label: "Duplicate Meta" },
  { id: "metaTooShort", label: "Short Meta" },
  { id: "metaTooLong", label: "Long Meta" },
  { id: "brokenLinks", label: "Broken Links" },
];

const columns: Array<{ key: keyof CrawlRecord; label: string; width: string }> = [
  { key: "statusCode", label: "Status", width: "76px" },
  { key: "finalUrl", label: "URL", width: "minmax(320px, 1.8fr)" },
  { key: "title", label: "Title", width: "minmax(220px, 1fr)" },
  { key: "indexability", label: "Indexability", width: "120px" },
  { key: "contentType", label: "Content Type", width: "150px" },
  { key: "responseTimeMs", label: "Time", width: "82px" },
  { key: "depth", label: "Depth", width: "70px" },
  { key: "inlinkCount", label: "Inlinks", width: "82px" },
  { key: "outlinkCount", label: "Outlinks", width: "88px" },
];

export default function App() {
  const {
    config,
    rows,
    selected,
    selectedView,
    globalSearch,
    sortBy,
    sortDir,
    total,
    summary,
    progress,
    running,
    error,
    setConfig,
    setRows,
    setSelected,
    setView,
    setSearch,
    setSort,
    setProgress,
    setRunning,
    setError,
  } = useAppStore();
  const parentRef = useRef<HTMLDivElement>(null);
  const refreshTimer = useRef<number | null>(null);

  const gridTemplate = useMemo(
    () => columns.map((column) => column.width).join(" "),
    [],
  );

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 38,
    overscan: 12,
  });

  const loadRows = async () => {
    const response = await invoke<GridResponse>("get_rows", {
      query: {
        offset: 0,
        limit: 1000,
        globalSearch,
        sortBy,
        sortDir,
        view: selectedView,
      },
    });
    setRows(response);
  };

  const scheduleRefresh = () => {
    if (refreshTimer.current !== null) {
      return;
    }
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = null;
      void loadRows();
    }, 180);
  };

  useEffect(() => {
    void loadRows();
  }, [selectedView, globalSearch, sortBy, sortDir]);

  useEffect(() => {
    const unlisten = listen<CrawlerEvent>("crawl-event", (event) => {
      const payload = event.payload;
      if (payload.kind === "started") {
        setRunning(true);
        setError(undefined);
      }
      if (payload.kind === "finished") {
        setRunning(false);
      }
      if (payload.kind === "error") {
        setError(payload.message ?? "Crawler error");
      }
      if (payload.progress) {
        setProgress(payload.progress);
      }
      scheduleRefresh();
    });

    return () => {
      void unlisten.then((dispose) => dispose());
      if (refreshTimer.current !== null) {
        window.clearTimeout(refreshTimer.current);
      }
    };
  }, [selectedView, globalSearch, sortBy, sortDir]);

  const startCrawl = async () => {
    setError(undefined);
    setRunning(true);
    await invoke("start_crawl", { config });
    await loadRows();
  };

  const pauseCrawl = async () => {
    await invoke("pause_crawl");
  };

  const resumeCrawl = async () => {
    await invoke("resume_crawl");
  };

  const stopCrawl = async () => {
    await invoke("stop_crawl");
    setRunning(false);
  };

  const exportCsv = async () => {
    const csv = await invoke<string>("export_csv", {
      query: {
        offset: 0,
        limit: 1_000_000,
        globalSearch,
        sortBy,
        sortDir,
        view: selectedView,
      },
    });
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "ferrous-frog-export.csv";
    anchor.click();
    URL.revokeObjectURL(url);
  };

  return (
    <main className="app-shell">
      <header className="toolbar">
        <div className="brand">
          <span className="brand-mark">FF</span>
          <div>
            <h1>Ferrous Frog</h1>
            <p>SEO Spider</p>
          </div>
        </div>
        <div className="url-control">
          <input
            aria-label="Seed URL"
            value={config.startUrl}
            onChange={(event) => setConfig({ startUrl: event.target.value })}
            placeholder="https://example.com/"
          />
        </div>
        <div className="crawl-controls">
          <button className="primary" onClick={startCrawl} disabled={running}>
            <Play size={16} />
            <span>Start</span>
          </button>
          <button onClick={pauseCrawl} disabled={!running} title="Pause crawl">
            <Pause size={16} />
          </button>
          <button onClick={resumeCrawl} disabled={!running} title="Resume crawl">
            <RefreshCcw size={16} />
          </button>
          <button onClick={stopCrawl} disabled={!running} title="Stop crawl">
            <Square size={16} />
          </button>
          <button onClick={exportCsv} disabled={rows.length === 0}>
            <Download size={16} />
            <span>CSV</span>
          </button>
        </div>
      </header>

      <section className="config-strip">
        <label>
          Max URLs
          <input
            type="number"
            min={1}
            value={config.maxUrls}
            onChange={(event) => setConfig({ maxUrls: Number(event.target.value) })}
          />
        </label>
        <label>
          Depth
          <input
            type="number"
            min={0}
            value={config.maxDepth}
            onChange={(event) => setConfig({ maxDepth: Number(event.target.value) })}
          />
        </label>
        <label>
          Threads
          <input
            type="number"
            min={1}
            value={config.concurrency}
            onChange={(event) => setConfig({ concurrency: Number(event.target.value) })}
          />
        </label>
        <label>
          Delay ms
          <input
            type="number"
            min={0}
            value={config.requestDelayMs}
            onChange={(event) =>
              setConfig({ requestDelayMs: Number(event.target.value) })
            }
          />
        </label>
        <label className="check-control">
          <input
            type="checkbox"
            checked={config.respectRobots}
            onChange={(event) => setConfig({ respectRobots: event.target.checked })}
          />
          Respect robots.txt
        </label>
        <div className="search-control">
          <Search size={16} />
          <input
            aria-label="Search results"
            value={globalSearch}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Search current view"
          />
        </div>
      </section>

      <section className="metrics">
        <Metric label="Crawled" value={progress?.crawled ?? summary.total} />
        <Metric label="Queued" value={progress?.queued ?? 0} />
        <Metric label="Discovered" value={progress?.discovered ?? summary.total} />
        <Metric label="Speed" value={(progress?.pagesPerSecond ?? 0).toFixed(2)} />
        <Metric label="2xx" value={summary.success} />
        <Metric label="Broken" value={summary.broken} tone="danger" />
      </section>

      {error ? <div className="error-bar">{error}</div> : null}

      <section className="workspace">
        <aside className="issue-tree">
          {views.map((view) => (
            <button
              key={view.id}
              className={selectedView === view.id ? "active" : ""}
              onClick={() => setView(view.id)}
            >
              {view.label}
            </button>
          ))}
        </aside>

        <section className="results-pane">
          <div className="grid-status">
            <span>{total.toLocaleString()} rows</span>
            <span>{progress?.status ?? "idle"}</span>
          </div>
          <div className="grid" ref={parentRef}>
            <div className="grid-header" style={{ gridTemplateColumns: gridTemplate }}>
              {columns.map((column) => (
                <button key={column.key} onClick={() => setSort(column.key)}>
                  {column.label}
                  {sortBy === column.key ? (sortDir === "asc" ? " ^" : " v") : ""}
                </button>
              ))}
            </div>
            <div
              className="grid-rows"
              style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const row = rows[virtualRow.index];
                return (
                  <button
                    key={row.id}
                    className={`grid-row ${selected?.id === row.id ? "selected" : ""}`}
                    style={{
                      transform: `translateY(${virtualRow.start}px)`,
                      gridTemplateColumns: gridTemplate,
                    }}
                    onClick={() => setSelected(row)}
                  >
                    {columns.map((column) => (
                      <span key={column.key}>{formatCell(row, column.key)}</span>
                    ))}
                  </button>
                );
              })}
            </div>
          </div>
        </section>

        <aside className="detail-panel">
          {selected ? (
            <>
              <h2>{selected.statusCode ?? "No response"} {selected.statusText}</h2>
              <p className="detail-url">{selected.finalUrl}</p>
              <dl>
                <dt>Title</dt>
                <dd>{selected.title || "Missing"}</dd>
                <dt>Meta Description</dt>
                <dd>{selected.metaDescription || "Missing"}</dd>
                <dt>H1</dt>
                <dd>{selected.h1 || "Missing"}</dd>
                <dt>Canonical</dt>
                <dd>{selected.canonical || "Missing"}</dd>
                <dt>Indexability</dt>
                <dd>{selected.indexabilityStatus}</dd>
                <dt>Links</dt>
                <dd>
                  {selected.internalOutlinkCount} internal,{" "}
                  {selected.externalOutlinkCount} external
                </dd>
                <dt>Error</dt>
                <dd>{selected.error || "None"}</dd>
              </dl>
            </>
          ) : (
            <div className="empty-detail">Select a URL</div>
          )}
        </aside>
      </section>
    </main>
  );
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string;
  value: string | number;
  tone?: "danger";
}) {
  return (
    <div className={tone === "danger" ? "metric danger" : "metric"}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function formatCell(row: CrawlRecord, key: keyof CrawlRecord) {
  const value = row[key];
  if (value === null || value === undefined || value === "") {
    return " ";
  }
  if (key === "responseTimeMs") {
    return `${value} ms`;
  }
  if (key === "sizeBytes") {
    return `${value} B`;
  }
  return String(value);
}
