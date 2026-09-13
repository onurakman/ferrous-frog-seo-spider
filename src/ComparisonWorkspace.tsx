import { invoke } from "@tauri-apps/api/core";
import * as Checkbox from "@radix-ui/react-checkbox";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Check, ChevronLeft, ChevronRight, Download, RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { CrawlRecord } from "./App";
import type { SavedCrawl } from "./CrawlHome";
import * as Dialog from "./Dialog";

type ComparisonRow = {
  key: number;
  identityKey: string;
  occurrence: number;
  previousUrl?: string | null;
  currentUrl?: string | null;
  previousFinalUrl?: string | null;
  currentFinalUrl?: string | null;
  previousListPosition?: number | null;
  currentListPosition?: number | null;
  url: string;
  change: string;
  previousStatusCode?: number | null;
  currentStatusCode?: number | null;
  previousTitle?: string | null;
  currentTitle?: string | null;
  previousMetaDescription?: string | null;
  currentMetaDescription?: string | null;
  previousIndexability?: string | null;
  currentIndexability?: string | null;
  previousResponseHash?: string | null;
  currentResponseHash?: string | null;
  changedFields: string[];
  contentComparison: string;
};
type ComparisonSummary = {
  baselineRecords: number; currentRecords: number; added: number; removed: number; changed: number;
  statusChanged: number; titleChanged: number; metaDescriptionChanged: number; indexabilityChanged: number;
  hashChanged: number; contentChanged: number; responseOnly: number; contentUnavailable: number;
  metricDeltas: { label: string; previous: number; current: number; delta: number }[];
};
type ComparisonQuery = {
  search: string; change: string; changedField?: string; includeResponseOnly: boolean;
  sortBy: string; sortDir: "asc" | "desc"; offset: number; limit: number;
};
type ComparisonPage = { summary: ComparisonSummary; total: number; offset: number; limit: number; rows: ComparisonRow[] };
type ComparisonDetail = { row: ComparisonRow; previous?: CrawlRecord | null; current?: CrawlRecord | null };
// Keep native preparations ordered across close/reopen; an older async handler must not replace a newer workspace.
let preparationQueue = Promise.resolve();
const initialQuery: ComparisonQuery = { search: "", change: "all", includeResponseOnly: false, sortBy: "url", sortDir: "asc", offset: 0, limit: 100 };
const fieldLabels: Record<string, string> = {
  finalUrl: "Final destination", statusCode: "HTTP status", title: "Title", metaDescription: "Meta description", indexability: "Indexability",
  headings: "Headings", canonical: "Canonical", robotsDirectives: "Robots directives", content: "Content", responseHash: "Response hash",
};
const metricLabels = {
  baselineRecords: "Baseline", currentRecords: "Current", added: "Added", removed: "Removed", changed: "Changed",
  statusChanged: "Status", titleChanged: "Titles", metaDescriptionChanged: "Descriptions", indexabilityChanged: "Indexability",
  contentChanged: "Content changes", responseOnly: "Response-only", contentUnavailable: "Content unavailable",
} as const;
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const changeLabel = (value: string) => value === "responseOnly" ? "Response only" : value.charAt(0).toUpperCase() + value.slice(1);
const reasons = (row: ComparisonRow) => row.changedFields.map((field) => fieldLabels[field] ?? field).join(", ") || (row.change === "added" ? "New URL" : row.change === "removed" ? "Removed URL" : "None");
const hashLabel = (hash?: string | null) => hash ? hash.slice(0, 10) + (hash.length > 10 ? "…" : "") : "Unavailable";

