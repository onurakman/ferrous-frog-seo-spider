import { useCallback, useEffect, useId, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, ReactNode, Ref } from "react";
import * as Dialog from "./Dialog";
import { ArrowDownLeft, ArrowUpRight, ChevronRight, Download, Filter, Focus, ListTree, Maximize2, Minus, Network, Plus, RefreshCw, Search, X } from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type Sigma from "sigma";
import type { MultiDirectedGraph } from "graphology";
import type { EdgeProgramType } from "sigma/rendering";
import {
  brokenGraphTargets, buildGraphLayout, filterGraphSnapshot, graphBounds, graphNodeLabel,
  graphStatusLabel, isBrokenGraphEdge, isBrokenNode, isRedirectGraphEdge, matchingGraphUrls,
  readableUrl, sectionColor,
} from "./crawl-graph-model";
import type { CrawlGraph, GraphEdge, GraphLayout, GraphLayoutMode, GraphNode, GraphSection, GraphStatusFilter, Point } from "./crawl-graph-model";
import "./crawl-graph.css";

export type { CrawlGraph, GraphNode, GraphEdge, GraphLayoutMode, GraphStatusFilter } from "./crawl-graph-model";

export type GraphDialogProps = {
  open: boolean; onOpenChange: (open: boolean) => void; graph?: CrawlGraph;
  loading: boolean; theme: "light" | "dark"; live: boolean; updatedAt?: number;
  internalOnly: boolean; onInternalOnlyChange: (value: boolean) => void;
  statusFilter: GraphStatusFilter; onStatusFilterChange: (value: GraphStatusFilter) => void;
  depthFilter: string; onDepthFilterChange: (value: string) => void;
  layoutMode: GraphLayoutMode; onLayoutModeChange: (value: GraphLayoutMode) => void;
  onOpenBrokenLinks: () => void; onOpenRedirects: () => void; onRefresh: () => void;
  feedback?: ReactNode; error?: string;
};
type GraphCamera = { zoom: (factor: number) => void; fit: (urls?: string[]) => void; focus: (url: string) => void };
type Libraries = typeof import("./graph-renderer");
const EMPTY_GRAPH: CrawlGraph = { nodes: [], edges: [], totalNodes: 0, totalEdges: 0 };

