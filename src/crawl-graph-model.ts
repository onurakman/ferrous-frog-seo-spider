export type GraphStatusFilter = "all" | "success" | "redirect" | "broken" | "brokenLinks" | "redirectLinks" | "external" | "uncrawled";
export type GraphLayoutMode = "clusters" | "depth" | "radial";
export type GraphNode = {
  url: string; label: string; crawled: boolean;
  classification?: "internal" | "external" | null;
  statusCode?: number | null; depth?: number | null; indexability?: string | null;
  inlinkCount: number; outlinkCount: number;
};
export type GraphEdge = {
  id: number; sourceUrl: string; targetUrl: string; anchorText: string;
  rel: string; relNofollow: boolean; linkType: "internal" | "external";
  sourceStatusCode?: number | null; targetStatusCode?: number | null;
  sourceDepth: number; targetDepth?: number | null; sourcePosition: number; discoveryOrder: number;
};
export type CrawlGraph = { nodes: GraphNode[]; edges: GraphEdge[]; totalNodes: number; totalEdges: number };
export type Point = { x: number; y: number };
export type GraphSection = Point & { id: string; label: string; host: string; index: number; urls: string[]; radius: number };
export type GraphLayout = {
  mode: GraphLayoutMode; points: Map<string, Point>; sections: GraphSection[];
  sectionByUrl: Map<string, GraphSection>;
};

export function graphSection(value: string) {
  try {
    const url = new URL(value);
    const segment = url.pathname.split("/").find(Boolean);
    const path = segment ? `/${segment}` : "/";
    return { id: `${url.host}${path}`, host: url.host, label: readableUrl(path) };
  } catch {
    return { id: "other", host: "Other URLs", label: "Other URLs" };
  }
}

export function readableUrl(value: string) {
  try { return decodeURI(value); } catch { return value; }
}

export function graphNodeLabel(node: GraphNode) {
  try {
    const url = new URL(node.url);
    return readableUrl(url.pathname === "/" ? url.host : `${url.pathname}${url.search}`);
  } catch { return node.label || node.url; }
}

export function isBrokenNode(node: GraphNode) {
  return (typeof node.statusCode === "number" && node.statusCode >= 400) || (node.crawled && node.statusCode == null);
}

export function graphStatusLabel(node: GraphNode) {
  if (!node.crawled) return "Uncrawled";
  if (node.statusCode == null) return "No response";
  if (node.statusCode >= 400) return `${node.statusCode} · Broken`;
  if (node.statusCode >= 300) return `${node.statusCode} · Redirect`;
  if (node.statusCode >= 200) return `${node.statusCode} · Success`;
  return String(node.statusCode);
}

export function brokenGraphTargets(graph?: CrawlGraph) {
  return new Set(graph?.nodes.filter(isBrokenNode).map((node) => node.url));
}

export function isBrokenGraphEdge(edge: GraphEdge, brokenTargets: Set<string>) {
  return (typeof edge.targetStatusCode === "number" && edge.targetStatusCode >= 400) || brokenTargets.has(edge.targetUrl);
}

export function isRedirectGraphEdge(edge: GraphEdge) {
  return typeof edge.targetStatusCode === "number" && edge.targetStatusCode >= 300 && edge.targetStatusCode < 400;
}

export function filterGraphSnapshot(graph: CrawlGraph | undefined, status: GraphStatusFilter, depth: string): CrawlGraph | undefined {
  if (!graph) return undefined;
  let edges = graph.edges;
  let endpoints: Set<string> | undefined;
  if (status === "brokenLinks" || status === "redirectLinks") {
    const brokenTargets = brokenGraphTargets(graph);
    edges = edges.filter((edge) => status === "brokenLinks" ? isBrokenGraphEdge(edge, brokenTargets) : isRedirectGraphEdge(edge));
    endpoints = new Set(edges.flatMap((edge) => [edge.sourceUrl, edge.targetUrl]));
  }
  const nodes = graph.nodes.filter((node) => {
    if (depth !== "all" && node.depth !== Number(depth)) return false;
    if (endpoints) return endpoints.has(node.url);
    switch (status) {
      case "success": return typeof node.statusCode === "number" && node.statusCode >= 200 && node.statusCode < 300;
      case "redirect": return typeof node.statusCode === "number" && node.statusCode >= 300 && node.statusCode < 400;
      case "broken": return isBrokenNode(node);
      case "external": return node.classification === "external";
      case "uncrawled": return !node.crawled;
      default: return true;
    }
  });
  const urls = new Set(nodes.map((node) => node.url));
  edges = edges.filter((edge) => urls.has(edge.sourceUrl) && urls.has(edge.targetUrl));
  return { nodes, edges, totalNodes: nodes.length, totalEdges: edges.length };
}