export default function ComparisonWorkspace({ open, onOpenChange, sessions }: {
  open: boolean; onOpenChange: (open: boolean) => void; sessions?: [SavedCrawl, SavedCrawl];
}) {
  const [archivePath, setArchivePath] = useState("");
  const [page, setPage] = useState<ComparisonPage>();
  const [query, setQuery] = useState(initialQuery);
  const [pageDraft, setPageDraft] = useState("1");
  const [preparing, setPreparing] = useState(false);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const [selectedKey, setSelectedKey] = useState<number>();
  const [detail, setDetail] = useState<ComparisonDetail>();
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailsExpanded, setDetailsExpanded] = useState(false);
  const comparisonId = useRef<string | undefined>(undefined);
  const generation = useRef(0);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const detailGeneration = useRef(0);
  const tableScroll = useRef<HTMLDivElement>(null);
  const rows = page?.rows ?? [];
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => tableScroll.current, estimateSize: () => 38, overscan: 6 });

  const clearDetail = () => {
    detailGeneration.current += 1;
    setSelectedKey(undefined); setDetail(undefined); setDetailLoading(false); setDetailsExpanded(false);
  };
  const closeSession = () => {
    clearTimeout(searchTimer.current);
    generation.current += 1;
    detailGeneration.current += 1;
    const id = comparisonId.current;
    comparisonId.current = undefined;
    if (id) void invoke("close_crawl_comparison", { comparisonId: id }).catch(() => {});
  };
  const prepare = async () => {
    if (!sessions && !archivePath.trim()) return;
    closeSession(); clearDetail();
    const id = crypto.randomUUID();
    comparisonId.current = id;
    const version = ++generation.current;
    setPreparing(true); setLoading(false); setExporting(false); setPage(undefined); setQuery(initialQuery); setError(undefined); setNotice(undefined);
    preparationQueue = preparationQueue.then(async () => {
      if (version !== generation.current || comparisonId.current !== id) return;
      try {
        const result = await invoke<ComparisonPage>("open_crawl_comparison", { request: {
          comparisonId: id, ...(sessions ? { baselineSessionId: sessions[0].id, currentSessionId: sessions[1].id } : { archivePath: archivePath.trim() }),
        } });
        if (version === generation.current) setPage(result);
        else await invoke("close_crawl_comparison", { comparisonId: id }).catch(() => {});
      } catch (caught) {
        if (version === generation.current) setError(errorText(caught));
      } finally {
        if (version === generation.current) setPreparing(false);
      }
    });
    await preparationQueue;
  };
  useEffect(() => {
    if (open) {
      setPage(undefined); setQuery(initialQuery); setError(undefined); setNotice(undefined); clearDetail();
      setPreparing(false); setLoading(false); setExporting(false);
      if (sessions) void prepare();
    } else closeSession();
    return closeSession;
    // Source selection only changes when the workspace opens. Archive edits use the Compare button.
  }, [open, sessions]);

  const runQuery = async (next: ComparisonQuery) => {
    clearTimeout(searchTimer.current);
    const id = comparisonId.current;
    if (!id || preparing || !page) return;
    const version = ++generation.current;
    clearDetail(); setQuery(next); setLoading(true); setError(undefined); setNotice(undefined); setExporting(false);
    setPage((current) => current ? { ...current, rows: [] } : current);
    tableScroll.current?.scrollTo({ top: 0 });
    try {
      const result = await invoke<ComparisonPage>("query_crawl_comparison", { comparisonId: id, query: next });
      if (version === generation.current) setPage(result);
    } catch (caught) {
      if (version === generation.current) setError(errorText(caught));
    } finally {
      if (version === generation.current) setLoading(false);
    }
  };
  useEffect(() => setPageDraft(String(Math.floor(query.offset / query.limit) + 1)), [query.offset, query.limit]);
  const updateQuery = (patch: Partial<ComparisonQuery>) => void runQuery({ ...query, ...patch, offset: 0 });
  const searchChanges = (search: string) => {
    clearTimeout(searchTimer.current);
    const next = { ...query, search, offset: 0 };
    generation.current += 1;
    clearDetail(); setQuery(next); setLoading(true); setError(undefined); setNotice(undefined); setExporting(false);
    setPage((current) => current ? { ...current, rows: [] } : current);
    searchTimer.current = setTimeout(() => void runQuery(next), 250);
  };
  const goToPage = () => {
    const target = Number(pageDraft);
    if (Number.isInteger(target) && target >= 1 && target <= Math.ceil((page?.total ?? 0) / query.limit)) {
      if ((target - 1) * query.limit !== query.offset) void runQuery({ ...query, offset: (target - 1) * query.limit });
    } else setPageDraft(String(Math.floor(query.offset / query.limit) + 1));
  };
  const selectRow = async (row: ComparisonRow) => {
    const id = comparisonId.current;
    if (!id) return;
    const version = ++detailGeneration.current;
    setSelectedKey(row.key); setDetail(undefined); setDetailLoading(true); setError(undefined);
    try {
      const result = await invoke<ComparisonDetail>("get_crawl_comparison_detail", { comparisonId: id, key: row.key });
      if (version === detailGeneration.current) setDetail(result);
    } catch (caught) {
      if (version === detailGeneration.current) setError(errorText(caught));
    } finally {
      if (version === detailGeneration.current) setDetailLoading(false);
    }
  };
  const exportRows = async () => {
    const id = comparisonId.current;
    if (!id || !page) return;
    const version = generation.current;
    setExporting(true); setError(undefined); setNotice(undefined);
    try {
      const result = await invoke<{ path: string; rowCount: number }>("export_crawl_comparison", { comparisonId: id, query });
      if (version === generation.current) setNotice(`Exported ${result.rowCount.toLocaleString()} matching changes to ${result.path}`);
    } catch (caught) {
      if (version === generation.current) setError(errorText(caught));
    } finally {
      if (version === generation.current) setExporting(false);
    }
  };
  const virtualRows = virtualizer.getVirtualItems();
  const first = virtualRows[0];
  const last = virtualRows.at(-1);
  return <Dialog.Root open={open} onOpenChange={onOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay className="modal-backdrop" />
      <Dialog.Content className="comparison-modal" inert={!open} aria-describedby="comparison-description">
        <div className="modal-header">
          <Dialog.Title asChild><h2>Crawl Comparison</h2></Dialog.Title>
          <Dialog.Close asChild><button title="Close crawl comparison"><X size={16} /></button></Dialog.Close>
        </div>
        <div className={`comparison-body${detailsExpanded ? " details-expanded" : ""}`}>
          <div className="comparison-controls">
            {sessions ? <div className="comparison-sessions">
              {sessions.map((session, index) => <span key={session.id}><small>{index ? "Current" : "Baseline"}</small><strong>{session.name}</strong><time>{new Date(session.createdAtMs).toLocaleString()}</time></span>)}
            </div> : <label>Baseline crawl archive<input aria-label="Baseline crawl archive" value={archivePath} placeholder="/path/to/crawl.ffcrawl.json" onChange={(event) => {
              closeSession(); clearDetail(); setPage(undefined); setPreparing(false); setLoading(false); setExporting(false); setArchivePath(event.target.value);
            }} /></label>}
            <button className="settings-action-button primary" onClick={() => void prepare()} disabled={preparing || (!sessions && !archivePath.trim())}>
              <RefreshCw size={15} /><span>{preparing ? "Preparing…" : page ? "Compare again" : "Compare"}</span>
            </button>
          </div>
          {error ? <p className="comparison-feedback error-bar" role="alert">{error}</p> : null}
          {notice ? <p className="comparison-feedback notice-bar" role="status">{notice}</p> : null}
          <div className="comparison-options">
            <label className="checkbox-field"><Checkbox.Root className="checkbox-root" checked={query.includeResponseOnly} disabled={!page || preparing} onCheckedChange={(checked) => updateQuery({ includeResponseOnly: checked === true })}><Checkbox.Indicator><Check size={12} /></Checkbox.Indicator></Checkbox.Root><span>Show response-only changes</span></label>
            <p id="comparison-description">Changed tracks captured SEO fields, comparable HTML text and file content. Response-only differences may come from markup or scripts.</p>
            <details><summary>Content availability and matching</summary><p>Older or incompatible captures cannot establish content changes. Capture both crawls with matching content and rendering settings. Rows match normalized requested URLs and preserve repeated occurrences. Reordered unique List URLs stay paired; repeated inputs are paired in their saved order. Redirect sources remain separate even when they share a final destination.</p></details>
          </div>
          {page ? <>
            <details className="comparison-overview" open><summary>All crawl changes</summary>
              <div className="comparison-summary">{Object.entries(metricLabels).map(([key, label]) => <div className="metric" key={key}><span>{label}</span><strong>{page.summary[key as keyof typeof metricLabels].toLocaleString()}</strong></div>)}</div>
              {page.summary.metricDeltas.length ? <details className="comparison-audit-totals"><summary>Audit totals</summary><div className="comparison-deltas">{page.summary.metricDeltas.map((metric) => <div key={metric.label}><span>{metric.label}</span><strong>{metric.delta > 0 ? "+" : ""}{metric.delta.toLocaleString()}</strong><em>{metric.previous.toLocaleString()} to {metric.current.toLocaleString()}</em></div>)}</div></details> : null}
            </details>
            <div className="comparison-filters">
              <label>Search<input aria-label="Search comparison" value={query.search} maxLength={1000} placeholder="URL or title" onChange={(event) => searchChanges(event.target.value)} /></label>
              <label>Change<select aria-label="Comparison change" value={query.change} onChange={(event) => updateQuery({ change: event.target.value })}>{["all", "added", "removed", "changed", "responseOnly"].map((value) => <option key={value} value={value}>{value === "all" ? "All changes" : changeLabel(value)}</option>)}</select></label>
              <label>Changed field<select aria-label="Comparison changed field" value={query.changedField ?? ""} onChange={(event) => updateQuery({ changedField: event.target.value || undefined })}><option value="">Any field</option>{Object.entries(fieldLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
              <label>Sort by<select aria-label="Comparison sort" value={query.sortBy} onChange={(event) => updateQuery({ sortBy: event.target.value })}>{Object.entries({ url: "URL", change: "Change", previousStatusCode: "Previous status", currentStatusCode: "Current status", previousTitle: "Previous title", currentTitle: "Current title" }).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
              <label>Order<select aria-label="Comparison sort direction" value={query.sortDir} onChange={(event) => updateQuery({ sortDir: event.target.value as "asc" | "desc" })}><option value="asc">Ascending</option><option value="desc">Descending</option></select></label>
              <button className="settings-action-button" onClick={() => void exportRows()} disabled={exporting || loading || !page.total}><Download size={15} />{exporting ? "Exporting…" : "Export filtered CSV"}</button>
            </div>
            <div className="comparison-results">
              <div className="comparison-pagination" aria-label="Comparison pagination">
                <span className="comparison-limit" role="status">{loading ? "Loading changes…" : page.total ? `${(page.offset + 1).toLocaleString()}–${Math.min(page.offset + rows.length, page.total).toLocaleString()} of ${page.total.toLocaleString()} matching changes` : "No matching changes."}</span>
                <button title="Previous comparison page" disabled={loading || query.offset === 0} onClick={() => void runQuery({ ...query, offset: Math.max(0, query.offset - query.limit) })}><ChevronLeft size={16} /></button>
                <button title="Next comparison page" disabled={loading || query.offset + query.limit >= page.total} onClick={() => void runQuery({ ...query, offset: query.offset + query.limit })}><ChevronRight size={16} /></button>
                <label>Page<input aria-label="Comparison page" type="number" min={1} max={Math.max(1, Math.ceil(page.total / query.limit))} value={pageDraft} disabled={loading || !page.total} onChange={(event) => setPageDraft(event.target.value)} onBlur={goToPage} onKeyDown={(event) => {
                  if (event.key === "Enter") { event.preventDefault(); goToPage(); }
                }} /></label>
              </div>
              <div className="link-report-table-wrap comparison-table-wrap" ref={tableScroll} aria-busy={loading}>
                <table className="link-report-table comparison-table" role="grid" aria-label="Comparison changes" aria-rowcount={page.total + 1}>
                  <thead><tr>{["Change", "Requested URL", "Occurrence", "List position", "Final destination", "Changed fields", "Content comparison", "Status", "Title", "Meta description", "Indexability", "Response hash"].map((label) => <th key={label}>{label}</th>)}</tr></thead>
                  <tbody>
                    {first?.start ? <tr aria-hidden="true"><td colSpan={12} style={{ height: first.start, padding: 0, border: 0 }} /></tr> : null}
                    {virtualRows.map((item) => {
                      const row = rows[item.index];
                      return <tr key={row.key} data-comparison-key={row.key} data-comparison-identity={row.identityKey} aria-rowindex={page.offset + item.index + 2} aria-selected={selectedKey === row.key} tabIndex={selectedKey === row.key || (selectedKey === undefined && item.index === 0) ? 0 : -1} onClick={() => void selectRow(row)} onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") { event.preventDefault(); void selectRow(row); }
                        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
                          event.preventDefault();
                          const index = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, item.index + (event.key === "ArrowDown" ? 1 : -1)));
                          virtualizer.scrollToIndex(index); void selectRow(rows[index]);
                          requestAnimationFrame(() => tableScroll.current?.querySelector<HTMLElement>(`[data-comparison-key="${rows[index].key}"]`)?.focus());
                        }
                      }}>
                        <td><span className={`severity-pill ${row.change === "removed" ? "error" : row.change === "changed" ? "warning" : "info"}`}>{changeLabel(row.change)}</span></td>
                        <td title={row.url}>{row.url}</td>
                        <td>{row.occurrence}</td><td>{row.previousListPosition ?? "—"} to {row.currentListPosition ?? "—"}</td>
                        <td title={`${row.previousFinalUrl ?? "URL absent"} → ${row.currentFinalUrl ?? "URL absent"}`}>{row.previousFinalUrl ?? "URL absent"} to {row.currentFinalUrl ?? "URL absent"}</td>
                        <td title={reasons(row)}>{reasons(row)}</td>
                        <td>{row.contentComparison === "notApplicable" ? "Not applicable" : changeLabel(row.contentComparison)}</td>
                        <td>{row.previousStatusCode ?? "None"} to {row.currentStatusCode ?? "None"}</td>
                        <td>{row.previousTitle || "None"} to {row.currentTitle || "None"}</td>
                        <td>{row.previousMetaDescription || "None"} to {row.currentMetaDescription || "None"}</td>
                        <td>{row.previousIndexability || "None"} to {row.currentIndexability || "None"}</td>
                        <td>{hashLabel(row.previousResponseHash)} to {hashLabel(row.currentResponseHash)}</td>
                      </tr>;
                    })}
                    {last && virtualizer.getTotalSize() > last.end ? <tr aria-hidden="true"><td colSpan={12} style={{ height: virtualizer.getTotalSize() - last.end, padding: 0, border: 0 }} /></tr> : null}
                  </tbody>
                </table>
              </div>
            </div>
            <ComparisonRecordDetail detail={detail} loading={detailLoading} expanded={detailsExpanded} onExpand={() => setDetailsExpanded((value) => !value)} />
          </> : <p className="link-report-empty" role="status">{preparing ? "Preparing comparison…" : sessions ? "Compare these saved crawls." : "Compare the current crawl against a previously exported crawl archive."}</p>}
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