export default function GraphDialog(props: GraphDialogProps) {
  return <Dialog.Root open={props.open} onOpenChange={props.onOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay className="modal-backdrop" />
      <Dialog.Content className="graph-modal crawl-graph-modal" inert={!props.open}>
        {/* Radix retains this child during exit; its renderers clean up on unmount. */}
        <GraphView {...props} />
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

function GraphView({ graph, loading, theme, live, updatedAt, internalOnly, onInternalOnlyChange,
  statusFilter, onStatusFilterChange, depthFilter, onDepthFilterChange, layoutMode,
  onLayoutModeChange, onOpenBrokenLinks, onOpenRedirects, onRefresh, feedback, error }: GraphDialogProps) {
  const [railOpen, setRailOpen] = useState(true);
  const [tab, setTab] = useState<"browse" | "filters">("browse");
  const [search, setSearch] = useState("");
  const [sectionId, setSectionId] = useState<string>();
  const [expandedSection, setExpandedSection] = useState<string>();
  const [selectedUrl, setSelectedUrl] = useState<string>();
  const [hoveredUrl, setHoveredUrl] = useState<string>();
  const [libraries, setLibraries] = useState<Libraries>();
  const [renderMode, setRenderMode] = useState<"webgl" | "svg">("webgl");
  const [renderError, setRenderError] = useState<string>();
  const [webglReady, setWebglReady] = useState(false);
  const svgCamera = useRef<GraphCamera>(null), webglCamera = useRef<GraphCamera>(null);
  const detailFocus = useRef<HTMLButtonElement>(null);
  const blockingError = graph?.nodes.length ? undefined : error;
  const camera = webglReady && renderMode === "webgl" ? webglCamera : svgCamera;
  const layoutCache = useRef(new Map<GraphLayoutMode, GraphLayout>());
  const layout = useMemo(() => {
    const result = buildGraphLayout(graph?.nodes ?? [], layoutMode, layoutCache.current.get(layoutMode));
    layoutCache.current.set(layoutMode, result);
    return result;
  }, [graph, layoutMode]);
  const visible = useMemo(() => filterGraphSnapshot(graph, statusFilter, depthFilter) ?? EMPTY_GRAPH, [graph, statusFilter, depthFilter]);
  const urls = useMemo(() => new Set(visible.nodes.map((node) => node.url)), [visible]);
  const nodesByUrl = useMemo(() => new Map(graph?.nodes.map((node) => [node.url, node])), [graph]);
  const matches = useMemo(() => matchingGraphUrls(visible.nodes, search), [visible, search]);
  const highlighted = useMemo(() => {
    if (!search.trim() && !sectionId) return null;
    return new Set([...matches].filter((url) => !sectionId || layout.sectionByUrl.get(url)?.id === sectionId));
  }, [matches, search, sectionId, layout]);
  const focusUrl = urls.has(hoveredUrl ?? selectedUrl ?? "") ? hoveredUrl ?? selectedUrl : undefined;
  const neighbors = useMemo(() => {
    const result = new Set<string>();
    if (!focusUrl) return result;
    result.add(focusUrl);
    for (const edge of visible.edges) {
      if (edge.sourceUrl === focusUrl) result.add(edge.targetUrl);
      if (edge.targetUrl === focusUrl) result.add(edge.sourceUrl);
    }
    return result;
  }, [focusUrl, visible]);
  const brokenTargets = useMemo(() => brokenGraphTargets(graph), [graph]);
  const brokenCount = graph?.edges.filter((edge) => isBrokenGraphEdge(edge, brokenTargets)).length ?? 0;
  const redirectCount = graph?.edges.filter(isRedirectGraphEdge).length ?? 0;
  const depths = [...new Set(graph?.nodes.flatMap((node) => typeof node.depth === "number" ? [node.depth] : []))].sort((a, b) => a - b);
  const selected = nodesByUrl.get(selectedUrl ?? "");
  const capped = Boolean(graph && (graph.totalNodes > graph.nodes.length || graph.totalEdges > graph.edges.length));
  const activeSections = layout.sections.filter((section) => section.urls.some((url) => urls.has(url)));
  const pickNode = (url: string) => {
    setSelectedUrl(url);
    setExpandedSection(layout.sectionByUrl.get(url)?.id);
    camera.current?.focus(url);
    if (window.matchMedia("(max-width: 760px)").matches) {
      setRailOpen(false);
      requestAnimationFrame(() => detailFocus.current?.focus());
    }
  };
  const resetFilters = () => { onStatusFilterChange("all"); onDepthFilterChange("all"); setSearch(""); setSectionId(undefined); };
  const failWebgl = useCallback((message: string) => { setRenderError(message); setWebglReady(false); setRenderMode("svg"); }, []);

  useEffect(() => {
    let cancelled = false;
    void import("./graph-renderer").then((result) => { if (!cancelled) setLibraries(result); })
      .catch(() => { if (!cancelled) failWebgl("WebGL could not load. The SVG canvas remains available."); });
    return () => { cancelled = true; };
  }, [failWebgl]);

  const exportVisible = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(visible, null, 2)], { type: "application/json;charset=utf-8" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = "ferrous-frog-graph-filtered.json";
    anchor.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const drawProps: DrawingProps = { graph: visible, layout, theme, highlighted, focusUrl, neighbors,
    brokenTargets, onSelect: pickNode, onHover: setHoveredUrl, onClear: () => setSelectedUrl(undefined) };
  const showWebgl = renderMode === "webgl" && Boolean(libraries);

  return <>
    <header className="cg-header">
      <div className="cg-heading"><span className="cg-emblem"><Network size={19} /></span><div>
        <Dialog.Title>Crawl Graph</Dialog.Title>
        <Dialog.Description>Explore URL sections and the links between them.</Dialog.Description>
      </div></div>
      <div className="graph-actions">
        <span className={`cg-live ${live ? "on" : ""}`}><i />{live ? "Live" : "Snapshot"}</span>
        <button onClick={exportVisible} disabled={!visible.nodes.length} title="Export the status and depth filtered snapshot as JSON"><Download size={14} />Export JSON</button>
        <button onClick={onRefresh} disabled={loading} title="Reload graph"><RefreshCw size={14} className={loading ? "cg-reloading" : ""} /><span>Reload</span></button>
        <Dialog.Close asChild><button className="cg-close" title="Close graph" aria-label="Close graph"><X size={18} /></button></Dialog.Close>
      </div>
    </header>
    {feedback || (error && !blockingError) ? <div className="cg-feedback">{feedback}
      {error && !blockingError ? <div className="error-bar" role="alert"><span>{error}</span><button onClick={onRefresh} disabled={loading}>Try again</button></div> : null}
    </div> : null}
    <div className={`cg-workspace ${railOpen ? "rail-open" : ""}`}>
      <aside className="cg-rail" aria-label="Graph navigation and filters" inert={!railOpen}>
        <div className="cg-rail-tabs" role="tablist" aria-label="Graph tools" onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault(); const next = event.key === "Home" ? "browse" : event.key === "End" ? "filters" : tab === "browse" ? "filters" : "browse";
          setTab(next); event.currentTarget.querySelector<HTMLButtonElement>(`[data-tab="${next}"]`)?.focus();
        }}>
          <button role="tab" data-tab="browse" id="cg-browse-tab" aria-controls="cg-browse" aria-selected={tab === "browse"} tabIndex={tab === "browse" ? 0 : -1} onClick={() => setTab("browse")}><ListTree size={14} />Browse</button>
          <button role="tab" data-tab="filters" id="cg-filters-tab" aria-controls="cg-filters" aria-selected={tab === "filters"} tabIndex={tab === "filters" ? 0 : -1} onClick={() => setTab("filters")}><Filter size={14} />Filters{statusFilter !== "all" || depthFilter !== "all" ? <i /> : null}</button>
        </div>
        <div id="cg-browse" role="tabpanel" aria-labelledby="cg-browse-tab" className="cg-browse" hidden={tab !== "browse"}>
          <GraphNavigator nodes={visible.nodes} layout={layout} search={search} onSearch={(value) => { setSearch(value); setSelectedUrl(undefined); setHoveredUrl(undefined); }} matches={matches}
            sectionId={sectionId} onSection={setSectionId} expanded={expandedSection} onExpand={setExpandedSection}
            selectedUrl={selectedUrl} onSelect={pickNode} onFrameMatches={() => camera.current?.fit([...matches])} theme={theme} />
        </div>
        <div id="cg-filters" role="tabpanel" aria-labelledby="cg-filters-tab" className="cg-filter-panel" hidden={tab !== "filters"}>
          <div className="cg-filter-heading"><strong>Snapshot filters</strong><button onClick={resetFilters}>Reset</button></div>
          <label className="cg-checkbox"><input type="checkbox" checked={internalOnly} onChange={(event) => onInternalOnlyChange(event.target.checked)} />Internal Only</label>
          <label>Status<select value={statusFilter} onChange={(event) => onStatusFilterChange(event.target.value as GraphStatusFilter)}>
            <option value="all">All statuses</option><option value="success">2xx · Success</option><option value="redirect">3xx · Redirect</option><option value="broken">Broken URLs</option>
            <option value="brokenLinks">Broken Links</option><option value="redirectLinks">Redirect Links</option><option value="external">External</option><option value="uncrawled">Uncrawled</option>
          </select></label>
          <label>Depth<select value={depthFilter} onChange={(event) => onDepthFilterChange(event.target.value)}><option value="all">All depths</option>{depths.map((depth) => <option key={depth} value={String(depth)}>{depth}</option>)}</select></label>
          <label>Layout<select value={layoutMode} onChange={(event) => onLayoutModeChange(event.target.value as GraphLayoutMode)}><option value="clusters">URL sections</option><option value="depth">Depth</option><option value="radial">Radial</option></select></label>
          <p>Colors group URLs by host and first path segment. Search highlights matches within this snapshot.</p>
          <div className="cg-filter-heading"><strong>Source-to-target links</strong></div>
          <button className={`cg-audit-filter ${statusFilter === "brokenLinks" ? "active" : ""}`} disabled={!brokenCount}
            title="Show broken source-to-target edges with both endpoint nodes" onClick={() => { onStatusFilterChange("brokenLinks"); onDepthFilterChange("all"); }}>
            <i className="broken" />Broken Links ({brokenCount.toLocaleString()})</button>
          <button className={`cg-audit-filter ${statusFilter === "redirectLinks" ? "active" : ""}`} disabled={!redirectCount}
            title="Show redirect source-to-target edges with both endpoint nodes" onClick={() => { onStatusFilterChange("redirectLinks"); onDepthFilterChange("all"); }}>
            <i className="redirect" />Redirect Links ({redirectCount.toLocaleString()})</button>
          <p>Link filters include both endpoints. Counts refer to the source snapshot.</p>
          <button className="cg-report-button" onClick={onOpenBrokenLinks} disabled={!brokenCount} title="Open source-to-target rows for broken links">Broken Report<ArrowUpRight size={14} /></button>
          <button className="cg-report-button" onClick={onOpenRedirects} disabled={!redirectCount} title="Open redirect-chain rows">Redirect Report<ArrowUpRight size={14} /></button>
        </div>
        <footer className="cg-rail-footer"><span>{activeSections.length} URL sections</span><span>{renderMode === "webgl" && webglReady ? "WebGL" : "SVG"} canvas</span></footer>
      </aside>
      <main className="cg-stage graph-canvas-shell" aria-label="Crawl graph canvas">
        <div className="cg-stage-tools">
          <button className="cg-rail-toggle" onClick={() => setRailOpen(!railOpen)} aria-expanded={railOpen} title={railOpen ? "Hide graph navigation and filters" : "Show graph navigation and filters"}><ListTree size={16} /><span>{railOpen ? "Hide navigation" : "Browse & filters"}</span></button>
          {statusFilter !== "all" || depthFilter !== "all" ? <button className="cg-filter-chip" onClick={() => { setRailOpen(true); setTab("filters"); }}><Filter size={12} />{statusFilter === "all" ? "All statuses" : statusFilter.replace(/([A-Z])/g, " $1")}{depthFilter !== "all" ? ` · Depth ${depthFilter}` : ""}</button> : null}
        </div>
        {!showWebgl || !webglReady ? <SvgGraph {...drawProps} ref={svgCamera} /> : null}
        {showWebgl ? <WebglGraph {...drawProps} libraries={libraries!} ref={webglCamera} ready={webglReady} onReady={setWebglReady} onError={failWebgl} /> : null}
        {blockingError || !visible.nodes.length ? <div className="cg-empty" role={blockingError ? "alert" : "status"}>
          <Network size={34} /><h3>{blockingError ? "Graph could not load" : loading ? "Loading graph…" : graph?.nodes.length ? "No URLs match these filters" : "No graph yet"}</h3>
          <p>{blockingError ?? (graph?.nodes.length ? "Change the status or depth filter to explore more URLs." : "Crawl a site to map its pages and the links connecting them.")}</p>
          {blockingError ? <button onClick={onRefresh} disabled={loading}>Try again</button> : graph?.nodes.length ? <button onClick={resetFilters}>Reset filters</button> : null}
        </div> : highlighted?.size === 0 ? <div className="cg-no-matches" role="status">No URLs match. <button onClick={() => { setSearch(""); setSectionId(undefined); }}>Clear search and section</button></div> : null}
        {selected ? <NodeDetails node={selected} visible={urls.has(selected.url)} edges={visible.edges} nodesByUrl={nodesByUrl} onSelect={pickNode}
          onClear={() => setSelectedUrl(undefined)} onFocus={() => camera.current?.focus(selected.url)} onReset={resetFilters} theme={theme} section={layout.sectionByUrl.get(selected.url)} focusButtonRef={detailFocus} /> : null}
        <div className="cg-camera" aria-label="Graph camera controls">
          <button onClick={() => camera.current?.zoom(0.75)} disabled={!visible.nodes.length} title="Zoom in" aria-label="Zoom in"><Plus size={17} /></button>
          <button onClick={() => camera.current?.zoom(1 / 0.75)} disabled={!visible.nodes.length} title="Zoom out" aria-label="Zoom out"><Minus size={17} /></button>
          <button onClick={() => camera.current?.fit()} disabled={!visible.nodes.length} title="Fit graph to view" aria-label="Fit graph to view"><Maximize2 size={16} /></button>
        </div>
        <div className="cg-canvas-status"><span>{loading ? "Refreshing snapshot…" : highlighted ? `${highlighted.size.toLocaleString()} matching URLs` : "Drag to pan · Scroll to zoom"}</span>
          <div className="graph-actions"><button onClick={() => { setRenderError(undefined); setWebglReady(false); setRenderMode(renderMode === "webgl" ? "svg" : "webgl"); }}>{renderMode === "webgl" ? "Use SVG" : "Use WebGL"}</button></div>
        </div>
        {renderError ? <p className="cg-render-message" role="status">{renderError}</p> : null}
      </main>
    </div>
    <footer className="cg-footer"><span><strong>{visible.nodes.length.toLocaleString()}</strong> URLs · <strong>{visible.edges.length.toLocaleString()}</strong> links</span>
      {capped ? <span className="cg-cap">Capped snapshot: {graph!.nodes.length.toLocaleString()} of {graph!.totalNodes.toLocaleString()} URLs · {graph!.edges.length.toLocaleString()} of {graph!.totalEdges.toLocaleString()} links</span> : <span>{activeSections.length} URL sections</span>}
      <span>{updatedAt ? `Updated ${new Date(updatedAt).toLocaleTimeString()}` : live ? "Waiting for crawl data" : "Select a URL to inspect connections"}</span>
    </footer>
  </>;
}