export function matchingGraphUrls(nodes: GraphNode[], search: string) {
  const needle = search.trim().toLowerCase();
  return new Set(nodes.filter((node) => `${readableUrl(node.url)} ${node.label}`.toLowerCase().includes(needle)).map((node) => node.url));
}

function hash(value: string) {
  let result = 2166136261;
  for (let i = 0; i < value.length; i++) result = Math.imul(result ^ value.charCodeAt(i), 16777619);
  result = Math.imul(result ^ (result >>> 16), 0x85ebca6b);
  result = Math.imul(result ^ (result >>> 13), 0xc2b2ae35);
  result ^= result >>> 16;
  return (result >>> 0) / 4294967296;
}

export function buildGraphLayout(nodes: GraphNode[], mode: GraphLayoutMode, previous?: GraphLayout): GraphLayout {
  const grouped = new Map<string, { id: string; host: string; label: string; urls: string[] }>();
  for (const node of nodes) {
    const section = graphSection(node.url);
    if (!grouped.has(section.id)) grouped.set(section.id, { ...section, urls: [] });
    grouped.get(section.id)!.urls.push(node.url);
  }
  const oldSections = new Map(previous?.sections.map((section) => [section.id, section]));
  let nextIndex = previous?.sections.reduce((index, section) => Math.max(index, section.index + 1), 0) ?? 0;
  const sections = [...grouped.values()].sort((a, b) => a.id.localeCompare(b.id)).map((section) => {
    const old = oldSections.get(section.id);
    const index = old?.index ?? nextIndex++;
    const angle = index * Math.PI * (3 - Math.sqrt(5));
    return { ...section, index, urls: section.urls.sort(), radius: Math.min(245, 52 + Math.sqrt(section.urls.length) * 13),
      x: old?.x ?? Math.cos(angle) * Math.sqrt(index) * 390,
      y: old?.y ?? Math.sin(angle) * Math.sqrt(index) * 390 };
  });
  const sectionByUrl = new Map(sections.flatMap((section) => section.urls.map((url) => [url, section] as const)));
  const points = new Map<string, Point>();
  for (const node of nodes) {
    const old = mode === "clusters" && previous?.mode === mode ? previous.points.get(node.url) : undefined;
    if (old) { points.set(node.url, old); continue; }
    const angle = hash(node.url) * Math.PI * 2;
    const depth = node.depth ?? 0;
    if (mode === "depth") points.set(node.url, { x: depth * 250, y: (hash(`${node.url}:y`) - 0.5) * 700 });
    else if (mode === "radial") {
      const radius = 80 + depth * 145 + hash(`${node.url}:r`) * 45;
      points.set(node.url, { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius });
    } else {
      const section = sectionByUrl.get(node.url)!;
      const radius = section.urls.length === 1 ? 0 : Math.sqrt(hash(`${node.url}:r`)) * section.radius;
      points.set(node.url, { x: section.x + Math.cos(angle) * radius, y: section.y + Math.sin(angle) * radius });
    }
  }
  return { mode, sections, sectionByUrl, points };
}

const COLORS = {
  light: ["#397e91", "#8b6cba", "#398d74", "#b47b45", "#ac668e", "#687faa", "#87984b", "#a5685e"],
  dark: ["#75b9c9", "#b298d8", "#79bda2", "#d4ac77", "#d29ab8", "#92a9d3", "#bac98a", "#d5a299"],
};
export function sectionColor(section: GraphSection | undefined, theme: "light" | "dark") {
  return COLORS[theme][(section?.index ?? 0) % COLORS[theme].length];
}

export function graphBounds(points: Point[], padding = 115) {
  if (!points.length) return { x: -250, y: -200, width: 500, height: 400 };
  const xs = points.map((point) => point.x), ys = points.map((point) => point.y);
  const x = Math.min(...xs) - padding, y = Math.min(...ys) - padding;
  return { x, y, width: Math.max(180, Math.max(...xs) + padding - x), height: Math.max(180, Math.max(...ys) + padding - y) };
}