const detailLabels: Record<string, string> = { ...fieldLabels, url: "Captured requested URL", finalUrl: "Final destination", listPosition: "List position", listDuplicateIndex: "Stored duplicate index", h1: "H1", h2: "H2", metaRobots: "Meta robots", xRobotsTag: "X-Robots-Tag", structuredDataIssues: "Structured data issues", contentHash: "Content hash", contentHashContext: "Content capture context", error: "Crawl error" };
const missingGraphFields = new Set(["firstInlinkSourceUrl", "firstInlinkAnchorText", "firstInlinkSourcePosition"]);
const detailLabel = (key: string) => detailLabels[key] ?? key.replace(/([A-Z])/g, " $1").replace(/^./, (letter) => letter.toUpperCase());
const detailValue = (value: unknown) => value == null ? "Unavailable" : typeof value === "boolean" ? value ? "Yes" : "No" : typeof value === "object" ? JSON.stringify(value, null, 2) : String(value);
function ComparisonRecordDetail({ detail, loading, expanded, onExpand }: { detail?: ComparisonDetail; loading: boolean; expanded: boolean; onExpand: () => void }) {
  const [search, setSearch] = useState("");
  const [differencesOnly, setDifferencesOnly] = useState(false);
  const keys = detail ? [...new Set([...Object.keys(detail.previous ?? {}), ...Object.keys(detail.current ?? {})])] as (keyof CrawlRecord)[] : [];
  const ordered = [...new Set(["url", "finalUrl", "listPosition", "listDuplicateIndex", "statusCode", "title", "metaDescription", "h1", "h2", "canonical", "indexability", "indexabilityStatus", "metaRobots", "xRobotsTag", "contentHash", "responseHash", "error", "structuredDataIssues", ...keys])] as (keyof CrawlRecord)[];
  return <section className={`comparison-detail${detail || loading ? " has-selection" : ""}`} aria-label="Comparison URL details" aria-busy={loading}>
    {detail ? <>
      <div className="comparison-detail-header"><div className="comparison-detail-title"><h3 title={detail.row.url}>{detail.row.url}</h3><button onClick={onExpand} aria-expanded={expanded}>{expanded ? "Show changes" : "Expand details"}</button></div><p>{changeLabel(detail.row.change)} · {reasons(detail.row)} · Occurrence {detail.row.occurrence} · List position {detail.row.previousListPosition ?? "—"} to {detail.row.currentListPosition ?? "—"}</p>
        <div className="comparison-detail-controls"><label>Find field<input aria-label="Find comparison detail field" value={search} onChange={(event) => setSearch(event.target.value)} /></label><label><input type="checkbox" checked={differencesOnly} onChange={(event) => setDifferencesOnly(event.target.checked)} />Differing values only</label></div>
        <p>Open the crawl workbench for incoming-link source details.</p>
      </div>
      <div className="comparison-detail-scroll"><table className="comparison-detail-table"><thead><tr><th>Captured field / audit evidence</th><th>Previous{!detail.previous ? " · URL absent" : ""}</th><th>Current{!detail.current ? " · URL absent" : ""}</th></tr></thead><tbody>
        {ordered.filter((key) => !missingGraphFields.has(key) && keys.includes(key) && detailLabel(key).toLowerCase().includes(search.toLowerCase()) && (!differencesOnly || JSON.stringify(detail.previous?.[key]) !== JSON.stringify(detail.current?.[key]))).map((key) => {
          const different = JSON.stringify(detail.previous?.[key]) !== JSON.stringify(detail.current?.[key]);
          return <tr key={key} className={different ? "comparison-field-different" : undefined}><th>{detailLabel(key)}{different ? <span className="comparison-difference-marker" aria-label="Values differ">●</span> : null}</th><td><pre>{detail.previous ? detailValue(detail.previous[key]) : "URL absent"}</pre></td><td><pre>{detail.current ? detailValue(detail.current[key]) : "URL absent"}</pre></td></tr>;
        })}
      </tbody></table></div>
    </> : <p className="link-report-empty" role="status">{loading ? "Loading URL details…" : "Select a change to inspect previous and current fields and captured audit evidence. Use ↑ / ↓ to move between rows."}</p>}
  </section>;
}