type NavigatorProps = {
  nodes: GraphNode[]; layout: GraphLayout; search: string; onSearch: (value: string) => void; matches: Set<string>;
  sectionId?: string; onSection: (value?: string) => void; expanded?: string; onExpand: (value?: string) => void;
  selectedUrl?: string; onSelect: (url: string) => void; onFrameMatches: () => void; theme: "light" | "dark";
};
function GraphNavigator({ nodes, layout, search, onSearch, matches, sectionId, onSection, expanded, onExpand, selectedUrl, onSelect, onFrameMatches, theme }: NavigatorProps) {
  const listRef = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<string | null>(null);
  const [focusedRow, setFocusedRow] = useState(0);
  const nodeMap = useMemo(() => new Map(nodes.map((node) => [node.url, node])), [nodes]);
  const rows = useMemo(() => layout.sections.flatMap((section) => {
    const members = section.urls.filter((url) => nodeMap.has(url) && matches.has(url));
    if (!members.length || (sectionId && sectionId !== section.id)) return [];
    const header = { key: `section:${section.id}`, section, node: undefined as GraphNode | undefined };
    return [header, ...(search.trim() || expanded === section.id ? members.map((url) => ({ key: url, section, node: nodeMap.get(url) })) : [])];
  }), [layout, nodeMap, matches, search, expanded, sectionId]);
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => listRef.current, estimateSize: () => 38, overscan: 6, getItemKey: (index) => rows[index].key });
  useEffect(() => {
    if (!selectedUrl || search.trim()) return;
    const index = rows.findIndex((row) => row.node?.url === selectedUrl);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "auto" });
  }, [selectedUrl, expanded, rows, search, virtualizer]);
  const focusRow = (index: number) => {
    const target = Math.max(0, Math.min(index, rows.length - 1)); setFocusedRow(target);
    pendingFocus.current = rows[target]?.key ?? null;
    virtualizer.scrollToIndex(target, { align: "auto" });
    const button = listRef.current?.querySelector<HTMLButtonElement>(`[data-graph-row="${target}"]`);
    if (button) { pendingFocus.current = null; button.focus(); }
  };
  const changeSearch = (value: string) => { pendingFocus.current = null; onSearch(value); setFocusedRow(0); };
  const handleKeys = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const shifts: Record<string, number> = { ArrowDown: index + 1, ArrowUp: index - 1, Home: 0, End: rows.length - 1 };
    if (event.key in shifts) { event.preventDefault(); focusRow(shifts[event.key]); }
    if (event.key === "ArrowRight" && !rows[index].node) {
      event.preventDefault();
      if (search.trim() || expanded === rows[index].section.id) focusRow(index + 1);
      else onExpand(rows[index].section.id);
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault(); onExpand(undefined);
      focusRow(rows.findIndex((row) => !row.node && row.section.id === rows[index].section.id));
    }
  };
  return <>
    <div className="cg-search"><Search size={15} /><input aria-label="Search graph URLs" placeholder="Search URLs…" value={search} onChange={(event) => changeSearch(event.target.value)} onKeyDown={(event) => {
      if (event.key === "ArrowDown" && rows.length) { event.preventDefault(); focusRow(0); }
      if (event.key === "Enter") { event.preventDefault(); onFrameMatches(); }
    }} />{search ? <button title="Clear graph search" aria-label="Clear graph search" onClick={() => changeSearch("")}><X size={13} /></button> : <kbd>↵</kbd>}</div>
    <div className="cg-section-picker"><label htmlFor="cg-section">URL section</label><select id="cg-section" value={sectionId ?? ""} onChange={(event) => { onSection(event.target.value || undefined); onExpand(event.target.value || undefined); }}>
      <option value="">All sections</option>{layout.sections.filter((section) => section.urls.some((url) => nodeMap.has(url))).map((section) => <option key={section.id} value={section.id}>{section.host}{section.label === "/" ? "/" : section.label}</option>)}
    </select></div>
    <div className="cg-navigation-heading"><strong>{search.trim() ? `${matches.size} matching URLs` : "Site sections"}</strong><span>↑ ↓ to browse</span></div>
    <div ref={listRef} className="cg-navigation" aria-label="Graph URL navigation">
      {rows.length ? <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>{virtualizer.getVirtualItems().map((item) => {
        const row = rows[item.index], node = row.node;
        return <button key={row.key} ref={(button) => {
          // A hidden tab can need another commit before its virtual rows mount.
          if (button && pendingFocus.current === row.key) { pendingFocus.current = null; button.focus(); }
        }} data-graph-row={item.index} className={`cg-navigation-row ${node ? "is-url" : "is-section"} ${node && node.url === selectedUrl ? "is-selected" : ""}`}
          style={{ transform: `translateY(${item.start}px)`, "--section-color": sectionColor(row.section, theme) } as CSSProperties}
          tabIndex={item.index === Math.min(focusedRow, rows.length - 1) ? 0 : -1} onFocus={() => setFocusedRow(item.index)} onKeyDown={(event) => handleKeys(event, item.index)}
          aria-pressed={node ? node.url === selectedUrl : undefined} aria-expanded={node ? undefined : Boolean(search.trim() || expanded === row.section.id)}
          title={node?.url ?? `${row.section.host}${row.section.label}`} onClick={() => node ? onSelect(node.url) : onExpand(expanded === row.section.id ? undefined : row.section.id)}>
          {node ? <i className={`cg-node-dot ${isBrokenNode(node) ? "is-broken" : ""}`} /> : <ChevronRight size={13} className={search.trim() || expanded === row.section.id ? "expanded" : ""} />}
          <span>{node ? graphNodeLabel(node) : <><b>{row.section.label}</b><small>{row.section.host}</small></>}</span>
          <em>{node ? node.statusCode ?? (node.crawled ? "—" : "…") : row.section.urls.filter((url) => nodeMap.has(url) && matches.has(url)).length}</em>
        </button>;
      })}</div> : <p className="cg-navigation-empty">{search ? "No URLs match this search." : "No URLs in this snapshot."}</p>}
    </div>
  </>;
}

function NodeDetails({ node, visible, edges, nodesByUrl, onSelect, onClear, onFocus, onReset, section, theme, focusButtonRef }: {
  node: GraphNode; visible: boolean; edges: GraphEdge[]; nodesByUrl: Map<string, GraphNode>;
  onSelect: (url: string) => void; onClear: () => void; onFocus: () => void; onReset: () => void;
  section?: GraphSection; theme: "light" | "dark";
  focusButtonRef: Ref<HTMLButtonElement>;
}) {
  const [direction, setDirection] = useState<"out" | "in">("out");
  const [limit, setLimit] = useState(12);
  useEffect(() => setLimit(12), [node.url, direction]);
  const related = new Map<string, number>();
  for (const edge of edges) {
    const url = direction === "out" ? edge.sourceUrl === node.url ? edge.targetUrl : undefined : edge.targetUrl === node.url ? edge.sourceUrl : undefined;
    if (url && nodesByUrl.has(url)) related.set(url, (related.get(url) ?? 0) + 1);
  }
  return <section className="cg-detail" aria-label="Selected graph URL">
    <div className="cg-detail-heading"><span style={{ color: sectionColor(section, theme) }}>{section?.label ?? "Selected URL"}</span><button onClick={onClear} title="Clear selected URL" aria-label="Clear selected URL"><X size={14} /></button></div>
    <strong className="cg-selected-url">{readableUrl(node.url)}</strong>
    <span className={`cg-status ${isBrokenNode(node) ? "broken" : node.statusCode && node.statusCode >= 300 && node.statusCode < 400 ? "redirect" : ""}`}>{graphStatusLabel(node)}{node.classification === "external" ? " · External" : ""}</span>
    <dl><div><dt>Depth</dt><dd>{node.depth ?? "—"}</dd></div><div><dt>Indexability</dt><dd>{node.indexability || "Unknown"}</dd></div><div><dt>Inlinks</dt><dd>{node.inlinkCount.toLocaleString()}</dd></div><div><dt>Outlinks</dt><dd>{node.outlinkCount.toLocaleString()}</dd></div></dl>
    {visible ? <button ref={focusButtonRef} className="cg-focus-button" onClick={onFocus}><Focus size={13} />Focus URL</button> : <button ref={focusButtonRef} className="cg-focus-button" onClick={onReset}>Hidden by filters · Reset filters</button>}
    <div className="cg-neighbor-tabs"><button aria-pressed={direction === "out"} onClick={() => setDirection("out")}><ArrowUpRight size={13} />Outgoing</button><button aria-pressed={direction === "in"} onClick={() => setDirection("in")}><ArrowDownLeft size={13} />Incoming</button></div>
    <div className="cg-related" aria-label={`${direction === "out" ? "Outgoing" : "Incoming"} URLs in the snapshot`}>
      {[...related].slice(0, limit).map(([url, count]) => <button key={url} title={url} onClick={() => onSelect(url)}><span>{graphNodeLabel(nodesByUrl.get(url)!)}</span><small>{count > 1 ? `${count} links` : nodesByUrl.get(url)!.statusCode ?? "—"}</small></button>)}
      {!related.size ? <p>No {direction === "out" ? "outgoing" : "incoming"} links in this view.</p> : null}
      {related.size > limit ? <button onClick={() => setLimit(limit + 12)}>Show more ({related.size - limit})</button> : null}
    </div>
    <small className="cg-detail-note">Connections shown within this snapshot.</small>
  </section>;
}

type DrawingProps = {
  graph: CrawlGraph; layout: GraphLayout; theme: "light" | "dark"; highlighted: Set<string> | null;
  focusUrl?: string; neighbors: Set<string>; brokenTargets: Set<string>;
  onSelect: (url: string) => void; onHover: (url?: string) => void; onClear: () => void;
};
function nodeOpacity(url: string, props: DrawingProps) {
  if (props.focusUrl) return props.neighbors.has(url) ? 1 : 0.12;
  if (props.highlighted && !props.highlighted.has(url)) return 0.13;
  return 1;
}
function nodeSize(node: GraphNode) { return Math.min(8, 3.2 + Math.log2(1 + node.inlinkCount) * 0.6); }
function edgeAppearance(edge: GraphEdge, props: DrawingProps) {
  const focused = edge.sourceUrl === props.focusUrl || edge.targetUrl === props.focusUrl;
  const color = isBrokenGraphEdge(edge, props.brokenTargets) ? props.theme === "dark" ? "#e4979f" : "#b35c69"
    : isRedirectGraphEdge(edge) ? props.theme === "dark" ? "#d2b581" : "#a28b59" : props.theme === "dark" ? "#81929d" : "#7f96a2";
  const dim = props.focusUrl ? !focused : props.highlighted && (!props.highlighted.has(edge.sourceUrl) || !props.highlighted.has(edge.targetUrl));
  return { color, opacity: dim ? 0.035 : focused ? 0.85 : props.theme === "light" ? 0.26 : 0.2, width: focused ? 1.4 : 0.7, focused };
}
function reducedMotion() { return window.matchMedia("(prefers-reduced-motion: reduce)").matches; }

type ViewBox = ReturnType<typeof graphBounds>;
function scaleBox(box: ViewBox, factor: number, point?: Point): ViewBox {
  const nextWidth = Math.max(35, Math.min(30000, box.width * factor));
  factor = nextWidth / box.width;
  const anchor = point ?? { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  return { x: anchor.x - (anchor.x - box.x) * factor, y: anchor.y - (anchor.y - box.y) * factor, width: nextWidth, height: box.height * factor };
}

function SvgGraph(props: DrawingProps & { ref: Ref<GraphCamera> }) {
  const { graph, layout, theme, ref } = props;
  const initial = graphBounds(graph.nodes.flatMap((node) => layout.points.get(node.url) ?? []));
  const [box, setBox] = useState(initial);
  const [dimensions, setDimensions] = useState({ width: 1, height: 1 });
  const boxRef = useRef(box), svgRef = useRef<SVGSVGElement>(null), frameRef = useRef(0);
  const dragRef = useRef<{ x: number; y: number; box: ViewBox; scale: number; moved: boolean }>(null);
  const previousLayout = useRef(layout.mode);
  const initialized = useRef(graph.nodes.length > 0);
  const markerId = useId();
  const displayScale = Math.max(0.01, Math.min(dimensions.width / box.width, dimensions.height / box.height));
  const updateBox = useCallback((value: ViewBox) => { boxRef.current = value; setBox(value); }, []);
  const animate = useCallback((target: ViewBox) => {
    cancelAnimationFrame(frameRef.current);
    const start = boxRef.current, startedAt = performance.now();
    const tick = (now: number) => {
      const progress = reducedMotion() ? 1 : Math.min(1, (now - startedAt) / 220), eased = 1 - Math.pow(1 - progress, 3);
      updateBox({ x: start.x + (target.x - start.x) * eased, y: start.y + (target.y - start.y) * eased,
        width: start.width + (target.width - start.width) * eased, height: start.height + (target.height - start.height) * eased });
      if (progress < 1) frameRef.current = requestAnimationFrame(tick);
    };
    if (reducedMotion()) updateBox(target); else frameRef.current = requestAnimationFrame(tick);
  }, [updateBox]);
  useImperativeHandle(ref, () => ({
    zoom: (factor) => animate(scaleBox(boxRef.current, factor)),
    fit: (urls) => animate(graphBounds((urls ?? graph.nodes.map((node) => node.url)).flatMap((url) => layout.points.get(url) ?? []))),
    focus: (url) => {
      const point = layout.points.get(url);
      if (point) {
        const width = Math.min(boxRef.current.width, 320), height = width * dimensions.height / dimensions.width;
        const verticalAnchor = window.matchMedia("(max-width: 760px)").matches ? 0.27 : 0.5;
        animate({ x: point.x - width / 2, y: point.y - height * verticalAnchor, width, height });
      }
    },
  }), [animate, graph, layout, dimensions]);
  useEffect(() => {
    if (previousLayout.current !== layout.mode || (!initialized.current && graph.nodes.length)) {
      initialized.current = graph.nodes.length > 0;
      previousLayout.current = layout.mode; animate(graphBounds(graph.nodes.flatMap((node) => layout.points.get(node.url) ?? [])));
    }
  }, [layout.mode, animate, graph, layout]);
  useLayoutEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const measure = () => setDimensions({ width: svg.clientWidth, height: svg.clientHeight });
    const observer = new ResizeObserver(measure); observer.observe(svg); measure();
    const wheel = (event: WheelEvent) => {
      event.preventDefault(); cancelAnimationFrame(frameRef.current);
      const matrix = svg.getScreenCTM();
      if (!matrix) return;
      const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
      updateBox(scaleBox(boxRef.current, Math.exp(Math.max(-250, Math.min(250, event.deltaY)) * 0.002), point));
    };
    svg.addEventListener("wheel", wheel, { passive: false });
    return () => { observer.disconnect(); svg.removeEventListener("wheel", wheel); cancelAnimationFrame(frameRef.current); };
  }, [updateBox]);
  const picture = useMemo(() => {
    const displayedUrls = new Set(graph.nodes.map((node) => node.url));
    return <>
    <defs><marker id={markerId} viewBox="0 0 10 10" refX="13" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" /></marker></defs>
    {layout.mode === "clusters" ? <g className="cg-section-labels">{layout.sections.filter((section) => section.urls.some((url) => displayedUrls.has(url))).slice(0, 24).map((section) => <text key={section.id} x={section.x} y={section.y - section.radius - 30} textAnchor="middle" fill={sectionColor(section, theme)} style={{ fontSize: 11 / displayScale, letterSpacing: 0.5 / displayScale }}>{section.label === "/" ? section.host : section.label}</text>)}</g> : null}
    <g className="graph-svg-edges">{graph.edges.map((edge, index) => {
      const source = layout.points.get(edge.sourceUrl), target = layout.points.get(edge.targetUrl), style = edgeAppearance(edge, props);
      return source && target ? <line key={`${edge.id}-${index}`} x1={source.x} y1={source.y} x2={target.x} y2={target.y} stroke={style.color} strokeOpacity={style.opacity} strokeWidth={style.width} vectorEffect="non-scaling-stroke" markerEnd={style.focused ? `url(#${markerId})` : undefined} /> : null;
    })}</g>
    <g className="graph-svg-nodes">{graph.nodes.map((node) => {
      const point = layout.points.get(node.url); if (!point) return null;
      const color = sectionColor(layout.sectionByUrl.get(node.url), theme), selected = node.url === props.focusUrl;
      const stroke = selected ? theme === "dark" ? "#f1f4f8" : "#24333d" : isBrokenNode(node) ? theme === "dark" ? "#e4979f" : "#b35c69" : color;
      const radius = (nodeSize(node) + (selected ? 2 : 0)) / displayScale;
      const labeled = selected || (props.focusUrl ? props.neighbors.has(node.url) : graph.nodes.length < 18 || node.depth === 0);
      return <g key={node.url} className="graph-svg-node" data-graph-url={node.url} opacity={nodeOpacity(node.url, props)} onClick={(event) => { event.stopPropagation(); props.onSelect(node.url); }} onPointerEnter={() => props.onHover(node.url)} onPointerLeave={() => props.onHover(undefined)}>
        <circle cx={point.x} cy={point.y} r={radius} fill={node.crawled ? color : theme === "dark" ? "#1b1e22" : "#f7f9fb"} stroke={stroke} strokeWidth={selected || isBrokenNode(node) ? 2 : 1} vectorEffect="non-scaling-stroke"><title>{node.url} · {graphStatusLabel(node)}</title></circle>
        {labeled ? <text x={point.x + radius + 7 / displayScale} y={point.y + 4 / displayScale} className="cg-node-label" style={{ fontSize: 10 / displayScale, strokeWidth: 4 / displayScale }}>{graphNodeLabel(node).slice(0, 46)}</text> : null}
      </g>;
    })}</g>
  </>; }, [graph, layout, theme, props.highlighted, props.focusUrl, props.neighbors, props.onSelect, props.onHover, markerId, displayScale]);
  return <svg ref={svgRef} className="graph-svg cg-svg" viewBox={`${box.x} ${box.y} ${box.width} ${box.height}`} tabIndex={0} role="img" aria-label="Interactive crawl graph. Drag to pan, scroll to zoom, or use Browse to select URLs with the keyboard."
    onPointerDown={(event) => {
      if (event.button !== 0 || (event.target as Element).closest("[data-graph-url]")) return;
      const matrix = event.currentTarget.getScreenCTM(); if (!matrix) return;
      cancelAnimationFrame(frameRef.current); event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = { x: event.clientX, y: event.clientY, box: boxRef.current, scale: matrix.inverse().a, moved: false };
    }} onPointerMove={(event) => {
      const drag = dragRef.current; if (!drag) return;
      const dx = event.clientX - drag.x, dy = event.clientY - drag.y; drag.moved ||= Math.abs(dx) + Math.abs(dy) > 4;
      updateBox({ ...drag.box, x: drag.box.x - dx * drag.scale, y: drag.box.y - dy * drag.scale });
    }} onPointerUp={() => { if (dragRef.current && !dragRef.current.moved) props.onClear(); dragRef.current = null; }} onPointerCancel={() => { dragRef.current = null; }}
    onKeyDown={(event) => {
      if (["+", "=", "-", "0", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) event.preventDefault();
      if (event.key === "+" || event.key === "=") animate(scaleBox(boxRef.current, 0.75));
      else if (event.key === "-") animate(scaleBox(boxRef.current, 1 / 0.75));
      else if (event.key === "0") animate(graphBounds(graph.nodes.flatMap((node) => layout.points.get(node.url) ?? [])));
      else if (event.key.startsWith("Arrow")) { const current = boxRef.current; animate({ ...current, x: current.x + (event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0) * current.width * 0.1, y: current.y + (event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0) * current.height * 0.1 }); }
    }}>{picture}</svg>;
}

type NodeAttributes = { x: number; y: number; label: string; size: number; color: string; forceLabel: boolean };
type EdgeAttributes = { color: string; size: number; type: string; edge: GraphEdge };
function colorOnCanvas(color: string, opacity: number, theme: "light" | "dark") {
  // Sigma expects premultiplied colors. Opaque blending avoids washed-out RGBA
  // strokes and uses the same surface as .cg-stage in crawl-graph.css.
  const surface = theme === "dark" ? [25, 29, 35] : [247, 249, 251];
  return `rgb(${surface.map((channel, index) => Math.round(channel + (parseInt(color.slice(1 + index * 2, 3 + index * 2), 16) - channel) * opacity)).join(",")})`;
}

function WebglGraph(props: DrawingProps & {
  ref: Ref<GraphCamera>; libraries: Libraries; ready: boolean; onReady: (ready: boolean) => void; onError: (message: string) => void;
}) {
  const container = useRef<HTMLDivElement>(null), labels = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<Sigma<NodeAttributes, EdgeAttributes>>(null);
  const modelRef = useRef<MultiDirectedGraph<NodeAttributes, EdgeAttributes>>(null);
  const latest = useRef(props); latest.current = props;
  const previousLayout = useRef(props.layout.mode);
  const moveCamera = (state: { x?: number; y?: number; ratio?: number }) => {
    const camera = rendererRef.current?.getCamera();
    if (!camera) return;
    if (reducedMotion()) {
      // animate also cancels Sigma's pending camera frame; setState alone does not.
      if (camera.isAnimated()) void camera.animate(state, { duration: 1 });
      else camera.setState(state);
    } else void camera.animate(state, { duration: 220 });
  };
  const fit = (urls?: string[]) => {
    const renderer = rendererRef.current;
    if (!renderer) return;
    const worldBounds = graphBounds([...latest.current.layout.points.values()]);
    renderer.setCustomBBox({ x: [worldBounds.x, worldBounds.x + worldBounds.width], y: [-worldBounds.y - worldBounds.height, -worldBounds.y] });
    renderer.refresh();
    const points = (urls ?? latest.current.graph.nodes.map((node) => node.url)).flatMap((url) => renderer.getNodeDisplayData(url) ?? []);
    if (!points.length) return;
    const dimensions = renderer.getDimensions();
    // Display coordinates are normalized; do not apply the world-space minimum size.
    const minX = Math.min(...points.map((point) => point.x)), maxX = Math.max(...points.map((point) => point.x));
    const minY = Math.min(...points.map((point) => point.y)), maxY = Math.max(...points.map((point) => point.y));
    const side = Math.min(dimensions.width, dimensions.height);
    const ratio = Math.max((maxX - minX) * side / dimensions.width, (maxY - minY) * side / dimensions.height, 0.12) * 1.35;
    moveCamera({ x: minX + (maxX - minX) / 2, y: minY + (maxY - minY) / 2, ratio });
  };
  useImperativeHandle(props.ref, () => ({
    zoom: (factor) => { const camera = rendererRef.current?.getCamera(); if (camera) moveCamera({ ratio: camera.ratio * factor }); },
    fit,
    focus: (url) => {
      const renderer = rendererRef.current, point = renderer?.getNodeDisplayData(url);
      if (renderer && point) {
        const state = { ...renderer.getCamera().getState(), x: point.x, y: point.y, ratio: Math.min(renderer.getCamera().ratio, 0.35) };
        if (window.matchMedia("(max-width: 760px)").matches) {
          const dimensions = renderer.getDimensions();
          const anchor = renderer.viewportToFramedGraph({ x: dimensions.width / 2, y: dimensions.height * 0.27 }, { cameraState: state });
          state.x += point.x - anchor.x; state.y += point.y - anchor.y;
        }
        moveCamera(state);
      }
    },
  }));

  useEffect(() => {
    if (!container.current) return;
    const model = new props.libraries.MultiDirectedGraph<NodeAttributes, EdgeAttributes>();
    let renderer: Sigma<NodeAttributes, EdgeAttributes>;
    try {
      renderer = new props.libraries.Sigma<NodeAttributes, EdgeAttributes>(model, container.current, {
        allowInvalidContainer: true, defaultEdgeType: "arrow", edgeProgramClasses: { arrow: props.libraries.EdgeArrowProgram as unknown as EdgeProgramType<NodeAttributes, EdgeAttributes> },
        labelFont: 'Inter, ui-sans-serif, system-ui, sans-serif', labelSize: 11, labelDensity: 0.08,
        labelRenderedSizeThreshold: 8, renderEdgeLabels: false, minEdgeThickness: 0.7, antiAliasingFeather: 0.4,
        stagePadding: 75, zIndex: true, minCameraRatio: 0.025, maxCameraRatio: 8, enableCameraRotation: false,
        nodeReducer: (url, data) => {
          const current = latest.current, selected = current.focusUrl === url, opacity = nodeOpacity(url, current);
          return { ...data, color: colorOnCanvas(sectionColor(current.layout.sectionByUrl.get(url), current.theme), opacity, current.theme),
            size: data.size + (selected ? 2 : 0), zIndex: selected ? 2 : opacity === 1 ? 1 : 0,
            label: opacity < 1 ? "" : data.label, forceLabel: selected || (Boolean(current.focusUrl) && current.neighbors.has(url)) || data.forceLabel };
        },
        edgeReducer: (_key, data) => {
          const appearance = edgeAppearance(data.edge, latest.current);
          return { color: colorOnCanvas(appearance.color, appearance.opacity, latest.current.theme), size: appearance.width, zIndex: appearance.focused ? 1 : 0 };
        },
      });
    } catch {
      latest.current.onError("WebGL is unavailable in this window. The SVG canvas remains available.");
      return;
    }
    rendererRef.current = renderer; modelRef.current = model;
    renderer.on("clickNode", ({ node }) => latest.current.onSelect(node));
    renderer.on("enterNode", ({ node }) => latest.current.onHover(node));
    renderer.on("leaveNode", () => latest.current.onHover(undefined));
    renderer.on("clickStage", () => latest.current.onClear());
    renderer.on("afterRender", () => {
      const current = latest.current, sections = new Map(current.layout.sections.map((section) => [section.id, section]));
      for (const child of Array.from(labels.current?.children ?? []) as HTMLElement[]) {
        const section = sections.get(child.dataset.sectionId ?? ""); if (!section || !model.order) continue;
        const point = renderer.graphToViewport({ x: section.x, y: -section.y + section.radius + 30 });
        child.style.transform = `translate(${point.x}px, ${point.y}px) translate(-50%, -100%)`;
        child.style.opacity = renderer.getCamera().ratio < 0.22 ? "0" : "0.68";
      }
    });
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotion = () => {
      // Sigma divides elapsed time by duration, so one millisecond avoids 0/0.
      renderer.setSetting("zoomDuration", motion.matches ? 1 : 180);
      renderer.setSetting("doubleClickZoomingDuration", motion.matches ? 1 : 220);
      renderer.setSetting("inertiaDuration", motion.matches ? 1 : 160);
      if (motion.matches && renderer.getCamera().isAnimated()) void renderer.getCamera().animate(renderer.getCamera().getState(), { duration: 1 });
    };
    updateMotion(); motion.addEventListener("change", updateMotion);
    const contextLost = (event: Event) => { event.preventDefault(); latest.current.onError("The WebGL context was lost. The SVG canvas remains available."); };
    const canvases = Object.values(renderer.getCanvases());
    canvases.forEach((canvas) => canvas.addEventListener("webglcontextlost", contextLost));
    return () => {
      motion.removeEventListener("change", updateMotion);
      canvases.forEach((canvas) => canvas.removeEventListener("webglcontextlost", contextLost));
      renderer.kill(); model.clear(); rendererRef.current = null; modelRef.current = null;
    };
  }, [props.libraries]);

  useEffect(() => {
    const renderer = rendererRef.current, model = modelRef.current;
    if (!renderer || !model) return;
    try {
      const { graph, layout, theme } = props;
      const bounds = graphBounds([...layout.points.values()]);
      // Keep the world-to-camera mapping stable as new sections arrive. Explicit Fit
      // or a layout change recalculates the bounds and includes newly added URLs.
      if ((layout.points.size && !renderer.getCustomBBox()) || previousLayout.current !== layout.mode) {
        renderer.setCustomBBox({ x: [bounds.x, bounds.x + bounds.width], y: [-bounds.y - bounds.height, -bounds.y] });
      }
      model.clear();
      for (const node of graph.nodes) {
        const point = layout.points.get(node.url); if (!point) continue;
        model.addNode(node.url, { x: point.x, y: -point.y, label: graphNodeLabel(node).slice(0, 64), size: nodeSize(node),
          color: sectionColor(layout.sectionByUrl.get(node.url), theme), forceLabel: node.depth === 0 || graph.nodes.length < 18 });
      }
      graph.edges.forEach((edge, index) => {
        if (model.hasNode(edge.sourceUrl) && model.hasNode(edge.targetUrl)) model.addDirectedEdgeWithKey(`${edge.id}-${index}`, edge.sourceUrl, edge.targetUrl, { type: "arrow", color: "#81929d", size: 0.7, edge });
      });
      renderer.refresh();
      if (previousLayout.current !== layout.mode) { previousLayout.current = layout.mode; fit(); }
      props.onReady(true);
    } catch { latest.current.onError("The WebGL renderer could not update. The SVG canvas remains available."); }
  }, [props.graph, props.layout, props.libraries]);

  useEffect(() => {
    const renderer = rendererRef.current;
    if (!renderer) return;
    renderer.setSetting("labelColor", { color: props.theme === "dark" ? "#d5d9df" : "#33424d" });
    renderer.refresh({ skipIndexation: true });
  }, [props.theme, props.focusUrl, props.highlighted, props.neighbors, props.libraries]);

  const urls = new Set(props.graph.nodes.map((node) => node.url));
  return <div className={`cg-webgl ${props.ready ? "is-ready" : ""}`} aria-hidden={!props.ready}>
    <div ref={container} className="graph-canvas" />
    <div ref={labels} className="cg-webgl-labels" aria-hidden="true">{props.layout.mode === "clusters" ? props.layout.sections.filter((section) => section.urls.some((url) => urls.has(url))).slice(0, 24).map((section) =>
      <span key={section.id} data-section-id={section.id} style={{ color: sectionColor(section, props.theme) }}>{section.label === "/" ? section.host : section.label}</span>) : null}</div>
  </div>;
}
