import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MultiDirectedGraph } from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import * as Checkbox from "@radix-ui/react-checkbox";
import * as Dialog from "@radix-ui/react-dialog";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import Sigma from "sigma";
import { EdgeArrowProgram, type EdgeProgramType } from "sigma/rendering";
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Download,
  GitFork,
  Info,
  MoreHorizontal,
  Moon,
  Network,
  Pause,
  Play,
  Plus,
  Search,
  Settings,
  Square,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { create } from "zustand";

type Theme = "light" | "dark";
type StorageMode = "memory" | "database";
type CrawlMode = "spider" | "list";
type ExtractorKind = "cssText" | "cssAttribute" | "xpath" | "regex";
type UrlClassification = "internal" | "external";
type LinkType = "internal" | "external";
type SortDirection = "asc" | "desc";
type GraphStatusFilter = "all" | "success" | "redirect" | "broken" | "external" | "uncrawled";
type GraphLayoutMode = "depth" | "radial";
type LinkEdgeView = "all" | "internal" | "external" | "broken" | "nofollow";
type LinkReportKind =
  | LinkEdgeView
  | "selectedInlinks"
  | "selectedOutlinks"
  | "redirects"
  | "anchorText";
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
  | "h1Missing"
  | "h1Duplicate"
  | "h1TooLong"
  | "h2Missing"
  | "h2Duplicate"
  | "h2TooLong"
  | "titleSameAsH1"
  | "canonicalMissing"
  | "canonicalMultiple"
  | "directivesNoindex"
  | "imagesMissingAlt"
  | "imagesAltTooLong"
  | "securityMixedContent"
  | "securityInsecureForms"
  | "securityMissingHsts"
  | "securityMissingCsp"
  | "securityMissingXFrameOptions"
  | "securityMissingContentTypeOptions"
  | "mobileMissingViewport"
  | "hreflangInvalid"
  | "hreflangMissingSelfReference"
  | "structuredDataInvalid"
  | "nearDuplicate"
  | "brokenLinks"
  | "sitemapOrphan";

type RedirectHop = {
  url: string;
  statusCode: number;
  location?: string | null;
  dnsLookupTimeMs?: number | null;
  ttfbMs?: number | null;
  elapsedMs?: number | null;
};

type CrawlRecord = {
  id: number;
  url: string;
  finalUrl: string;
  classification: UrlClassification;
  inSitemap: boolean;
  statusCode?: number | null;
  statusText: string;
  contentType?: string | null;
  indexability: string;
  indexabilityStatus: string;
  responseTimeMs: number;
  dnsLookupTimeMs?: number | null;
  ttfbMs?: number | null;
  downloadTimeMs?: number | null;
  totalNetworkTimeMs?: number | null;
  transferRateBytesPerSec?: number | null;
  resolvedIpCount: number;
  sizeBytes: number;
  responseHash?: string | null;
  depth: number;
  redirectTarget?: string | null;
  redirectType?: string | null;
  redirectChain: RedirectHop[];
  title?: string | null;
  titleLen: number;
  metaDescription?: string | null;
  metaDescriptionLen: number;
  metaRobots?: string | null;
  xRobotsTag?: string | null;
  h1?: string | null;
  h1Len: number;
  h1Count: number;
  h2?: string | null;
  h2Len: number;
  h2Count: number;
  canonical?: string | null;
  canonicalCount: number;
  simhash?: number | null;
  wordCount: number;
  textToCodeRatio: number;
  imageCount: number;
  imagesMissingAlt: number;
  imagesAltTooLong: number;
  mixedContentCount: number;
  insecureFormCount: number;
  hstsHeader: boolean;
  contentSecurityPolicyHeader: boolean;
  xFrameOptionsHeader: boolean;
  xContentTypeOptionsHeader: boolean;
  viewport: boolean;
  amphtml?: string | null;
  relNext?: string | null;
  relPrev?: string | null;
  hreflangCount: number;
  hreflangInvalidCount: number;
  hreflangMissingSelfReference: boolean;
  jsonLdCount: number;
  jsonLdInvalidCount: number;
  openGraphCount: number;
  twitterCardCount: number;
  nearDuplicateClusterId?: number | null;
  inlinkCount: number;
  outlinkCount: number;
  internalOutlinkCount: number;
  externalOutlinkCount: number;
  customExtractions: CustomExtractionValue[];
  error?: string | null;
};

type CustomExtractionValue = {
  name: string;
  values: string[];
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
  nearDuplicates: number;
  indexable: number;
  nonIndexable: number;
  titleMissing: number;
  titleDuplicate: number;
  metaMissing: number;
  metaDuplicate: number;
  h1Missing: number;
  h1Duplicate: number;
  h2Missing: number;
  h2Duplicate: number;
  canonicalMissing: number;
  canonicalMultiple: number;
  noindex: number;
  imagesMissingAlt: number;
  imagesAltTooLong: number;
  mixedContent: number;
  insecureForms: number;
  hreflangInvalid: number;
  structuredDataInvalid: number;
  missingViewport: number;
  missingHsts: number;
  sitemapOrphans: number;
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

type ProgressSample = {
  timestamp: number;
  crawled: number;
  queued: number;
  discovered: number;
  pagesPerSecond: number;
};

type CrawlSession = {
  id: string;
  name: string;
  startUrl: string;
  databasePath: string;
  createdAtMs: number;
  updatedAtMs: number;
  isCurrent: boolean;
};

type ConfigProfile = {
  id: string;
  name: string;
  config: CrawlConfig;
  createdAtMs: number;
  updatedAtMs: number;
};

type RobotsTxtTestResult = {
  allowed: boolean;
  crawlDelayMs?: number | null;
};

type RobotsTxtDownloadResult = {
  robotsUrl: string;
  statusCode: number;
  robotsTxt: string;
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

type LinkEdge = {
  id: number;
  sourceUrl: string;
  targetUrl: string;
  anchorText: string;
  rel: string;
  relNofollow: boolean;
  linkType: LinkType;
  sourceStatusCode?: number | null;
  targetStatusCode?: number | null;
  sourceDepth: number;
  targetDepth?: number | null;
  sourcePosition: number;
  discoveryOrder: number;
};

type LinkEdgeResponse = {
  edges: LinkEdge[];
  total: number;
};

type AnchorTextRow = {
  anchorText: string;
  targetUrl: string;
  linkType: LinkType;
  linkCount: number;
  sourceCount: number;
  nofollowCount: number;
  firstSourceUrl: string;
  targetStatusCode?: number | null;
};

type AnchorTextResponse = {
  rows: AnchorTextRow[];
  total: number;
};

type GraphNode = {
  url: string;
  label: string;
  crawled: boolean;
  classification?: UrlClassification | null;
  statusCode?: number | null;
  depth?: number | null;
  indexability?: string | null;
  inlinkCount: number;
  outlinkCount: number;
};

type CrawlGraph = {
  nodes: GraphNode[];
  edges: LinkEdge[];
  totalNodes: number;
  totalEdges: number;
};

type GraphNodeAttributes = {
  label: string;
  url: string;
  status: string;
  depth: string;
  inlinks: number;
  outlinks: number;
  x: number;
  y: number;
  size: number;
  color: string;
  forceLabel: boolean;
};

type GraphEdgeAttributes = {
  label: string;
  status: string;
  size: number;
  color: string;
  type: "arrow";
};

type OverviewTone = "success" | "warning" | "danger" | "muted";

type OverviewRowModel = {
  label: string;
  value: number;
  tone?: OverviewTone;
  view?: IssueView;
};

type OverviewStatusSegment = {
  label: string;
  value: number;
  className: string;
  view?: IssueView;
};

type CrawlConfig = {
  mode: CrawlMode;
  startUrl: string;
  listUrls: string[];
  maxUrls: number;
  maxDepth: number;
  concurrency: number;
  requestsPerSecond: number;
  requestDelayMs: number;
  respectRobots: boolean;
  useRobotsTxtOverride: boolean;
  robotsTxtOverride: string;
  userAgent: string;
  timeoutSecs: number;
  maxRedirects: number;
  nearDuplicateThreshold: number;
  includeUrlPatterns: string[];
  excludeUrlPatterns: string[];
  resourceTypes: ResourceTypeConfig;
  querySettings: QuerySettingsConfig;
  customExtractors: CustomExtractor[];
};

type ResourceTypeConfig = {
  html: boolean;
  images: boolean;
  css: boolean;
  javascript: boolean;
  external: boolean;
  other: boolean;
};

type QuerySettingsConfig = {
  sortParameters: boolean;
  stripAll: boolean;
  maxParameters: number;
  stripParameterPatterns: string[];
};

type CustomExtractor = {
  name: string;
  kind: ExtractorKind;
  pattern: string;
  attribute?: string | null;
  allMatches: boolean;
};

type AppState = {
  theme: Theme;
  storageMode: StorageMode;
  resumeCrawl: boolean;
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
  paused: boolean;
  error?: string;
  setConfig: (config: Partial<CrawlConfig>) => void;
  setRows: (response: GridResponse) => void;
  upsertLiveRecord: (
    record: CrawlRecord,
    view: IssueView,
    search: string,
    sortBy?: string,
    sortDir?: SortDirection,
  ) => void;
  setSelected: (record?: CrawlRecord) => void;
  setView: (view: IssueView) => void;
  setSearch: (search: string) => void;
  setSort: (sortBy: string) => void;
  setProgress: (progress?: CrawlProgress) => void;
  setRunning: (running: boolean) => void;
  setPaused: (paused: boolean) => void;
  setError: (error?: string) => void;
  setTheme: (theme: Theme) => void;
  setStorageMode: (storageMode: StorageMode) => void;
  setResumeCrawl: (resumeCrawl: boolean) => void;
};

const themeStorageKey = "ferrous-frog-theme";
const lastUrlStorageKey = "ferrous-frog-last-url";
const overviewWidthStorageKey = "ferrous-frog-overview-width";
const overviewMinWidth = 260;
const overviewMaxWidth = 560;

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
  nearDuplicates: 0,
  indexable: 0,
  nonIndexable: 0,
  titleMissing: 0,
  titleDuplicate: 0,
  metaMissing: 0,
  metaDuplicate: 0,
  h1Missing: 0,
  h1Duplicate: 0,
  h2Missing: 0,
  h2Duplicate: 0,
  canonicalMissing: 0,
  canonicalMultiple: 0,
  noindex: 0,
  imagesMissingAlt: 0,
  imagesAltTooLong: 0,
  mixedContent: 0,
  insecureForms: 0,
  hreflangInvalid: 0,
  structuredDataInvalid: 0,
  missingViewport: 0,
  missingHsts: 0,
  sitemapOrphans: 0,
};

const defaultConfig: CrawlConfig = {
  mode: "spider",
  startUrl: getInitialStartUrl(),
  listUrls: [],
  maxUrls: 250,
  maxDepth: 3,
  concurrency: 4,
  requestsPerSecond: 2,
  requestDelayMs: 250,
  respectRobots: true,
  useRobotsTxtOverride: false,
  robotsTxtOverride: "",
  userAgent: "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)",
  timeoutSecs: 20,
  maxRedirects: 10,
  nearDuplicateThreshold: 6,
  includeUrlPatterns: [],
  excludeUrlPatterns: [],
  resourceTypes: {
    html: true,
    images: false,
    css: false,
    javascript: false,
    external: false,
    other: false,
  },
  querySettings: {
    sortParameters: false,
    stripAll: false,
    maxParameters: 0,
    stripParameterPatterns: [],
  },
  customExtractors: [],
};

const useAppStore = create<AppState>((set, get) => ({
  theme: getInitialTheme(),
  storageMode: "memory",
  resumeCrawl: false,
  config: defaultConfig,
  rows: [],
  selectedView: "all",
  globalSearch: "",
  sortDir: "asc",
  total: 0,
  summary: emptySummary,
  running: false,
  paused: false,
  setConfig: (config) =>
    set((state) => ({ config: { ...state.config, ...config } })),
  setRows: (response) =>
    set((state) => ({
      rows: response.rows,
      total: response.total,
      summary: response.summary,
      selected: state.selected
        ? response.rows.find((row) => row.id === state.selected?.id)
        : undefined,
    })),
  upsertLiveRecord: (record, view, search, sortBy, sortDir) =>
    set((state) => {
      const existed = state.rows.some((row) => row.id === record.id);
      const currentRows = state.rows.filter((row) => row.id !== record.id);
      const shouldShow = recordMatchesView(record, view) && recordMatchesSearch(record, search);
      const rows = shouldShow ? [...currentRows, record] : currentRows;
      sortLiveRows(rows, sortBy, sortDir);
      const limitedRows = rows.slice(0, 1000);
      const totalDelta = shouldShow && !existed ? 1 : !shouldShow && existed ? -1 : 0;

      return {
        rows: limitedRows,
        total: Math.max(0, state.total + totalDelta),
        selected:
          state.selected?.id === record.id
            ? record
            : state.selected &&
                limitedRows.some((row) => row.id === state.selected?.id)
              ? state.selected
              : undefined,
      };
    }),
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
  setPaused: (paused) => set({ paused }),
  setError: (error) => set({ error }),
  setTheme: (theme) => set({ theme }),
  setStorageMode: (storageMode) => set({ storageMode }),
  setResumeCrawl: (resumeCrawl) => set({ resumeCrawl }),
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
  { id: "titleSameAsH1", label: "Title = H1" },
  { id: "metaMissing", label: "Missing Meta" },
  { id: "metaDuplicate", label: "Duplicate Meta" },
  { id: "metaTooShort", label: "Short Meta" },
  { id: "metaTooLong", label: "Long Meta" },
  { id: "h1Missing", label: "Missing H1" },
  { id: "h1Duplicate", label: "Duplicate H1" },
  { id: "h1TooLong", label: "Long H1" },
  { id: "h2Missing", label: "Missing H2" },
  { id: "h2Duplicate", label: "Duplicate H2" },
  { id: "h2TooLong", label: "Long H2" },
  { id: "canonicalMissing", label: "Missing Canonical" },
  { id: "canonicalMultiple", label: "Multiple Canonicals" },
  { id: "directivesNoindex", label: "Noindex" },
  { id: "imagesMissingAlt", label: "Missing Alt" },
  { id: "imagesAltTooLong", label: "Long Alt" },
  { id: "securityMixedContent", label: "Mixed Content" },
  { id: "securityInsecureForms", label: "Insecure Forms" },
  { id: "securityMissingHsts", label: "Missing HSTS" },
  { id: "securityMissingCsp", label: "Missing CSP" },
  { id: "securityMissingXFrameOptions", label: "Missing XFO" },
  { id: "securityMissingContentTypeOptions", label: "Missing XCTO" },
  { id: "mobileMissingViewport", label: "Missing Viewport" },
  { id: "hreflangInvalid", label: "Invalid Hreflang" },
  { id: "hreflangMissingSelfReference", label: "Hreflang Self Ref" },
  { id: "structuredDataInvalid", label: "Invalid JSON-LD" },
  { id: "nearDuplicate", label: "Near Duplicates" },
  { id: "brokenLinks", label: "Broken Links" },
  { id: "sitemapOrphan", label: "Sitemap Orphans" },
];

type GridColumn =
  | {
      kind: "native";
      key: keyof CrawlRecord;
      label: string;
      width: number;
      grow?: number;
      sortable: boolean;
    }
  | {
      kind: "custom";
      name: string;
      id: string;
      label: string;
      width: number;
      grow?: number;
      sortable: false;
    };

const nativeColumns: GridColumn[] = [
  { kind: "native", key: "statusCode", label: "Status", width: 76, sortable: true },
  {
    kind: "native",
    key: "finalUrl",
    label: "URL",
    width: 460,
    grow: 1.5,
    sortable: true,
  },
  {
    kind: "native",
    key: "title",
    label: "Title",
    width: 280,
    grow: 1,
    sortable: true,
  },
  {
    kind: "native",
    key: "metaDescription",
    label: "Meta",
    width: 260,
    grow: 1,
    sortable: true,
  },
  {
    kind: "native",
    key: "h1",
    label: "H1",
    width: 220,
    sortable: true,
  },
  {
    kind: "native",
    key: "h2",
    label: "H2",
    width: 220,
    sortable: true,
  },
  {
    kind: "native",
    key: "indexability",
    label: "Indexability",
    width: 120,
    sortable: false,
  },
  {
    kind: "native",
    key: "contentType",
    label: "Content Type",
    width: 150,
    sortable: false,
  },
  {
    kind: "native",
    key: "inSitemap",
    label: "In Sitemap",
    width: 104,
    sortable: true,
  },
  {
    kind: "native",
    key: "responseTimeMs",
    label: "Time",
    width: 82,
    sortable: true,
  },
  {
    kind: "native",
    key: "dnsLookupTimeMs",
    label: "DNS",
    width: 72,
    sortable: true,
  },
  {
    kind: "native",
    key: "ttfbMs",
    label: "TTFB",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "downloadTimeMs",
    label: "Down",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "wordCount",
    label: "Words",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "nearDuplicateClusterId",
    label: "Dup",
    width: 72,
    sortable: true,
  },
  {
    kind: "native",
    key: "canonicalCount",
    label: "Canon",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "imagesMissingAlt",
    label: "Alt",
    width: 66,
    sortable: true,
  },
  {
    kind: "native",
    key: "mixedContentCount",
    label: "Mixed",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "insecureFormCount",
    label: "Forms",
    width: 72,
    sortable: true,
  },
  {
    kind: "native",
    key: "hreflangInvalidCount",
    label: "Hfl",
    width: 64,
    sortable: true,
  },
  {
    kind: "native",
    key: "jsonLdInvalidCount",
    label: "LD",
    width: 58,
    sortable: true,
  },
  { kind: "native", key: "depth", label: "Depth", width: 70, sortable: true },
  {
    kind: "native",
    key: "inlinkCount",
    label: "Inlinks",
    width: 82,
    sortable: true,
  },
  {
    kind: "native",
    key: "outlinkCount",
    label: "Outlinks",
    width: 88,
    sortable: true,
  },
];

const extractorKinds: Array<{ value: ExtractorKind; label: string }> = [
  { value: "cssText", label: "CSS text" },
  { value: "cssAttribute", label: "CSS attribute" },
  { value: "xpath", label: "XPath" },
  { value: "regex", label: "Regex" },
];

const linkReportViews: Array<{ id: LinkReportKind; label: string }> = [
  { id: "all", label: "All Links" },
  { id: "internal", label: "Internal" },
  { id: "external", label: "External" },
  { id: "broken", label: "Broken" },
  { id: "nofollow", label: "Nofollow" },
  { id: "anchorText", label: "Anchor Text" },
  { id: "selectedInlinks", label: "Selected Inlinks" },
  { id: "selectedOutlinks", label: "Selected Outlinks" },
  { id: "redirects", label: "Redirect Chains" },
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
    paused,
    error,
    theme,
    storageMode,
    resumeCrawl,
    setConfig,
    setRows,
    upsertLiveRecord,
    setSelected,
    setView,
    setSearch,
    setSort,
    setProgress,
    setRunning,
    setPaused,
    setError,
    setTheme,
    setStorageMode,
    setResumeCrawl,
  } = useAppStore();
  const parentRef = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<HTMLElement>(null);
  const desktopRuntime = isTauri();

  const columns = useMemo<GridColumn[]>(() => {
    const customColumns = config.customExtractors
      .filter((extractor) => extractor.name.trim().length > 0)
      .map<GridColumn>((extractor, index) => ({
        kind: "custom",
        name: extractor.name,
        id: `custom:${extractor.name}:${index}`,
        label: extractor.name,
        width: 180,
        sortable: false,
      }));

    return [...nativeColumns, ...customColumns];
  }, [config.customExtractors]);

  const tableColumns = useMemo<ColumnDef<CrawlRecord>[]>(
    () =>
      columns.map((column) => ({
        id: columnKey(column),
        header: column.label,
        size: column.width,
        minSize: column.width,
        enableSorting: column.kind === "native" && column.sortable,
        cell: ({ row }) => formatCell(row.original, column),
      })),
    [columns],
  );
  const gridWidth = useMemo(
    () => columns.reduce((totalWidth, column) => totalWidth + column.width, 0),
    [columns],
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [linkReportsOpen, setLinkReportsOpen] = useState(false);
  const [selectedLinkReport, setSelectedLinkReport] = useState<LinkReportKind>("all");
  const [linkEdges, setLinkEdges] = useState<LinkEdge[]>([]);
  const [linkEdgeTotal, setLinkEdgeTotal] = useState(0);
  const [anchorTextRows, setAnchorTextRows] = useState<AnchorTextRow[]>([]);
  const [anchorTextTotal, setAnchorTextTotal] = useState(0);
  const [redirectReportRows, setRedirectReportRows] = useState<CrawlRecord[]>([]);
  const [redirectReportTotal, setRedirectReportTotal] = useState(0);
  const [linkReportLoading, setLinkReportLoading] = useState(false);
  const [linkReportSearch, setLinkReportSearch] = useState("");
  const [linkReportSortBy, setLinkReportSortBy] = useState("sourceUrl");
  const [linkReportSortDir, setLinkReportSortDir] = useState<SortDirection>("asc");
  const [anchorTextSortBy, setAnchorTextSortBy] = useState("linkCount");
  const [anchorTextSortDir, setAnchorTextSortDir] = useState<SortDirection>("desc");
  const [graphOpen, setGraphOpen] = useState(false);
  const [graph, setGraph] = useState<CrawlGraph>();
  const [graphLoading, setGraphLoading] = useState(false);
  const [graphUpdatedAt, setGraphUpdatedAt] = useState<number>();
  const [graphInternalOnly, setGraphInternalOnly] = useState(false);
  const [graphStatusFilter, setGraphStatusFilter] = useState<GraphStatusFilter>("all");
  const [graphDepthFilter, setGraphDepthFilter] = useState("all");
  const [graphLayoutMode, setGraphLayoutMode] = useState<GraphLayoutMode>("depth");
  const [progressHistory, setProgressHistory] = useState<ProgressSample[]>([]);
  const [crawlSessions, setCrawlSessions] = useState<CrawlSession[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [newSessionName, setNewSessionName] = useState("");
  const [configProfiles, setConfigProfiles] = useState<ConfigProfile[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [newProfileName, setNewProfileName] = useState("");
  const [robotsTestUrl, setRobotsTestUrl] = useState("");
  const [robotsTestResult, setRobotsTestResult] = useState<string>();
  const [overviewWidth, setOverviewWidth] = useState(getInitialOverviewWidth);
  const [overviewResizing, setOverviewResizing] = useState(false);
  const exportDisabled = rows.length === 0 || !desktopRuntime;
  const crawlStateLabel = !desktopRuntime
    ? "Desktop required"
    : paused
      ? "Paused"
      : running
      ? "Crawling"
      : progress?.status === "finished"
        ? "Finished"
        : "Ready";
  const progressCrawled = progress?.crawled ?? summary.total;
  const progressDiscovered = Math.max(progress?.discovered ?? summary.total, progressCrawled);
  const progressPercent =
    progressDiscovered > 0
      ? Math.min(100, Math.round((progressCrawled / progressDiscovered) * 100))
      : 0;
  const crawlTargetLabel =
    config.mode === "list"
      ? `${config.listUrls.length || 1} list URL${config.listUrls.length === 1 ? "" : "s"}`
      : config.startUrl;
  const selectedUrl = selected?.finalUrl;
  const table = useReactTable({
    data: rows,
    columns: tableColumns,
    getCoreRowModel: getCoreRowModel(),
    manualSorting: true,
    enableSortingRemoval: false,
  });
  const tableRows = table.getRowModel().rows;

  const rowVirtualizer = useVirtualizer({
    count: tableRows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 38,
    overscan: 12,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const virtualPaddingTop = virtualRows[0]?.start ?? 0;
  const virtualPaddingBottom =
    virtualRows.length > 0
      ? rowVirtualizer.getTotalSize() - virtualRows[virtualRows.length - 1].end
      : 0;

  const appendProgressSample = useCallback((nextProgress: CrawlProgress) => {
    setProgressHistory((history) => [
      ...history.slice(-79),
      {
        timestamp: Date.now(),
        crawled: nextProgress.crawled,
        queued: nextProgress.queued,
        discovered: nextProgress.discovered,
        pagesPerSecond: nextProgress.pagesPerSecond,
      },
    ]);
  }, []);

  const loadRows = useCallback(async () => {
    if (!desktopRuntime) {
      setError("Open the desktop app with make dev to run crawls.");
      return;
    }

    try {
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
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }, [
    desktopRuntime,
    globalSearch,
    selectedView,
    setError,
    setRows,
    sortBy,
    sortDir,
  ]);

  const loadSessions = useCallback(async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const sessions = await invoke<CrawlSession[]>("list_crawl_sessions");
      setCrawlSessions(sessions);
      setSelectedSessionId(sessions.find((session) => session.isCurrent)?.id ?? "");
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }, [desktopRuntime, setError]);

  const loadProfiles = useCallback(async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const profiles = await invoke<ConfigProfile[]>("list_config_profiles");
      setConfigProfiles(profiles);
      if (!profiles.some((profile) => profile.id === selectedProfileId)) {
        setSelectedProfileId("");
      }
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }, [desktopRuntime, selectedProfileId, setError]);

  const loadLinkReport = useCallback(
    async (report: LinkReportKind) => {
      if (!desktopRuntime) {
        return;
      }

      setLinkReportLoading(true);
      try {
        if (report === "redirects") {
          const response = await invoke<GridResponse>("get_rows", {
            query: {
              offset: 0,
              limit: 500,
              globalSearch: linkReportSearch.trim() || null,
              sortBy: "finalUrl",
              sortDir: "asc",
              view: "status3xx",
            },
          });
          setRedirectReportRows(response.rows);
          setRedirectReportTotal(response.total);
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setAnchorTextRows([]);
          setAnchorTextTotal(0);
          return;
        }

        if (report === "anchorText") {
          const response = await invoke<AnchorTextResponse>("get_anchor_texts", {
            query: {
              offset: 0,
              limit: 500,
              globalSearch: linkReportSearch.trim() || null,
              sortBy: anchorTextSortBy,
              sortDir: anchorTextSortDir,
              view: "all",
              internalOnly: false,
            },
          });
          setAnchorTextRows(response.rows);
          setAnchorTextTotal(response.total);
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setRedirectReportRows([]);
          setRedirectReportTotal(0);
          return;
        }

        const needsSelected = report === "selectedInlinks" || report === "selectedOutlinks";
        if (needsSelected && !selectedUrl) {
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setAnchorTextRows([]);
          setAnchorTextTotal(0);
          setRedirectReportRows([]);
          setRedirectReportTotal(0);
          return;
        }

        const response = await invoke<LinkEdgeResponse>("get_link_edges", {
          query: {
            offset: 0,
            limit: 500,
            globalSearch: linkReportSearch.trim() || null,
            sortBy: linkReportSortBy,
            sortDir: linkReportSortDir,
            view: linkReportEdgeView(report),
            sourceUrl: report === "selectedOutlinks" ? selectedUrl : null,
            targetUrl: report === "selectedInlinks" ? selectedUrl : null,
            internalOnly: false,
          },
        });
        setLinkEdges(response.edges);
        setLinkEdgeTotal(response.total);
        setAnchorTextRows([]);
        setAnchorTextTotal(0);
        setRedirectReportRows([]);
        setRedirectReportTotal(0);
      } catch (caught) {
        setError(errorMessage(caught));
      } finally {
        setLinkReportLoading(false);
      }
    },
    [
      desktopRuntime,
      anchorTextSortBy,
      anchorTextSortDir,
      linkReportSearch,
      linkReportSortBy,
      linkReportSortDir,
      selectedUrl,
      setError,
    ],
  );

  const loadGraph = useCallback(async (silent = false) => {
    if (!desktopRuntime) {
      return;
    }

    if (!silent) {
      setGraphLoading(true);
    }
    try {
      const response = await invoke<CrawlGraph>("get_crawl_graph", {
        query: {
          maxNodes: 700,
          maxEdges: 1_500,
          internalOnly: graphInternalOnly,
        },
      });
      setGraph(response);
      setGraphUpdatedAt(Date.now());
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      if (!silent) {
        setGraphLoading(false);
      }
    }
  }, [desktopRuntime, graphInternalOnly, setError]);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  useEffect(() => {
    if (settingsOpen) {
      void loadSessions();
      void loadProfiles();
    }
  }, [loadProfiles, loadSessions, settingsOpen]);

  useEffect(() => {
    if (linkReportsOpen) {
      void loadLinkReport(selectedLinkReport);
    }
  }, [linkReportsOpen, loadLinkReport, selectedLinkReport]);

  useEffect(() => {
    if (graphOpen) {
      void loadGraph();
    }
  }, [graphOpen, loadGraph]);

  useEffect(() => {
    if (!graphOpen || !running) {
      return;
    }

    const intervalId = window.setInterval(() => {
      void loadGraph(true);
    }, 1_500);

    return () => window.clearInterval(intervalId);
  }, [graphOpen, loadGraph, running]);

  useEffect(() => {
    applyTheme(theme);
    window.localStorage.setItem(themeStorageKey, theme);
  }, [theme]);

  useEffect(() => {
    const trimmedUrl = config.startUrl.trim();
    if (trimmedUrl.length > 0) {
      window.localStorage.setItem(lastUrlStorageKey, trimmedUrl);
    }
  }, [config.startUrl]);

  useEffect(() => {
    if (!desktopRuntime) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<CrawlerEvent>("crawl-event", (event) => {
      const payload = event.payload;
      if (payload.kind === "started") {
        setRunning(true);
        setPaused(false);
        setError(undefined);
      }
      if (payload.kind === "finished") {
        setRunning(false);
        setPaused(false);
      }
      if (payload.kind === "error") {
        setError(payload.message ?? "Crawler error");
      }
      if (payload.progress) {
        setProgress(payload.progress);
        appendProgressSample(payload.progress);
      }
      if (payload.record) {
        upsertLiveRecord(
          payload.record,
          selectedView,
          globalSearch,
          sortBy,
          sortDir,
        );
      }
      if (payload.kind === "finished") {
        void loadRows();
        if (graphOpen) {
          void loadGraph(true);
        }
      }
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlisten = dispose;
        }
      })
      .catch((caught) => {
        setError(errorMessage(caught));
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [
    desktopRuntime,
    appendProgressSample,
    globalSearch,
    graphOpen,
    loadGraph,
    loadRows,
    selectedView,
    setError,
    setProgress,
    setRunning,
    setPaused,
    sortBy,
    sortDir,
    upsertLiveRecord,
  ]);

  const startCrawl = async () => {
    if (!desktopRuntime) {
      setError("Open the desktop app with make dev to run crawls.");
      return;
    }

    setError(undefined);
    setRunning(true);
    setPaused(false);
    setProgressHistory([]);
    try {
      await invoke("start_crawl", { config, storageMode, resume: resumeCrawl });
      await loadRows();
    } catch (caught) {
      setRunning(false);
      setPaused(false);
      setError(errorMessage(caught));
    }
  };

  const pauseCrawl = async () => {
    try {
      await invoke("pause_crawl");
      setPaused(true);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const resumeActiveCrawl = async () => {
    try {
      await invoke("resume_crawl");
      setPaused(false);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const stopCrawl = async () => {
    try {
      await invoke("stop_crawl");
      setRunning(false);
      setPaused(false);
      await loadRows();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const resizeOverview = useCallback((clientX: number) => {
    const nextWidth = clamp(
      window.innerWidth - clientX,
      overviewMinWidth,
      Math.min(overviewMaxWidth, Math.max(overviewMinWidth, window.innerWidth - 360)),
    );
    setOverviewWidth(nextWidth);
    window.localStorage.setItem(overviewWidthStorageKey, String(nextWidth));
  }, []);

  const setOverviewPanelWidth = useCallback((width: number) => {
    const nextWidth = clamp(width, overviewMinWidth, overviewMaxWidth);
    setOverviewWidth(nextWidth);
    window.localStorage.setItem(overviewWidthStorageKey, String(nextWidth));
  }, []);

  const startOverviewResize = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    setOverviewResizing(true);
    resizeOverview(event.clientX);
  };

  const moveOverviewResize = (event: PointerEvent<HTMLDivElement>) => {
    if (overviewResizing) {
      resizeOverview(event.clientX);
    }
  };

  const stopOverviewResize = (event: PointerEvent<HTMLDivElement>) => {
    if (overviewResizing) {
      event.currentTarget.releasePointerCapture(event.pointerId);
      setOverviewResizing(false);
    }
  };

  const handleOverviewResizeKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setOverviewPanelWidth(overviewWidth + 24);
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      setOverviewPanelWidth(overviewWidth - 24);
    }
    if (event.key === "Home") {
      event.preventDefault();
      setOverviewPanelWidth(overviewMinWidth);
    }
    if (event.key === "End") {
      event.preventDefault();
      setOverviewPanelWidth(overviewMaxWidth);
    }
  };

  const primaryCrawlAction = () => {
    if (!running) {
      void startCrawl();
      return;
    }
    if (paused) {
      void resumeActiveCrawl();
      return;
    }
    void pauseCrawl();
  };

  const scrollTabs = (direction: -1 | 1) => {
    tabsRef.current?.scrollBy({
      left: direction * 280,
      behavior: "smooth",
    });
  };

  const addExtractor = () => {
    setConfig({
      customExtractors: [
        ...config.customExtractors,
        {
          name: `extractor_${config.customExtractors.length + 1}`,
          kind: "cssText",
          pattern: "h1",
          attribute: null,
          allMatches: false,
        },
      ],
    });
  };

  const updateExtractor = (index: number, patch: Partial<CustomExtractor>) => {
    setConfig({
      customExtractors: config.customExtractors.map((extractor, currentIndex) =>
        currentIndex === index ? { ...extractor, ...patch } : extractor,
      ),
    });
  };

  const removeExtractor = (index: number) => {
    setConfig({
      customExtractors: config.customExtractors.filter(
        (_, currentIndex) => currentIndex !== index,
      ),
    });
  };

  const updateResourceType = (key: keyof ResourceTypeConfig, checked: boolean) => {
    setConfig({
      resourceTypes: {
        ...config.resourceTypes,
        [key]: checked,
      },
    });
  };

  const updateQuerySettings = (patch: Partial<QuerySettingsConfig>) => {
    setConfig({
      querySettings: {
        ...config.querySettings,
        ...patch,
      },
    });
  };

  const testRobotsTxt = async () => {
    if (!desktopRuntime || !robotsTestUrl.trim() || !config.robotsTxtOverride.trim()) {
      return;
    }
    try {
      const result = await invoke<RobotsTxtTestResult>("test_robots_txt", {
        request: {
          userAgent: config.userAgent,
          robotsTxt: config.robotsTxtOverride,
          url: robotsTestUrl,
        },
      });
      const delay =
        result.crawlDelayMs !== null && result.crawlDelayMs !== undefined
          ? `, crawl-delay ${result.crawlDelayMs} ms`
          : "";
      setRobotsTestResult(`${result.allowed ? "Allowed" : "Blocked"}${delay}`);
    } catch (caught) {
      setRobotsTestResult(undefined);
      setError(errorMessage(caught));
    }
  };

  const downloadRobotsTxt = async () => {
    if (!desktopRuntime || !config.startUrl.trim()) {
      return;
    }
    try {
      const result = await invoke<RobotsTxtDownloadResult>("download_robots_txt", {
        request: {
          url: config.startUrl,
          userAgent: config.userAgent,
          timeoutSecs: config.timeoutSecs,
        },
      });
      setConfig({
        respectRobots: true,
        useRobotsTxtOverride: true,
        robotsTxtOverride: result.robotsTxt,
      });
      setRobotsTestResult(
        `Downloaded ${result.statusCode} from ${new URL(result.robotsUrl).pathname}`,
      );
    } catch (caught) {
      setRobotsTestResult(undefined);
      setError(errorMessage(caught));
    }
  };

  const createSession = async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const session = await invoke<CrawlSession>("create_crawl_session", {
        request: {
          name: newSessionName.trim() || sessionNameFromUrl(config.startUrl),
          startUrl: config.startUrl,
        },
      });
      setStorageMode("database");
      setResumeCrawl(true);
      setSelectedSessionId(session.id);
      setNewSessionName("");
      await loadSessions();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const openSession = async (sessionId: string) => {
    if (!desktopRuntime || !sessionId) {
      return;
    }
    try {
      const session = await invoke<CrawlSession>("open_crawl_session", { sessionId });
      setStorageMode("database");
      setResumeCrawl(true);
      setSelectedSessionId(session.id);
      if (session.startUrl.trim()) {
        setConfig({ startUrl: session.startUrl });
      }
      await loadSessions();
      await loadRows();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const deleteSession = async () => {
    if (!desktopRuntime || !selectedSessionId || running) {
      return;
    }
    try {
      await invoke("delete_crawl_session", { sessionId: selectedSessionId });
      setSelectedSessionId("");
      await loadSessions();
      await loadRows();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const saveProfile = async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const profile = await invoke<ConfigProfile>("save_config_profile", {
        request: {
          name: newProfileName.trim() || `${sessionNameFromUrl(config.startUrl)} profile`,
          config,
        },
      });
      setSelectedProfileId(profile.id);
      setNewProfileName("");
      await loadProfiles();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const loadProfile = async (profileId: string) => {
    if (!desktopRuntime || !profileId) {
      return;
    }
    try {
      const profile = await invoke<ConfigProfile>("load_config_profile", { profileId });
      setConfig(profile.config);
      setSelectedProfileId(profile.id);
      await loadProfiles();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const deleteProfile = async () => {
    if (!desktopRuntime || !selectedProfileId || running) {
      return;
    }
    try {
      await invoke("delete_config_profile", { profileId: selectedProfileId });
      setSelectedProfileId("");
      await loadProfiles();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportCsv = async () => {
    try {
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
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportXlsx = async () => {
    try {
      const bytes = await invoke<number[]>("export_xlsx", {
        query: {
          offset: 0,
          limit: 1_000_000,
          globalSearch,
          sortBy,
          sortDir,
          view: selectedView,
        },
      });
      const blob = new Blob([new Uint8Array(bytes)], {
        type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "ferrous-frog-export.xlsx";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportSitemap = async () => {
    try {
      const xml = await invoke<string>("export_sitemap", {
        query: {
          offset: 0,
          limit: 1_000_000,
          globalSearch,
          sortBy,
          sortDir,
          view: selectedView,
        },
      });
      const blob = new Blob([xml], { type: "application/xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "sitemap.xml";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportLinkEdgesCsv = async () => {
    try {
      const csv = await invoke<string>("export_link_edges_csv");
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "ferrous-frog-link-edges.csv";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportRedirectChainsCsv = async () => {
    try {
      const csv = await invoke<string>("export_redirect_chains_csv");
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "ferrous-frog-redirect-chains.csv";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportHtmlReport = async () => {
    try {
      const html = await invoke<string>("export_html_report");
      const blob = new Blob([html], { type: "text/html;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "ferrous-frog-seo-report.html";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const exportGraphJson = async () => {
    try {
      const graph = await invoke<unknown>("get_crawl_graph", {
        query: {
          maxNodes: 5_000,
          maxEdges: 10_000,
          internalOnly: false,
        },
      });
      const blob = new Blob([JSON.stringify(graph, null, 2)], {
        type: "application/json;charset=utf-8",
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "ferrous-frog-graph.json";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (caught) {
      setError(errorMessage(caught));
    }
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
          <select
            className="mode-select"
            aria-label="Crawl mode"
            value={config.mode}
            disabled={running}
            onChange={(event) => setConfig({ mode: event.target.value as CrawlMode })}
            title={running ? "Stop the crawl before changing mode" : "Crawl mode"}
          >
            <option value="spider">Spider</option>
            <option value="list">List</option>
          </select>
          <input
            aria-label={config.mode === "list" ? "Root URL" : "Seed URL"}
            value={config.startUrl}
            onChange={(event) => setConfig({ startUrl: event.target.value })}
            readOnly={running}
            aria-readonly={running}
            title={
              running
                ? "Stop the crawl before changing the URL"
                : config.mode === "list"
                  ? "Root URL"
                  : "Seed URL"
            }
            placeholder="https://example.com/"
          />
        </div>
        <div className="crawl-controls">
          <button
            className="primary"
            onClick={primaryCrawlAction}
            disabled={!desktopRuntime}
            title={!running ? "Start crawl" : paused ? "Resume crawl" : "Pause crawl"}
          >
            {!running || paused ? <Play size={16} /> : <Pause size={16} />}
            <span>{!running ? "Start" : paused ? "Resume" : "Pause"}</span>
          </button>
          <button
            className="destructive"
            onClick={stopCrawl}
            disabled={!running || !desktopRuntime}
            title="Stop crawl"
          >
            <Square size={16} />
            <span>Stop</span>
          </button>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button className="export-trigger" disabled={exportDisabled}>
                <Download size={16} />
                <span>Export</span>
                <ChevronDown size={14} />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content className="dropdown-content" align="end" sideOffset={8}>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportCsv()}
                >
                  CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportXlsx()}
                >
                  XLSX
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportSitemap()}
                >
                  XML Sitemap
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportLinkEdgesCsv()}
                >
                  Link Edges CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportRedirectChainsCsv()}
                >
                  Redirect Chains CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportHtmlReport()}
                >
                  HTML Report
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportGraphJson()}
                >
                  Graph JSON
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button className="compact-menu-trigger" title="More tools">
                <MoreHorizontal size={16} />
                <span>More</span>
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content className="dropdown-content toolbar-menu" align="end" sideOffset={8}>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  disabled={!desktopRuntime}
                  onSelect={() => setLinkReportsOpen(true)}
                >
                  <Network size={15} />
                  <span>Link Reports</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  disabled={!desktopRuntime}
                  onSelect={() => setGraphOpen(true)}
                >
                  <GitFork size={15} />
                  <span>Crawl Graph</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  onSelect={() => setSettingsOpen(true)}
                >
                  <Settings size={15} />
                  <span>Settings</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  onSelect={() => setTheme(theme === "dark" ? "light" : "dark")}
                >
                  {theme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
                  <span>{theme === "dark" ? "Light Theme" : "Dark Theme"}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  onSelect={() => setAboutOpen(true)}
                >
                  <Info size={15} />
                  <span>About</span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
      </header>

      <Dialog.Root open={aboutOpen} onOpenChange={setAboutOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="modal-backdrop" />
          <Dialog.Content className="about-modal">
            <div className="modal-header about-modal-header">
              <Dialog.Title asChild>
                <h2 id="about-title">About Ferrous Frog</h2>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button title="Close about">
                  <X size={16} />
                </button>
              </Dialog.Close>
            </div>

            <section className="about-hero" aria-labelledby="about-title">
              <span className="about-mark">FF</span>
              <div>
                <p className="about-kicker">Operation Oxidized Toad</p>
                <h3>It used to croak. Now it compiles.</h3>
                <p>
                  Ferrous Frog is a clean-room desktop SEO spider built with Rust,
                  Tauri, and a stubborn preference for fast local tooling.
                </p>
              </div>
            </section>

            <section className="about-section">
              <h4>Lineage</h4>
              <p>
                The workflow is inspired by professional technical SEO spiders,
                including Screaming Frog SEO Spider. Ferrous Frog is independent,
                unaffiliated, and does not use their name as branding, their logo,
                proprietary assets, or proprietary code.
              </p>
            </section>

            <section className="about-section">
              <h4>Current Build</h4>
              <div className="about-facts">
                <span>Rust crawler core</span>
                <span>Tauri 2 shell</span>
                <span>React data grid</span>
                <span>SQLite mode</span>
                <span>Polite by default</span>
                <span>Clean-room implementation</span>
              </div>
            </section>

            <section className="about-section about-note">
              <h4>Field Notes</h4>
              <p>
                This build focuses on crawl correctness, live streaming updates,
                audit coverage, exports, and keeping large crawls out of the
                frontend heap. The rest is getting bolted on one measurable signal
                at a time.
              </p>
            </section>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <Dialog.Root open={settingsOpen} onOpenChange={setSettingsOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="modal-backdrop" />
          <Dialog.Content className="settings-modal">
            <div className="modal-header">
              <Dialog.Title asChild>
                <h2 id="settings-title">Crawl Settings</h2>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button title="Close settings">
                  <X size={16} />
                </button>
              </Dialog.Close>
            </div>

            <section className="settings-section">
              <h3>Limits and Politeness</h3>
              <div className="settings-grid">
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
                  RPS
                  <input
                    type="number"
                    min={1}
                    value={config.requestsPerSecond}
                    onChange={(event) =>
                      setConfig({ requestsPerSecond: Number(event.target.value) })
                    }
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
                <label>
                  Dup bits
                  <input
                    type="number"
                    min={0}
                    max={64}
                    value={config.nearDuplicateThreshold}
                    onChange={(event) =>
                      setConfig({ nearDuplicateThreshold: Number(event.target.value) })
                    }
                  />
                </label>
                <CheckboxField
                  checked={config.respectRobots}
                  onCheckedChange={(checked) => setConfig({ respectRobots: checked })}
                >
                  Respect robots.txt
                </CheckboxField>
                <CheckboxField
                  checked={config.useRobotsTxtOverride}
                  disabled={!config.respectRobots}
                  onCheckedChange={(checked) =>
                    setConfig({ useRobotsTxtOverride: checked })
                  }
                >
                  Override robots.txt
                </CheckboxField>
                <label className="settings-wide">
                  robots.txt override
                  <textarea
                    rows={4}
                    disabled={!config.respectRobots || !config.useRobotsTxtOverride}
                    value={config.robotsTxtOverride}
                    onChange={(event) =>
                      setConfig({ robotsTxtOverride: event.target.value })
                    }
                  />
                </label>
                <label>
                  Robots test URL
                  <input
                    disabled={!config.respectRobots || !config.useRobotsTxtOverride}
                    value={robotsTestUrl}
                    placeholder="https://example.com/path"
                    onChange={(event) => {
                      setRobotsTestUrl(event.target.value);
                      setRobotsTestResult(undefined);
                    }}
                  />
                </label>
                <div className="settings-actions robots-test-actions">
                  <button
                    onClick={() => void downloadRobotsTxt()}
                    disabled={!desktopRuntime || !config.startUrl.trim()}
                  >
                    Download Robots
                  </button>
                  <button
                    onClick={() => void testRobotsTxt()}
                    disabled={
                      !desktopRuntime ||
                      !config.respectRobots ||
                      !config.useRobotsTxtOverride ||
                      !robotsTestUrl.trim() ||
                      !config.robotsTxtOverride.trim()
                    }
                  >
                    Test Robots
                  </button>
                  <span className="settings-result">{robotsTestResult ?? "No result"}</span>
                </div>
              </div>
            </section>

            <section className="settings-section">
              <h3>Scope</h3>
              <div className="settings-grid">
                <label className="settings-wide">
                  List URLs
                  <textarea
                    rows={4}
                    value={patternsToText(config.listUrls)}
                    onChange={(event) =>
                      setConfig({ listUrls: textToPatterns(event.target.value) })
                    }
                  />
                </label>
                <label className="settings-wide">
                  Include regex
                  <textarea
                    rows={3}
                    value={patternsToText(config.includeUrlPatterns)}
                    onChange={(event) =>
                      setConfig({ includeUrlPatterns: textToPatterns(event.target.value) })
                    }
                  />
                </label>
                <label className="settings-wide">
                  Exclude regex
                  <textarea
                    rows={3}
                    value={patternsToText(config.excludeUrlPatterns)}
                    onChange={(event) =>
                      setConfig({ excludeUrlPatterns: textToPatterns(event.target.value) })
                    }
                  />
                </label>
              </div>
            </section>

            <section className="settings-section">
              <h3>Resource Types</h3>
              <div className="settings-grid">
                <CheckboxField
                  checked={config.resourceTypes.html}
                  onCheckedChange={(checked) => updateResourceType("html", checked)}
                >
                  HTML pages
                </CheckboxField>
                <CheckboxField
                  checked={config.resourceTypes.images}
                  onCheckedChange={(checked) => updateResourceType("images", checked)}
                >
                  Images
                </CheckboxField>
                <CheckboxField
                  checked={config.resourceTypes.css}
                  onCheckedChange={(checked) => updateResourceType("css", checked)}
                >
                  CSS
                </CheckboxField>
                <CheckboxField
                  checked={config.resourceTypes.javascript}
                  onCheckedChange={(checked) => updateResourceType("javascript", checked)}
                >
                  JavaScript
                </CheckboxField>
                <CheckboxField
                  checked={config.resourceTypes.external}
                  onCheckedChange={(checked) => updateResourceType("external", checked)}
                >
                  External URLs
                </CheckboxField>
                <CheckboxField
                  checked={config.resourceTypes.other}
                  onCheckedChange={(checked) => updateResourceType("other", checked)}
                >
                  Other files
                </CheckboxField>
              </div>
            </section>

            <section className="settings-section">
              <h3>Query Strings</h3>
              <div className="settings-grid">
                <CheckboxField
                  checked={config.querySettings.sortParameters}
                  onCheckedChange={(checked) =>
                    updateQuerySettings({ sortParameters: checked })
                  }
                >
                  Sort parameters
                </CheckboxField>
                <CheckboxField
                  checked={config.querySettings.stripAll}
                  onCheckedChange={(checked) => updateQuerySettings({ stripAll: checked })}
                >
                  Strip all query
                </CheckboxField>
                <label>
                  Max parameters
                  <input
                    type="number"
                    min={0}
                    value={config.querySettings.maxParameters}
                    onChange={(event) =>
                      updateQuerySettings({ maxParameters: Number(event.target.value) })
                    }
                  />
                </label>
                <label className="settings-wide">
                  Strip parameter regex
                  <textarea
                    rows={3}
                    value={patternsToText(config.querySettings.stripParameterPatterns)}
                    onChange={(event) =>
                      updateQuerySettings({
                        stripParameterPatterns: textToPatterns(event.target.value),
                      })
                    }
                  />
                </label>
              </div>
            </section>

            <section className="settings-section">
              <h3>Storage</h3>
              <div className="settings-grid compact">
                <label>
                  Mode
                  <select
                    value={storageMode}
                    onChange={(event) => setStorageMode(event.target.value as StorageMode)}
                  >
                    <option value="memory">Memory</option>
                    <option value="database">Database</option>
                  </select>
                </label>
                <CheckboxField
                  checked={resumeCrawl}
                  disabled={storageMode !== "database"}
                  onCheckedChange={setResumeCrawl}
                >
                  Resume database
                </CheckboxField>
                <label>
                  Session
                  <select
                    value={selectedSessionId}
                    disabled={storageMode !== "database"}
                    onChange={(event) => void openSession(event.target.value)}
                  >
                    <option value="">Current database</option>
                    {crawlSessions.map((session) => (
                      <option key={session.id} value={session.id}>
                        {session.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  New session
                  <input
                    value={newSessionName}
                    disabled={storageMode !== "database"}
                    placeholder={sessionNameFromUrl(config.startUrl)}
                    onChange={(event) => setNewSessionName(event.target.value)}
                  />
                </label>
                <div className="settings-actions">
                  <button
                    onClick={() => void createSession()}
                    disabled={storageMode !== "database"}
                  >
                    Save Session
                  </button>
                  <button
                    onClick={() => void deleteSession()}
                    disabled={storageMode !== "database" || !selectedSessionId || running}
                  >
                    Delete Session
                  </button>
                </div>
              </div>
            </section>

            <section className="settings-section">
              <h3>Configuration Profiles</h3>
              <div className="settings-grid compact">
                <label>
                  Profile
                  <select
                    value={selectedProfileId}
                    disabled={running}
                    onChange={(event) => void loadProfile(event.target.value)}
                  >
                    <option value="">Select profile</option>
                    {configProfiles.map((profile) => (
                      <option key={profile.id} value={profile.id}>
                        {profile.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  New profile
                  <input
                    value={newProfileName}
                    disabled={running}
                    placeholder={`${sessionNameFromUrl(config.startUrl)} profile`}
                    onChange={(event) => setNewProfileName(event.target.value)}
                  />
                </label>
                <div className="settings-actions">
                  <button onClick={() => void saveProfile()} disabled={running}>
                    Save Profile
                  </button>
                  <button
                    onClick={() => void deleteProfile()}
                    disabled={!selectedProfileId || running}
                  >
                    Delete Profile
                  </button>
                </div>
              </div>
            </section>

            <section className="settings-section">
              <div className="section-heading">
                <h3>Custom Extraction</h3>
                <button onClick={addExtractor}>
                  <Plus size={16} />
                  <span>Add</span>
                </button>
              </div>
              <div className="extractor-list">
                {config.customExtractors.length === 0 ? (
                  <span className="extractor-empty">No custom extractors</span>
                ) : (
                  config.customExtractors.map((extractor, index) => (
                    <div className="extractor-row" key={`${extractor.name}-${index}`}>
                      <input
                        aria-label="Extractor name"
                        value={extractor.name}
                        onChange={(event) =>
                          updateExtractor(index, { name: event.target.value })
                        }
                        placeholder="Column name"
                      />
                      <select
                        aria-label="Extractor kind"
                        value={extractor.kind}
                        onChange={(event) =>
                          updateExtractor(index, {
                            kind: event.target.value as ExtractorKind,
                          })
                        }
                      >
                        {extractorKinds.map((kind) => (
                          <option key={kind.value} value={kind.value}>
                            {kind.label}
                          </option>
                        ))}
                      </select>
                      <input
                        aria-label="Extractor pattern"
                        value={extractor.pattern}
                        onChange={(event) =>
                          updateExtractor(index, { pattern: event.target.value })
                        }
                        placeholder="Selector, XPath, or regex"
                      />
                      <input
                        aria-label="Extractor attribute"
                        value={extractor.attribute ?? ""}
                        disabled={extractor.kind !== "cssAttribute"}
                        onChange={(event) =>
                          updateExtractor(index, {
                            attribute: event.target.value || null,
                          })
                        }
                        placeholder="Attribute"
                      />
                      <CheckboxField
                        checked={extractor.allMatches}
                        onCheckedChange={(checked) =>
                          updateExtractor(index, { allMatches: checked })
                        }
                      >
                        All
                      </CheckboxField>
                      <button
                        onClick={() => removeExtractor(index)}
                        title="Remove extractor"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  ))
                )}
              </div>
            </section>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <LinkReportsDialog
        open={linkReportsOpen}
        onOpenChange={setLinkReportsOpen}
        selectedReport={selectedLinkReport}
        onSelectedReportChange={setSelectedLinkReport}
        edges={linkEdges}
        edgeTotal={linkEdgeTotal}
        anchorRows={anchorTextRows}
        anchorTotal={anchorTextTotal}
        redirectRows={redirectReportRows}
        redirectTotal={redirectReportTotal}
        loading={linkReportLoading}
        selectedUrl={selectedUrl}
        searchValue={linkReportSearch}
        onSearchValueChange={setLinkReportSearch}
        sortBy={linkReportSortBy}
        sortDir={linkReportSortDir}
        onSortChange={(column) => {
          setLinkReportSortDir((currentDirection) =>
            linkReportSortBy === column && currentDirection === "asc" ? "desc" : "asc",
          );
          setLinkReportSortBy(column);
        }}
        anchorSortBy={anchorTextSortBy}
        anchorSortDir={anchorTextSortDir}
        onAnchorSortChange={(column) => {
          setAnchorTextSortDir((currentDirection) =>
            anchorTextSortBy === column && currentDirection === "asc" ? "desc" : "asc",
          );
          setAnchorTextSortBy(column);
        }}
        onRefresh={() => void loadLinkReport(selectedLinkReport)}
      />

      <GraphDialog
        open={graphOpen}
        onOpenChange={setGraphOpen}
        graph={graph}
        loading={graphLoading}
        theme={theme}
        live={running}
        updatedAt={graphUpdatedAt}
        internalOnly={graphInternalOnly}
        onInternalOnlyChange={setGraphInternalOnly}
        statusFilter={graphStatusFilter}
        onStatusFilterChange={setGraphStatusFilter}
        depthFilter={graphDepthFilter}
        onDepthFilterChange={setGraphDepthFilter}
        layoutMode={graphLayoutMode}
        onLayoutModeChange={setGraphLayoutMode}
        onRefresh={() => void loadGraph()}
      />

      <section className="metrics">
        <Metric label="Crawled" value={progress?.crawled ?? summary.total} />
        <Metric label="Queued" value={progress?.queued ?? 0} />
        <Metric label="Discovered" value={progress?.discovered ?? summary.total} />
        <Metric label="Speed" value={(progress?.pagesPerSecond ?? 0).toFixed(2)} />
        <Metric label="2xx" value={summary.success} />
        <Metric label="Broken" value={summary.broken} tone="danger" />
        <Metric label="Near Dupes" value={summary.nearDuplicates} />
      </section>

      {error ? <div className="error-bar">{error}</div> : null}

      <section
        className={overviewResizing ? "workspace resizing" : "workspace"}
        style={{ "--overview-width": `${overviewWidth}px` } as CSSProperties}
      >
        <section className="results-pane">
          <div className="issue-tabs-shell">
            <button
              className="tab-scroll-button"
              onClick={() => scrollTabs(-1)}
              title="Scroll issue tabs left"
            >
              <ChevronLeft size={16} />
            </button>
            <nav className="issue-tabs" ref={tabsRef} aria-label="Issue views">
              {views.map((view) => (
                <button
                  key={view.id}
                  className={selectedView === view.id ? "active" : ""}
                  onClick={() => setView(view.id)}
                >
                  {view.label}
                </button>
              ))}
            </nav>
            <button
              className="tab-scroll-button"
              onClick={() => scrollTabs(1)}
              title="Scroll issue tabs right"
            >
              <ChevronRight size={16} />
            </button>
          </div>
          <div className="grid-status">
            <div className="grid-status-left">
              <span>{total.toLocaleString()} rows</span>
              <span>{progress?.status ?? "idle"}</span>
            </div>
            <div className="search-control grid-search">
              <Search size={16} />
              <input
                aria-label="Search results"
                value={globalSearch}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="Search current view"
              />
            </div>
          </div>
          <div className="grid" ref={parentRef}>
            <table
              className="data-table"
              style={{
                minWidth: gridWidth,
                width: gridWidth,
              }}
            >
              <colgroup>
                {columns.map((column) => (
                  <col
                    key={columnKey(column)}
                    style={{
                      width: column.width,
                      minWidth: column.width,
                      maxWidth: column.width,
                    }}
                  />
                ))}
              </colgroup>
              <thead>
                {table.getHeaderGroups().map((headerGroup) => (
                  <tr key={headerGroup.id}>
                    {headerGroup.headers.map((header) => (
                      <th
                        key={header.id}
                        style={{
                          width: header.getSize(),
                          minWidth: header.getSize(),
                          maxWidth: header.getSize(),
                        }}
                      >
                        <button
                          onClick={() => {
                            if (header.column.getCanSort()) {
                              setSort(header.column.id);
                            }
                          }}
                          disabled={!header.column.getCanSort()}
                        >
                          {flexRender(
                            header.column.columnDef.header,
                            header.getContext(),
                          )}
                          {sortBy === header.column.id
                            ? sortDir === "asc"
                              ? " ^"
                              : " v"
                            : ""}
                        </button>
                      </th>
                    ))}
                  </tr>
                ))}
              </thead>
              <tbody>
                {virtualPaddingTop > 0 ? (
                  <tr className="virtual-spacer" style={{ height: virtualPaddingTop }}>
                    <td colSpan={columns.length} />
                  </tr>
                ) : null}
                {virtualRows.map((virtualRow) => {
                  const tableRow = tableRows[virtualRow.index];
                  const row = tableRow.original;
                  return (
                    <tr
                      key={tableRow.id}
                      className={selected?.id === row.id ? "selected" : ""}
                      onClick={() => setSelected(row)}
                    >
                      {tableRow.getVisibleCells().map((cell) => (
                        <td
                          key={cell.id}
                          style={{
                            width: cell.column.getSize(),
                            minWidth: cell.column.getSize(),
                            maxWidth: cell.column.getSize(),
                          }}
                        >
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      ))}
                    </tr>
                  );
                })}
                {virtualPaddingBottom > 0 ? (
                  <tr
                    className="virtual-spacer"
                    style={{ height: virtualPaddingBottom }}
                  >
                    <td colSpan={columns.length} />
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>

          {selected ? (
            <aside className="detail-panel">
              <div className="detail-header">
                <div>
                  <h2>{selected.statusCode ?? "No response"} {selected.statusText}</h2>
                  <p className="detail-url">{selected.finalUrl}</p>
                </div>
                <button onClick={() => setSelected(undefined)} title="Close details">
                  <X size={16} />
                </button>
              </div>
              <dl>
                <dt>Title</dt>
                <dd>{selected.title || "Missing"}</dd>
                <dt>Meta Description</dt>
                <dd>{selected.metaDescription || "Missing"}</dd>
                <dt>H1</dt>
                <dd>
                  {selected.h1 || "Missing"}{" "}
                  <span className="detail-muted">({selected.h1Count})</span>
                </dd>
                <dt>H2</dt>
                <dd>
                  {selected.h2 || "Missing"}{" "}
                  <span className="detail-muted">({selected.h2Count})</span>
                </dd>
                <dt>Canonical</dt>
                <dd>
                  {selected.canonical || "Missing"}{" "}
                  <span className="detail-muted">({selected.canonicalCount})</span>
                </dd>
                <dt>Directives</dt>
                <dd>
                  Meta robots: {selected.metaRobots || "None"}; X-Robots-Tag:{" "}
                  {selected.xRobotsTag || "None"}
                </dd>
                <dt>Indexability</dt>
                <dd>{selected.indexabilityStatus}</dd>
                <dt>Images</dt>
                <dd>
                  {selected.imageCount.toLocaleString()} images,{" "}
                  {selected.imagesMissingAlt.toLocaleString()} missing alt,{" "}
                  {selected.imagesAltTooLong.toLocaleString()} long alt
                </dd>
                <dt>Security</dt>
                <dd>
                  {selected.mixedContentCount.toLocaleString()} mixed-content references,{" "}
                  {selected.insecureFormCount.toLocaleString()} insecure forms; HSTS{" "}
                  {flagLabel(selected.hstsHeader)}, CSP{" "}
                  {flagLabel(selected.contentSecurityPolicyHeader)}, XFO{" "}
                  {flagLabel(selected.xFrameOptionsHeader)}, XCTO{" "}
                  {flagLabel(selected.xContentTypeOptionsHeader)}
                </dd>
                <dt>Mobile</dt>
                <dd>Viewport {flagLabel(selected.viewport)}</dd>
                <dt>Pagination</dt>
                <dd>
                  Next: {selected.relNext || "None"}; Prev: {selected.relPrev || "None"}
                </dd>
                <dt>AMP</dt>
                <dd>{selected.amphtml || "None"}</dd>
                <dt>Hreflang</dt>
                <dd>
                  {selected.hreflangCount.toLocaleString()} alternates,{" "}
                  {selected.hreflangInvalidCount.toLocaleString()} invalid, self-reference{" "}
                  {selected.hreflangMissingSelfReference ? "missing" : "ok"}
                </dd>
                <dt>Structured Data</dt>
                <dd>
                  {selected.jsonLdCount.toLocaleString()} JSON-LD blocks,{" "}
                  {selected.jsonLdInvalidCount.toLocaleString()} invalid
                </dd>
                <dt>Social</dt>
                <dd>
                  {selected.openGraphCount.toLocaleString()} Open Graph tags,{" "}
                  {selected.twitterCardCount.toLocaleString()} Twitter tags
                </dd>
                <dt>Redirects</dt>
                <dd>
                  {selected.redirectChain.length > 0 ? (
                    <ol className="redirect-chain">
                      {selected.redirectChain.map((hop, index) => (
                        <li key={`${hop.url}-${index}`}>
                          <span>{hop.statusCode}</span>
                          <span>
                            {hop.url} ({formatMs(hop.ttfbMs ?? hop.elapsedMs)})
                          </span>
                          {hop.location ? <span>{hop.location}</span> : null}
                        </li>
                      ))}
                    </ol>
                  ) : (
                    "None"
                  )}
                </dd>
                <dt>Links</dt>
                <dd>
                  {selected.internalOutlinkCount} internal,{" "}
                  {selected.externalOutlinkCount} external
                </dd>
                <dt>Content</dt>
                <dd>
                  {selected.wordCount.toLocaleString()} words,{" "}
                  {(selected.textToCodeRatio * 100).toFixed(1)}% text/code
                </dd>
                <dt>Network</dt>
                <dd>
                  <NetworkTimingBar record={selected} />
                </dd>
                <dt>Response Hash</dt>
                <dd>{selected.responseHash || "None"}</dd>
                <dt>SimHash</dt>
                <dd>
                  {selected.simhash !== null && selected.simhash !== undefined
                    ? String(selected.simhash)
                    : "None"}
                </dd>
                <dt>Near-Duplicate Cluster</dt>
                <dd>{selected.nearDuplicateClusterId ?? "None"}</dd>
                <dt>Custom Extractions</dt>
                <dd>
                  {selected.customExtractions.length > 0 ? (
                    <ul className="extraction-values">
                      {selected.customExtractions.map((extraction) => (
                        <li key={extraction.name}>
                          <strong>{extraction.name}</strong>
                          <span>{extraction.values.join(", ") || "No value"}</span>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    "None"
                  )}
                </dd>
                <dt>Error</dt>
                <dd>{selected.error || "None"}</dd>
              </dl>
            </aside>
          ) : null}
        </section>
        <div
          className="overview-resizer"
          role="separator"
          aria-label="Resize overview panel"
          aria-orientation="vertical"
          aria-valuemin={overviewMinWidth}
          aria-valuemax={overviewMaxWidth}
          aria-valuenow={overviewWidth}
          tabIndex={0}
          onPointerDown={startOverviewResize}
          onPointerMove={moveOverviewResize}
          onPointerUp={stopOverviewResize}
          onPointerCancel={stopOverviewResize}
          onKeyDown={handleOverviewResizeKey}
        />
        <OverviewPanel
          summary={summary}
          progress={progress}
          progressPercent={progressPercent}
          progressHistory={progressHistory}
          onViewSelect={setView}
        />
      </section>
      <footer className="status-bar">
        <div className="status-main">
          <span
            className={
              paused ? "status-dot paused" : running ? "status-dot active" : "status-dot"
            }
          />
          <strong>{crawlStateLabel}</strong>
          <span>{crawlTargetLabel}</span>
        </div>
        <div
          className="status-progress"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progressPercent}
        >
          <span style={{ width: `${progressPercent}%` }} />
        </div>
        <div className="status-stats">
          <span>{progressCrawled.toLocaleString()} crawled</span>
          <span>{progressDiscovered.toLocaleString()} discovered</span>
          <span>{(progress?.queued ?? 0).toLocaleString()} queued</span>
          <span>{(progress?.pagesPerSecond ?? 0).toFixed(2)} URL/s</span>
        </div>
      </footer>
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

function LinkReportsDialog({
  open,
  onOpenChange,
  selectedReport,
  onSelectedReportChange,
  edges,
  edgeTotal,
  anchorRows,
  anchorTotal,
  redirectRows,
  redirectTotal,
  loading,
  selectedUrl,
  searchValue,
  onSearchValueChange,
  sortBy,
  sortDir,
  onSortChange,
  anchorSortBy,
  anchorSortDir,
  onAnchorSortChange,
  onRefresh,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  selectedReport: LinkReportKind;
  onSelectedReportChange: (report: LinkReportKind) => void;
  edges: LinkEdge[];
  edgeTotal: number;
  anchorRows: AnchorTextRow[];
  anchorTotal: number;
  redirectRows: CrawlRecord[];
  redirectTotal: number;
  loading: boolean;
  selectedUrl?: string;
  searchValue: string;
  onSearchValueChange: (value: string) => void;
  sortBy: string;
  sortDir: SortDirection;
  onSortChange: (column: string) => void;
  anchorSortBy: string;
  anchorSortDir: SortDirection;
  onAnchorSortChange: (column: string) => void;
  onRefresh: () => void;
}) {
  const isRedirectReport = selectedReport === "redirects";
  const isAnchorReport = selectedReport === "anchorText";
  const total = isRedirectReport ? redirectTotal : isAnchorReport ? anchorTotal : edgeTotal;
  const visible = isRedirectReport
    ? redirectRows.length
    : isAnchorReport
      ? anchorRows.length
      : edges.length;
  const selectedReportNeedsUrl =
    selectedReport === "selectedInlinks" || selectedReport === "selectedOutlinks";

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-backdrop" />
        <Dialog.Content className="link-report-modal">
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>Link Reports</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button title="Close link reports">
                <X size={16} />
              </button>
            </Dialog.Close>
          </div>

          <div className="link-report-tabs" role="tablist" aria-label="Link report views">
            {linkReportViews.map((view) => (
              <button
                key={view.id}
                className={selectedReport === view.id ? "active" : ""}
                onClick={() => onSelectedReportChange(view.id)}
                disabled={
                  (view.id === "selectedInlinks" || view.id === "selectedOutlinks") &&
                  !selectedUrl
                }
              >
                {view.label}
              </button>
            ))}
          </div>

          <div className="link-report-status">
            <div>
              <strong>{loading ? "Loading" : `${total.toLocaleString()} rows`}</strong>
              <span>
                Showing {visible.toLocaleString()} rows
                {selectedReportNeedsUrl && selectedUrl ? ` for ${selectedUrl}` : ""}
              </span>
            </div>
            <button onClick={onRefresh} disabled={loading}>
              Refresh
            </button>
          </div>

          <div className="link-report-controls">
            <label>
              <Search size={14} />
              <input
                value={searchValue}
                onChange={(event) => onSearchValueChange(event.target.value)}
                placeholder="Search link reports"
              />
            </label>
          </div>

          {selectedReportNeedsUrl && !selectedUrl ? (
            <p className="link-report-empty">
              Select a URL row first to inspect inlinks or outlinks for that URL.
            </p>
          ) : isRedirectReport ? (
            <RedirectReportTable rows={redirectRows} />
          ) : isAnchorReport ? (
            <AnchorTextReportTable
              rows={anchorRows}
              sortBy={anchorSortBy}
              sortDir={anchorSortDir}
              onSortChange={onAnchorSortChange}
            />
          ) : (
            <LinkEdgeReportTable
              edges={edges}
              sortBy={sortBy}
              sortDir={sortDir}
              onSortChange={onSortChange}
            />
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function AnchorTextReportTable({
  rows,
  sortBy,
  sortDir,
  onSortChange,
}: {
  rows: AnchorTextRow[];
  sortBy: string;
  sortDir: SortDirection;
  onSortChange: (column: string) => void;
}) {
  if (rows.length === 0) {
    return <p className="link-report-empty">No anchor text groups match this report.</p>;
  }

  const columns = [
    { key: "anchorText", label: "Anchor Text" },
    { key: "targetUrl", label: "Target" },
    { key: "linkType", label: "Type" },
    { key: "linkCount", label: "Links" },
    { key: "sourceCount", label: "Sources" },
    { key: "nofollowCount", label: "Nofollow" },
    { key: "firstSourceUrl", label: "First Source" },
    { key: "targetStatusCode", label: "Target Status" },
  ];

  return (
    <div className="link-report-table-wrap">
      <table className="link-report-table anchor-text-table">
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column.key}>
                <button onClick={() => onSortChange(column.key)}>
                  <span>{column.label}</span>
                  {sortBy === column.key ? (
                    <span className="sort-indicator" aria-hidden="true">
                      {sortDir.toUpperCase()}
                    </span>
                  ) : null}
                </button>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={`${row.anchorText}:${row.targetUrl}:${row.linkType}`}>
              <td>{row.anchorText || "No anchor text"}</td>
              <td>{row.targetUrl}</td>
              <td>{linkTypeLabel(row.linkType)}</td>
              <td>{row.linkCount.toLocaleString()}</td>
              <td>{row.sourceCount.toLocaleString()}</td>
              <td>{row.nofollowCount.toLocaleString()}</td>
              <td>{row.firstSourceUrl}</td>
              <td>{statusCell(row.targetStatusCode)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function LinkEdgeReportTable({
  edges,
  sortBy,
  sortDir,
  onSortChange,
}: {
  edges: LinkEdge[];
  sortBy: string;
  sortDir: SortDirection;
  onSortChange: (column: string) => void;
}) {
  if (edges.length === 0) {
    return <p className="link-report-empty">No link edges match this report.</p>;
  }

  const columns = [
    { key: "sourcePosition", label: "Position" },
    { key: "sourceUrl", label: "Source" },
    { key: "targetUrl", label: "Target" },
    { key: "anchorText", label: "Anchor" },
    { key: "linkType", label: "Type" },
    { key: "rel", label: "Rel" },
    { key: "sourceStatusCode", label: "Source Status" },
    { key: "targetStatusCode", label: "Target Status" },
    { key: "sourceDepth", label: "Depth" },
    { key: "discoveryOrder", label: "Discovery" },
  ];

  return (
    <div className="link-report-table-wrap">
      <table className="link-report-table">
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column.key}>
                <button onClick={() => onSortChange(column.key)}>
                  <span>{column.label}</span>
                  {sortBy === column.key ? (
                    <span className="sort-indicator" aria-hidden="true">
                      {sortDir.toUpperCase()}
                    </span>
                  ) : null}
                </button>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {edges.map((edge) => (
            <tr key={edge.id}>
              <td>{edge.sourcePosition}</td>
              <td>{edge.sourceUrl}</td>
              <td>{edge.targetUrl}</td>
              <td>{edge.anchorText || "No anchor text"}</td>
              <td>{linkTypeLabel(edge.linkType)}</td>
              <td>{edge.rel || (edge.relNofollow ? "nofollow" : "None")}</td>
              <td>{statusCell(edge.sourceStatusCode)}</td>
              <td>{statusCell(edge.targetStatusCode)}</td>
              <td>
                {edge.sourceDepth}
                {edge.targetDepth !== null && edge.targetDepth !== undefined
                  ? ` -> ${edge.targetDepth}`
                  : ""}
              </td>
              <td>{edge.discoveryOrder}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function RedirectReportTable({ rows }: { rows: CrawlRecord[] }) {
  if (rows.length === 0) {
    return <p className="link-report-empty">No redirect chains match this report.</p>;
  }

  return (
    <div className="link-report-table-wrap">
      <table className="link-report-table">
        <thead>
          <tr>
            <th>Source URL</th>
            <th>Final URL</th>
            <th>Hops</th>
            <th>Last Target</th>
            <th>Status</th>
            <th>Time</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const lastHop = row.redirectChain[row.redirectChain.length - 1];
            return (
              <tr key={row.id}>
                <td>{row.url}</td>
                <td>{row.finalUrl}</td>
                <td>{row.redirectChain.length}</td>
                <td>{lastHop?.location ?? row.redirectTarget ?? "None"}</td>
                <td>{statusCell(row.statusCode)}</td>
                <td>{formatMs(row.responseTimeMs)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function GraphDialog({
  open,
  onOpenChange,
  graph,
  loading,
  theme,
  live,
  updatedAt,
  internalOnly,
  onInternalOnlyChange,
  statusFilter,
  onStatusFilterChange,
  depthFilter,
  onDepthFilterChange,
  layoutMode,
  onLayoutModeChange,
  onRefresh,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  graph?: CrawlGraph;
  loading: boolean;
  theme: Theme;
  live: boolean;
  updatedAt?: number;
  internalOnly: boolean;
  onInternalOnlyChange: (value: boolean) => void;
  statusFilter: GraphStatusFilter;
  onStatusFilterChange: (value: GraphStatusFilter) => void;
  depthFilter: string;
  onDepthFilterChange: (value: string) => void;
  layoutMode: GraphLayoutMode;
  onLayoutModeChange: (value: GraphLayoutMode) => void;
  onRefresh: () => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<Sigma<GraphNodeAttributes, GraphEdgeAttributes> | null>(
    null,
  );
  const graphRef = useRef<
    MultiDirectedGraph<GraphNodeAttributes, GraphEdgeAttributes> | null
  >(null);
  const [hoveredNode, setHoveredNode] = useState<GraphNodeAttributes | null>(null);
  const [selectedNode, setSelectedNode] = useState<GraphNodeAttributes | null>(null);
  const [graphRenderMode, setGraphRenderMode] = useState<"webgl" | "svg">("svg");
  const visibleGraph = useMemo(
    () => filterGraphSnapshot(graph, statusFilter, depthFilter),
    [depthFilter, graph, statusFilter],
  );
  const graphReady = Boolean(visibleGraph && visibleGraph.nodes.length > 0);
  const previewNode = selectedNode ?? hoveredNode;
  const depthOptions = useMemo(() => graphDepthOptions(graph), [graph]);

  useEffect(() => {
    if (open) {
      setGraphRenderMode("svg");
    }
  }, [open]);

  useEffect(() => {
    if (!open || !containerRef.current || graphRenderMode !== "webgl" || !graphReady) {
      return;
    }
    if (!supportsWebGl()) {
      setGraphRenderMode("svg");
      return;
    }

    const graphology = new MultiDirectedGraph<
      GraphNodeAttributes,
      GraphEdgeAttributes
    >();
    const colors = graphPalette();
    let renderer: Sigma<GraphNodeAttributes, GraphEdgeAttributes>;
    try {
      renderer = new Sigma<GraphNodeAttributes, GraphEdgeAttributes>(
        graphology,
        containerRef.current,
        {
          allowInvalidContainer: true,
          defaultEdgeColor: colors.border,
          defaultEdgeType: "arrow",
          defaultNodeColor: colors.primary,
          edgeProgramClasses: {
            arrow: EdgeArrowProgram as unknown as EdgeProgramType<
              GraphNodeAttributes,
              GraphEdgeAttributes
            >,
          },
          hideEdgesOnMove: true,
          hideLabelsOnMove: true,
          labelColor: { color: colors.text },
          labelDensity: 0.08,
          labelFont:
            'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
          labelRenderedSizeThreshold: 8,
          labelSize: 11,
          minEdgeThickness: 0.5,
          renderEdgeLabels: false,
          renderLabels: true,
          stagePadding: 28,
          zIndex: true,
        },
      );
    } catch {
      graphology.clear();
      setGraphRenderMode("svg");
      return;
    }

    graphRef.current = graphology;
    rendererRef.current = renderer;
    renderer.on("enterNode", ({ node }) => {
      setHoveredNode(graphology.getNodeAttributes(node));
    });
    renderer.on("leaveNode", () => {
      setHoveredNode(null);
    });
    renderer.on("clickNode", ({ node }) => {
      setSelectedNode(graphology.getNodeAttributes(node));
    });
    renderer.on("clickStage", () => {
      setSelectedNode(null);
    });

    return () => {
      renderer.kill();
      graphology.clear();
      rendererRef.current = null;
      graphRef.current = null;
      setHoveredNode(null);
    };
  }, [graphReady, graphRenderMode, open, theme]);

  useEffect(() => {
    setSelectedNode(null);
  }, [depthFilter, statusFilter, graph]);

  useEffect(() => {
    const graphology = graphRef.current;
    const renderer = rendererRef.current;
    if (
      !open ||
      graphRenderMode !== "webgl" ||
      !graphReady ||
      !graphology ||
      !renderer ||
      !visibleGraph
    ) {
      return;
    }

    const colors = graphPalette();
    const previousPositions = new Map<string, { x: number; y: number }>();
    graphology.forEachNode((node, attributes) => {
      previousPositions.set(node, { x: attributes.x, y: attributes.y });
    });

    graphology.clear();
    const radius = Math.max(6, visibleGraph.nodes.length / 6);
    visibleGraph.nodes.forEach((node, index) => {
      const previous = previousPositions.get(node.url);
      const position = graphInitialPosition(
        node,
        index,
        visibleGraph.nodes,
        layoutMode,
        radius,
      );
      graphology.addNode(node.url, {
        label: node.label || graphLabel(node.url),
        url: node.url,
        status: graphStatusLabel(node),
        depth:
          node.depth === null || node.depth === undefined ? "Unknown" : String(node.depth),
        inlinks: node.inlinkCount,
        outlinks: node.outlinkCount,
        x: previous?.x ?? position.x,
        y: previous?.y ?? position.y,
        size: graphNodeSize(node),
        color: graphNodeColor(node, colors),
        forceLabel: graphForceLabel(node, visibleGraph.nodes.length),
      });
    });

    const nodeIds = new Set(visibleGraph.nodes.map((node) => node.url));
    visibleGraph.edges
      .filter((edge) => nodeIds.has(edge.sourceUrl) && nodeIds.has(edge.targetUrl))
      .forEach((edge) => {
        graphology.addDirectedEdgeWithKey(
          `edge-${edge.id}`,
          edge.sourceUrl,
          edge.targetUrl,
          {
            label: edge.anchorText || "",
            status: graphEdgeStatusLabel(edge),
            size: graphEdgeSize(edge),
            color: graphEdgeColor(edge, colors),
            type: "arrow",
          },
        );
      });

    if (graphology.order > 0) {
      if (graphology.order > 1 && graphology.size > 0 && previousPositions.size > 0) {
        forceAtlas2.assign(graphology, {
          iterations: 35,
          settings: {
            ...forceAtlas2.inferSettings(graphology),
            barnesHutOptimize: graphology.order > 120,
            gravity: 0.6,
            scalingRatio: graphology.order > 300 ? 28 : 18,
            slowDown: 2,
          },
        });
      }
    }

    renderer.setSetting("labelColor", { color: colors.text });
    renderer.setSetting("defaultNodeColor", colors.primary);
    renderer.setSetting("defaultEdgeColor", colors.border);
    renderer.resize();
    renderer.refresh({ skipIndexation: false });
    if (previousPositions.size === 0) {
      requestAnimationFrame(() => {
        void renderer.getCamera().animatedReset({ duration: 180 });
      });
    }
  }, [graphReady, graphRenderMode, layoutMode, open, theme, visibleGraph]);

  const fitGraph = () => {
    void rendererRef.current?.getCamera().animatedReset({ duration: 220 });
  };

  const toggleGraphRenderMode = () => {
    setGraphRenderMode((mode) => (mode === "webgl" ? "svg" : "webgl"));
  };

  const exportVisibleGraph = () => {
    if (!visibleGraph) {
      return;
    }
    const blob = new Blob([JSON.stringify(visibleGraph, null, 2)], {
      type: "application/json;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "ferrous-frog-graph-filtered.json";
    anchor.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-backdrop" />
        <Dialog.Content className="graph-modal">
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>Crawl Graph</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button title="Close graph">
                <X size={16} />
              </button>
            </Dialog.Close>
          </div>
          <div className="graph-toolbar">
            <div>
              <strong>
                {loading
                  ? "Loading graph"
                  : `${(visibleGraph?.nodes.length ?? 0).toLocaleString()} nodes / ${(visibleGraph?.edges.length ?? 0).toLocaleString()} edges`}
              </strong>
              <span>
                {live ? "Live updates are enabled while crawling." : "Graph is idle."}
                {updatedAt ? ` Updated ${formatTime(updatedAt)}.` : ""}
                {graph
                  ? ` Source snapshot ${(graph.nodes.length).toLocaleString()} nodes / ${(graph.edges.length).toLocaleString()} edges.`
                  : ""}
              </span>
            </div>
            <div className="graph-actions">
              <span className={live ? "graph-live on" : "graph-live"}>{live ? "Live" : "Idle"}</span>
              <button onClick={fitGraph} disabled={!visibleGraph || visibleGraph.nodes.length === 0}>
                Fit
              </button>
              <button
                onClick={toggleGraphRenderMode}
                disabled={!visibleGraph || visibleGraph.nodes.length === 0}
              >
                {graphRenderMode === "webgl" ? "Use SVG" : "Use WebGL"}
              </button>
              <button
                onClick={exportVisibleGraph}
                disabled={!visibleGraph || visibleGraph.nodes.length === 0}
              >
                Export JSON
              </button>
              <button onClick={onRefresh} disabled={loading}>
                Reload
              </button>
            </div>
          </div>
          <div className="graph-filters">
            <button
              className={internalOnly ? "active" : ""}
              onClick={() => onInternalOnlyChange(!internalOnly)}
            >
              Internal Only
            </button>
            <label>
              Status
              <select
                value={statusFilter}
                onChange={(event) =>
                  onStatusFilterChange(event.target.value as GraphStatusFilter)
                }
              >
                <option value="all">All</option>
                <option value="success">2xx</option>
                <option value="redirect">Redirect</option>
                <option value="broken">Broken</option>
                <option value="external">External</option>
                <option value="uncrawled">Uncrawled</option>
              </select>
            </label>
            <label>
              Depth
              <select
                value={depthFilter}
                onChange={(event) => onDepthFilterChange(event.target.value)}
              >
                <option value="all">All</option>
                {depthOptions.map((depth) => (
                  <option key={depth} value={String(depth)}>
                    {depth}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Layout
              <select
                value={layoutMode}
                onChange={(event) =>
                  onLayoutModeChange(event.target.value as GraphLayoutMode)
                }
              >
                <option value="depth">Depth</option>
                <option value="radial">Radial</option>
              </select>
            </label>
          </div>
          <div className="graph-legend" aria-label="Graph legend">
            <span className="success">2xx</span>
            <span className="warning">Redirect</span>
            <span className="danger">Broken</span>
            <span className="muted">External</span>
            <span className="outline">Uncrawled</span>
          </div>
          <div className={previewNode ? "graph-node-preview" : "graph-node-preview empty"}>
            {previewNode ? (
              <>
                <strong>{previewNode.label}</strong>
                <span>{selectedNode ? "Selected" : "Hovered"}</span>
                <span>{previewNode.status}</span>
                <span>Depth {previewNode.depth}</span>
                <span>
                  {previewNode.inlinks.toLocaleString()} in /{" "}
                  {previewNode.outlinks.toLocaleString()} out
                </span>
                {selectedNode ? (
                  <button onClick={() => setSelectedNode(null)}>Clear</button>
                ) : null}
              </>
            ) : (
              <span>Hover or select a node for details.</span>
            )}
          </div>
          <div className="graph-canvas-shell">
            <div
              ref={containerRef}
              className={
                graphRenderMode === "webgl" ? "graph-canvas" : "graph-canvas is-hidden"
              }
            />
            {visibleGraph && visibleGraph.nodes.length > 0 && graphRenderMode === "svg" ? (
              <SvgGraph
                graph={visibleGraph}
                layoutMode={layoutMode}
                onNodeSelect={setSelectedNode}
              />
            ) : null}
            {!visibleGraph || visibleGraph.nodes.length === 0 ? (
              <p className="graph-empty-overlay">
                {loading ? "Loading graph data" : "No graph data is available yet."}
              </p>
            ) : null}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function SvgGraph({
  graph,
  layoutMode,
  onNodeSelect,
}: {
  graph: CrawlGraph;
  layoutMode: GraphLayoutMode;
  onNodeSelect: (node: GraphNodeAttributes) => void;
}) {
  const colors = graphPalette();
  const layout = useMemo(() => svgGraphLayout(graph, layoutMode), [graph, layoutMode]);

  return (
    <svg
      className="graph-svg"
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      role="img"
      aria-label="Crawl graph fallback rendering"
    >
      <rect width={layout.width} height={layout.height} fill={colors.surface} />
      <g className="graph-svg-edges">
        {layout.edges.map((edge) => (
          <line
            key={edge.id}
            x1={edge.source.x}
            y1={edge.source.y}
            x2={edge.target.x}
            y2={edge.target.y}
            stroke={edge.color}
            strokeOpacity={edge.opacity}
            strokeWidth={edge.width}
          />
        ))}
      </g>
      <g className="graph-svg-nodes">
        {layout.nodes.map((node) => (
          <g
            key={node.url}
            className="graph-svg-node"
            onClick={() => onNodeSelect(graphNodePreviewAttributes(node))}
          >
            <circle
              cx={node.x}
              cy={node.y}
              r={node.radius}
              fill={node.color}
              stroke={colors.surface}
              strokeWidth="2"
            >
              <title>{node.url}</title>
            </circle>
            {node.forceLabel ? (
              <text
                x={node.x + node.radius + 5}
                y={node.y + 4}
                fill={colors.text}
                fontSize="11"
              >
                {node.label}
              </text>
            ) : null}
          </g>
        ))}
      </g>
    </svg>
  );
}

function OverviewPanel({
  summary,
  progress,
  progressPercent,
  progressHistory,
  onViewSelect,
}: {
  summary: CrawlSummary;
  progress?: CrawlProgress;
  progressPercent: number;
  progressHistory: ProgressSample[];
  onViewSelect: (view: IssueView) => void;
}) {
  const statusSegments: OverviewStatusSegment[] = [
    { label: "2xx", value: summary.success, className: "success", view: "status2xx" },
    { label: "3xx", value: summary.redirects, className: "redirect", view: "status3xx" },
    { label: "4xx", value: summary.clientErrors, className: "warning", view: "status4xx" },
    { label: "5xx", value: summary.serverErrors, className: "danger", view: "status5xx" },
    { label: "No response", value: summary.noResponse, className: "muted", view: "noResponse" },
  ];
  const issueRows: OverviewRowModel[] = [
    { label: "Broken", value: summary.broken, tone: "danger" as const, view: "brokenLinks" },
    {
      label: "Near duplicates",
      value: summary.nearDuplicates,
      tone: "warning" as const,
      view: "nearDuplicate",
    },
    { label: "External URLs", value: summary.external, tone: "muted" as const, view: "external" },
  ];
  const urlRows: OverviewRowModel[] = [
    { label: "Internal", value: summary.internal, tone: "success" as const, view: "internal" },
    { label: "External", value: summary.external, tone: "muted" as const, view: "external" },
    { label: "No response", value: summary.noResponse, tone: "danger" as const, view: "noResponse" },
  ];
  const metadataRows: OverviewRowModel[] = [
    { label: "Missing titles", value: summary.titleMissing, tone: "warning" as const, view: "titleMissing" },
    { label: "Duplicate titles", value: summary.titleDuplicate, tone: "warning" as const, view: "titleDuplicate" },
    { label: "Missing meta", value: summary.metaMissing, tone: "warning" as const, view: "metaMissing" },
    { label: "Duplicate meta", value: summary.metaDuplicate, tone: "warning" as const, view: "metaDuplicate" },
  ];
  const headingRows: OverviewRowModel[] = [
    { label: "Missing H1", value: summary.h1Missing, tone: "warning" as const, view: "h1Missing" },
    { label: "Duplicate H1", value: summary.h1Duplicate, tone: "warning" as const, view: "h1Duplicate" },
    { label: "Missing H2", value: summary.h2Missing, tone: "muted" as const, view: "h2Missing" },
    { label: "Duplicate H2", value: summary.h2Duplicate, tone: "muted" as const, view: "h2Duplicate" },
  ];
  const directiveRows: OverviewRowModel[] = [
    { label: "Missing canonical", value: summary.canonicalMissing, tone: "muted" as const, view: "canonicalMissing" },
    { label: "Multiple canonicals", value: summary.canonicalMultiple, tone: "warning" as const, view: "canonicalMultiple" },
    { label: "Noindex", value: summary.noindex, tone: "muted" as const, view: "directivesNoindex" },
  ];
  const mediaRows: OverviewRowModel[] = [
    { label: "Images missing alt", value: summary.imagesMissingAlt, tone: "warning" as const, view: "imagesMissingAlt" },
    { label: "Long alt text", value: summary.imagesAltTooLong, tone: "muted" as const, view: "imagesAltTooLong" },
  ];
  const technicalRows: OverviewRowModel[] = [
    { label: "Invalid hreflang", value: summary.hreflangInvalid, tone: "warning" as const, view: "hreflangInvalid" },
    {
      label: "Invalid JSON-LD",
      value: summary.structuredDataInvalid,
      tone: "warning" as const,
      view: "structuredDataInvalid",
    },
    { label: "Mixed content", value: summary.mixedContent, tone: "danger" as const, view: "securityMixedContent" },
    { label: "Insecure forms", value: summary.insecureForms, tone: "danger" as const, view: "securityInsecureForms" },
    { label: "Sitemap orphans", value: summary.sitemapOrphans, tone: "warning" as const, view: "sitemapOrphan" },
    { label: "Missing viewport", value: summary.missingViewport, tone: "warning" as const, view: "mobileMissingViewport" },
    { label: "Missing HSTS", value: summary.missingHsts, tone: "muted" as const, view: "securityMissingHsts" },
  ];
  const unknownIndexability = Math.max(
    0,
    summary.total - summary.indexable - summary.nonIndexable,
  );

  return (
    <aside className="overview-panel" aria-label="Crawl overview">
      <div className="overview-header">
        <div>
          <h2>Overview</h2>
          <p>{progress?.status ?? "idle"}</p>
        </div>
        <strong>{progressPercent}%</strong>
      </div>

      <section className="overview-section">
        <div className="overview-progress">
          <span style={{ width: `${progressPercent}%` }} />
        </div>
        <div className="overview-kpis">
          <OverviewKpi label="Crawled" value={progress?.crawled ?? summary.total} />
          <OverviewKpi label="Queued" value={progress?.queued ?? 0} />
          <OverviewKpi label="Discovered" value={progress?.discovered ?? summary.total} />
          <OverviewKpi label="Speed" value={(progress?.pagesPerSecond ?? 0).toFixed(2)} />
        </div>
      </section>

      <OverviewTrend history={progressHistory} />

      <section className="overview-section">
        <div className="overview-section-title">
          <h3>Status Codes</h3>
          <span>{summary.total.toLocaleString()}</span>
        </div>
        <StackedBar segments={statusSegments} total={Math.max(summary.total, 1)} />
        <ChartLegend segments={statusSegments} onSelect={onViewSelect} />
      </section>

      <OverviewGroup
        title="URL Distribution"
        rows={urlRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />

      <section className="overview-section">
        <div className="overview-section-title">
          <h3>Indexability</h3>
          <span>{summary.indexable.toLocaleString()} indexable</span>
        </div>
        <div className="overview-indexability">
          <DonutChart
            total={Math.max(summary.total, 1)}
            primary={summary.indexable}
            secondary={summary.nonIndexable}
          />
          <div className="overview-indexability-list">
            <OverviewRow label="Indexable" value={summary.indexable} total={summary.total} />
            <OverviewRow
              label="Non-indexable"
              value={summary.nonIndexable}
              total={summary.total}
              tone="danger"
            />
            <OverviewRow label="Unknown" value={unknownIndexability} total={summary.total} />
          </div>
        </div>
      </section>

      <OverviewGroup
        title="Page Titles & Meta"
        rows={metadataRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />
      <OverviewGroup
        title="Headings"
        rows={headingRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />
      <OverviewGroup
        title="Canonicals & Directives"
        rows={directiveRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />
      <OverviewGroup
        title="Images"
        rows={mediaRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />
      <OverviewGroup
        title="Technical"
        rows={technicalRows}
        total={summary.total}
        onViewSelect={onViewSelect}
      />

      <section className="overview-section">
        <div className="overview-section-title">
          <h3>Issue Signals</h3>
          <span>{summary.broken.toLocaleString()} broken</span>
        </div>
        <div className="overview-rows">
          {issueRows.map((row) => (
            <OverviewRow
              key={row.label}
              label={row.label}
              value={row.value}
              total={Math.max(summary.total, 1)}
              tone={row.tone}
              view={row.view}
              onSelect={onViewSelect}
            />
          ))}
        </div>
      </section>
    </aside>
  );
}

function OverviewTrend({ history }: { history: ProgressSample[] }) {
  const samples = history.slice(-36);
  const latest = samples[samples.length - 1];
  const maxSpeed = Math.max(0.1, ...samples.map((sample) => sample.pagesPerSecond));
  const maxQueued = Math.max(1, ...samples.map((sample) => sample.queued));

  return (
    <section className="overview-section">
      <div className="overview-section-title">
        <h3>Crawl Speed</h3>
        <span>{latest ? `${latest.pagesPerSecond.toFixed(2)} URL/s` : "No samples"}</span>
      </div>
      <div className="overview-trend" aria-label="Crawl speed history">
        {samples.length === 0 ? (
          <span className="overview-trend-empty">No crawl samples yet</span>
        ) : (
          samples.map((sample, index) => (
            <span
              key={`${sample.timestamp}-${index}`}
              style={{
                height: `${Math.max(4, (sample.pagesPerSecond / maxSpeed) * 100)}%`,
                opacity: 0.42 + Math.min(0.5, sample.queued / maxQueued),
              }}
              title={`${sample.pagesPerSecond.toFixed(2)} URL/s, ${sample.queued} queued`}
            />
          ))
        )}
      </div>
    </section>
  );
}

function OverviewGroup({
  title,
  rows,
  total,
  onViewSelect,
}: {
  title: string;
  rows: OverviewRowModel[];
  total: number;
  onViewSelect: (view: IssueView) => void;
}) {
  return (
    <section className="overview-section">
      <div className="overview-section-title">
        <h3>{title}</h3>
        <span>{rows.reduce((sum, row) => sum + row.value, 0).toLocaleString()}</span>
      </div>
      <div className="overview-rows">
        {rows.map((row) => (
          <OverviewRow
            key={row.label}
            label={row.label}
            value={row.value}
            total={Math.max(total, 1)}
            tone={row.tone ?? "success"}
            view={row.view}
            onSelect={onViewSelect}
          />
        ))}
      </div>
    </section>
  );
}

function OverviewKpi({ label, value }: { label: string; value: string | number }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{typeof value === "number" ? value.toLocaleString() : value}</strong>
    </div>
  );
}

function StackedBar({
  segments,
  total,
}: {
  segments: OverviewStatusSegment[];
  total: number;
}) {
  return (
    <div className="stacked-bar" aria-label="Status code breakdown">
      {segments.map((segment) => (
        <span
          key={segment.label}
          className={segment.className}
          style={{ width: `${Math.max(0, (segment.value / total) * 100)}%` }}
        />
      ))}
    </div>
  );
}

function ChartLegend({
  segments,
  onSelect,
}: {
  segments: OverviewStatusSegment[];
  onSelect: (view: IssueView) => void;
}) {
  return (
    <div className="chart-legend">
      {segments.map((segment) => {
        const content = (
          <>
            <span className={segment.className} />
            <strong>{segment.label}</strong>
            <em>{segment.value.toLocaleString()}</em>
          </>
        );
        return segment.view ? (
          <button
            key={segment.label}
            type="button"
            onClick={() => onSelect(segment.view as IssueView)}
            disabled={segment.value === 0}
          >
            {content}
          </button>
        ) : (
          <div key={segment.label}>{content}</div>
        );
      })}
    </div>
  );
}

function DonutChart({
  total,
  primary,
  secondary,
}: {
  total: number;
  primary: number;
  secondary: number;
}) {
  const primaryDeg = (primary / total) * 360;
  const secondaryDeg = ((primary + secondary) / total) * 360;
  return (
    <div
      className="donut-chart"
      style={{
        background: `conic-gradient(var(--ui-success) 0deg ${primaryDeg}deg, var(--ui-danger) ${primaryDeg}deg ${secondaryDeg}deg, var(--ui-bg-accented) ${secondaryDeg}deg 360deg)`,
      }}
    >
      <span>{Math.round((primary / total) * 100)}%</span>
    </div>
  );
}

function OverviewRow({
  label,
  value,
  total,
  tone = "success",
  view,
  onSelect,
}: {
  label: string;
  value: number;
  total: number;
  tone?: OverviewTone;
  view?: IssueView;
  onSelect?: (view: IssueView) => void;
}) {
  const percent = total > 0 ? Math.min(100, (value / total) * 100) : 0;
  const content = (
    <>
      <div>
        <span>{label}</span>
        <strong>{value.toLocaleString()}</strong>
      </div>
      <div className={`overview-row-bar ${tone}`}>
        <span style={{ width: `${percent}%` }} />
      </div>
    </>
  );

  if (view && onSelect) {
    return (
      <button
        type="button"
        className="overview-row interactive"
        onClick={() => onSelect(view)}
        disabled={value === 0}
      >
        {content}
      </button>
    );
  }

  return (
    <div className="overview-row">
      {content}
    </div>
  );
}

function CheckboxField({
  checked,
  disabled,
  onCheckedChange,
  children,
}: {
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
  children: ReactNode;
}) {
  return (
    <label className="checkbox-field">
      <Checkbox.Root
        className="checkbox-root"
        checked={checked}
        disabled={disabled}
        onCheckedChange={(value) => onCheckedChange(value === true)}
      >
        <Checkbox.Indicator className="checkbox-indicator">
          <Check size={13} />
        </Checkbox.Indicator>
      </Checkbox.Root>
      <span>{children}</span>
    </label>
  );
}

function NetworkTimingBar({ record }: { record: CrawlRecord }) {
  const ttfb = record.ttfbMs ?? 0;
  const download = record.downloadTimeMs ?? 0;
  const total = Math.max(record.totalNetworkTimeMs ?? ttfb + download, ttfb + download, 1);
  const ttfbWidth = Math.max(0, Math.min(100, (ttfb / total) * 100));
  const downloadWidth = Math.max(0, Math.min(100 - ttfbWidth, (download / total) * 100));

  return (
    <div className="network-timing">
      <div className="network-timing-bar" aria-label="Network timing breakdown">
        <span className="network-ttfb" style={{ width: `${ttfbWidth}%` }} />
        <span className="network-download" style={{ width: `${downloadWidth}%` }} />
      </div>
      <div className="network-timing-meta">
        <span>DNS {formatMs(record.dnsLookupTimeMs)}</span>
        <span>TTFB {formatMs(record.ttfbMs)}</span>
        <span>Download {formatMs(record.downloadTimeMs)}</span>
        <span>Total {formatMs(record.totalNetworkTimeMs)}</span>
        <span>{formatBytes(record.transferRateBytesPerSec ?? 0)}/s</span>
        <span>{record.resolvedIpCount} IPs</span>
      </div>
    </div>
  );
}

function columnKey(column: GridColumn) {
  return column.kind === "native" ? String(column.key) : column.id;
}

function formatCell(row: CrawlRecord, column: GridColumn) {
  if (column.kind === "custom") {
    return (
      row.customExtractions
        .find((extraction) => extraction.name === column.name)
        ?.values.join(", ") || " "
    );
  }

  const value = row[column.key];
  if (value === null || value === undefined || value === "") {
    return " ";
  }
  if (
    column.key === "responseTimeMs" ||
    column.key === "dnsLookupTimeMs" ||
    column.key === "ttfbMs" ||
    column.key === "downloadTimeMs" ||
    column.key === "totalNetworkTimeMs"
  ) {
    return `${value} ms`;
  }
  if (column.key === "transferRateBytesPerSec") {
    return `${formatBytes(Number(value))}/s`;
  }
  if (column.key === "sizeBytes") {
    return formatBytes(Number(value));
  }
  if (column.key === "inSitemap") {
    return value ? "Yes" : "No";
  }
  return String(value);
}

function flagLabel(value: boolean) {
  return value ? "present" : "missing";
}

function formatMs(value?: number | null) {
  return typeof value === "number" ? `${value} ms` : "n/a";
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  let current = value;
  let unitIndex = 0;
  while (current >= 1024 && unitIndex < units.length - 1) {
    current /= 1024;
    unitIndex += 1;
  }
  return `${current >= 10 ? current.toFixed(0) : current.toFixed(1)} ${units[unitIndex]}`;
}

function sessionNameFromUrl(value: string) {
  try {
    const url = new URL(value);
    const path = url.pathname === "/" ? "" : url.pathname;
    return `${url.hostname}${path}`.slice(0, 80) || "New crawl session";
  } catch {
    return value.trim().slice(0, 80) || "New crawl session";
  }
}

function recordMatchesView(record: CrawlRecord, view: IssueView) {
  switch (view) {
    case "all":
      return true;
    case "internal":
      return record.classification === "internal";
    case "external":
      return record.classification === "external";
    case "status2xx":
      return isStatusBetween(record.statusCode, 200, 299);
    case "status3xx":
      return isStatusBetween(record.statusCode, 300, 399) || record.redirectChain.length > 0;
    case "status4xx":
      return isStatusBetween(record.statusCode, 400, 499);
    case "status5xx":
      return typeof record.statusCode === "number" && record.statusCode >= 500;
    case "noResponse":
      return record.statusCode === null || record.statusCode === undefined;
    case "titleMissing":
      return !record.title?.trim();
    case "titleTooShort":
      return Boolean(record.title?.trim()) && record.titleLen < 30;
    case "titleTooLong":
      return record.titleLen > 60;
    case "metaMissing":
      return !record.metaDescription?.trim();
    case "metaTooShort":
      return Boolean(record.metaDescription?.trim()) && record.metaDescriptionLen < 70;
    case "metaTooLong":
      return record.metaDescriptionLen > 160;
    case "h1Missing":
      return !record.h1?.trim();
    case "h1TooLong":
      return record.h1Len > 70;
    case "h2Missing":
      return !record.h2?.trim();
    case "h2TooLong":
      return record.h2Len > 70;
    case "titleSameAsH1": {
      const title = record.title?.trim();
      const h1 = record.h1?.trim();
      return Boolean(title && h1 && title.toLowerCase() === h1.toLowerCase());
    }
    case "canonicalMissing":
      return !record.canonical?.trim();
    case "canonicalMultiple":
      return record.canonicalCount > 1;
    case "directivesNoindex":
      return record.indexabilityStatus.toLowerCase().includes("noindex");
    case "imagesMissingAlt":
      return record.imagesMissingAlt > 0;
    case "imagesAltTooLong":
      return record.imagesAltTooLong > 0;
    case "securityMixedContent":
      return record.mixedContentCount > 0;
    case "securityInsecureForms":
      return record.insecureFormCount > 0;
    case "securityMissingHsts":
      return isSuccess(record) && record.finalUrl.startsWith("https://") && !record.hstsHeader;
    case "securityMissingCsp":
      return isSuccessHtml(record) && !record.contentSecurityPolicyHeader;
    case "securityMissingXFrameOptions":
      return isSuccessHtml(record) && !record.xFrameOptionsHeader;
    case "securityMissingContentTypeOptions":
      return isSuccess(record) && !record.xContentTypeOptionsHeader;
    case "mobileMissingViewport":
      return isSuccessHtml(record) && !record.viewport;
    case "hreflangInvalid":
      return record.hreflangInvalidCount > 0;
    case "hreflangMissingSelfReference":
      return record.hreflangMissingSelfReference;
    case "structuredDataInvalid":
      return record.jsonLdInvalidCount > 0;
    case "brokenLinks":
      return Boolean(record.error) || !record.statusCode || record.statusCode >= 400;
    case "sitemapOrphan":
      return record.inSitemap && record.inlinkCount === 0 && record.classification === "internal";
    case "titleDuplicate":
    case "metaDuplicate":
    case "h1Duplicate":
    case "h2Duplicate":
    case "nearDuplicate":
      return false;
  }
}

function recordMatchesSearch(record: CrawlRecord, search: string) {
  const normalizedSearch = search.trim().toLowerCase();
  if (!normalizedSearch) {
    return true;
  }

  return [
    record.url,
    record.finalUrl,
    record.title,
    record.metaDescription,
    record.metaRobots,
    record.xRobotsTag,
    record.h1,
    record.h2,
    record.canonical,
    record.amphtml,
    record.relNext,
    record.relPrev,
    record.responseHash,
    record.statusCode?.toString(),
    record.nearDuplicateClusterId?.toString(),
  ].some((value) => value?.toLowerCase().includes(normalizedSearch));
}

function sortLiveRows(
  rows: CrawlRecord[],
  sortBy?: string,
  sortDir: SortDirection = "asc",
) {
  rows.sort((left, right) => {
    const result = compareLiveRows(left, right, sortBy);
    return sortDir === "desc" ? -result : result;
  });
}

function compareLiveRows(left: CrawlRecord, right: CrawlRecord, sortBy?: string) {
  switch (sortBy) {
    case "statusCode":
      return compareValues(left.statusCode, right.statusCode);
    case "finalUrl":
      return compareValues(left.finalUrl, right.finalUrl);
    case "title":
      return compareValues(left.title, right.title);
    case "metaDescription":
      return compareValues(left.metaDescription, right.metaDescription);
    case "h1":
      return compareValues(left.h1, right.h1);
    case "h1Len":
      return compareValues(left.h1Len, right.h1Len);
    case "h1Count":
      return compareValues(left.h1Count, right.h1Count);
    case "h2":
      return compareValues(left.h2, right.h2);
    case "h2Len":
      return compareValues(left.h2Len, right.h2Len);
    case "h2Count":
      return compareValues(left.h2Count, right.h2Count);
    case "responseTimeMs":
      return compareValues(left.responseTimeMs, right.responseTimeMs);
    case "dnsLookupTimeMs":
      return compareValues(left.dnsLookupTimeMs, right.dnsLookupTimeMs);
    case "ttfbMs":
      return compareValues(left.ttfbMs, right.ttfbMs);
    case "downloadTimeMs":
      return compareValues(left.downloadTimeMs, right.downloadTimeMs);
    case "totalNetworkTimeMs":
      return compareValues(left.totalNetworkTimeMs, right.totalNetworkTimeMs);
    case "transferRateBytesPerSec":
      return compareValues(left.transferRateBytesPerSec, right.transferRateBytesPerSec);
    case "resolvedIpCount":
      return compareValues(left.resolvedIpCount, right.resolvedIpCount);
    case "inSitemap":
      return compareValues(Number(left.inSitemap), Number(right.inSitemap));
    case "wordCount":
      return compareValues(left.wordCount, right.wordCount);
    case "canonicalCount":
      return compareValues(left.canonicalCount, right.canonicalCount);
    case "imageCount":
      return compareValues(left.imageCount, right.imageCount);
    case "imagesMissingAlt":
      return compareValues(left.imagesMissingAlt, right.imagesMissingAlt);
    case "imagesAltTooLong":
      return compareValues(left.imagesAltTooLong, right.imagesAltTooLong);
    case "mixedContentCount":
      return compareValues(left.mixedContentCount, right.mixedContentCount);
    case "insecureFormCount":
      return compareValues(left.insecureFormCount, right.insecureFormCount);
    case "hreflangCount":
      return compareValues(left.hreflangCount, right.hreflangCount);
    case "hreflangInvalidCount":
      return compareValues(left.hreflangInvalidCount, right.hreflangInvalidCount);
    case "jsonLdCount":
      return compareValues(left.jsonLdCount, right.jsonLdCount);
    case "jsonLdInvalidCount":
      return compareValues(left.jsonLdInvalidCount, right.jsonLdInvalidCount);
    case "openGraphCount":
      return compareValues(left.openGraphCount, right.openGraphCount);
    case "twitterCardCount":
      return compareValues(left.twitterCardCount, right.twitterCardCount);
    case "nearDuplicateClusterId":
      return compareValues(left.nearDuplicateClusterId, right.nearDuplicateClusterId);
    case "depth":
      return compareValues(left.depth, right.depth);
    case "inlinkCount":
      return compareValues(left.inlinkCount, right.inlinkCount);
    case "outlinkCount":
      return compareValues(left.outlinkCount, right.outlinkCount);
    default:
      return compareValues(left.id, right.id);
  }
}

function compareValues(left?: string | number | null, right?: string | number | null) {
  if (left === right) {
    return 0;
  }
  if (left === null || left === undefined) {
    return -1;
  }
  if (right === null || right === undefined) {
    return 1;
  }
  if (typeof left === "number" && typeof right === "number") {
    return left - right;
  }
  return String(left).localeCompare(String(right));
}

function isStatusBetween(status: number | null | undefined, min: number, max: number) {
  return typeof status === "number" && status >= min && status <= max;
}

function isSuccess(record: CrawlRecord) {
  return isStatusBetween(record.statusCode, 200, 299);
}

function isSuccessHtml(record: CrawlRecord) {
  return (
    isSuccess(record) &&
    Boolean(record.contentType?.toLowerCase().includes("text/html"))
  );
}

function linkReportEdgeView(report: LinkReportKind): LinkEdgeView {
  if (
    report === "internal" ||
    report === "external" ||
    report === "broken" ||
    report === "nofollow"
  ) {
    return report;
  }
  return "all";
}

function linkTypeLabel(linkType: LinkType) {
  return linkType === "internal" ? "Internal" : "External";
}

function statusCell(status?: number | null) {
  if (status === null || status === undefined) {
    return "Unknown";
  }
  return String(status);
}

function graphLabel(url: string) {
  try {
    const parsed = new URL(url);
    const path = parsed.pathname === "/" ? parsed.hostname : parsed.pathname;
    return path.length > 34 ? `${path.slice(0, 31)}...` : path;
  } catch {
    return url.length > 34 ? `${url.slice(0, 31)}...` : url;
  }
}

function graphStatusLabel(node: GraphNode) {
  if (!node.crawled) {
    return "Uncrawled";
  }
  if (node.classification === "external") {
    return "External";
  }
  if (node.statusCode === null || node.statusCode === undefined) {
    return "Unknown";
  }
  if (node.statusCode >= 400) {
    return `Broken ${node.statusCode}`;
  }
  if (node.statusCode >= 300) {
    return `Redirect ${node.statusCode}`;
  }
  if (node.statusCode >= 200) {
    return `Success ${node.statusCode}`;
  }
  return String(node.statusCode);
}

function filterGraphSnapshot(
  graph: CrawlGraph | undefined,
  statusFilter: GraphStatusFilter,
  depthFilter: string,
) {
  if (!graph) {
    return undefined;
  }

  const selectedDepth = depthFilter === "all" ? undefined : Number(depthFilter);
  const nodes = graph.nodes.filter((node) => {
    if (!nodeMatchesGraphStatus(node, statusFilter)) {
      return false;
    }
    if (selectedDepth !== undefined) {
      return node.depth === selectedDepth;
    }
    return true;
  });
  const nodeUrls = new Set(nodes.map((node) => node.url));
  const edges = graph.edges.filter(
    (edge) => nodeUrls.has(edge.sourceUrl) && nodeUrls.has(edge.targetUrl),
  );

  return {
    ...graph,
    nodes,
    edges,
    totalNodes: nodes.length,
    totalEdges: edges.length,
  };
}

function nodeMatchesGraphStatus(node: GraphNode, statusFilter: GraphStatusFilter) {
  switch (statusFilter) {
    case "success":
      return isStatusBetween(node.statusCode, 200, 299);
    case "redirect":
      return isStatusBetween(node.statusCode, 300, 399);
    case "broken":
      return typeof node.statusCode === "number" && node.statusCode >= 400;
    case "external":
      return node.classification === "external";
    case "uncrawled":
      return !node.crawled;
    default:
      return true;
  }
}

function graphDepthOptions(graph: CrawlGraph | undefined) {
  if (!graph) {
    return [];
  }
  return [
    ...new Set(
      graph.nodes.flatMap((node) =>
        typeof node.depth === "number" ? [node.depth] : [],
      ),
    ),
  ].sort((left, right) => left - right);
}

function graphNodeColor(node: GraphNode, colors: ReturnType<typeof graphPalette>) {
  if (!node.crawled || node.statusCode === null || node.statusCode === undefined) {
    return colors.border;
  }
  if (node.classification === "external") {
    return colors.muted;
  }
  if (node.statusCode >= 400) {
    return colors.danger;
  }
  if (node.statusCode >= 300) {
    return colors.warning;
  }
  if (node.statusCode >= 200) {
    return colors.success;
  }
  return colors.primary;
}

function graphNodeSize(node: GraphNode) {
  const linkWeight = Math.log2(node.inlinkCount + node.outlinkCount + 2);
  if (!node.crawled) {
    return 4 + linkWeight;
  }
  if (
    node.statusCode !== null &&
    node.statusCode !== undefined &&
    node.statusCode >= 400
  ) {
    return 8 + linkWeight;
  }
  return 6 + linkWeight;
}

function graphForceLabel(node: GraphNode, nodeCount: number) {
  if (node.depth === 0) {
    return true;
  }
  if (
    node.statusCode !== null &&
    node.statusCode !== undefined &&
    node.statusCode >= 400
  ) {
    return true;
  }
  return nodeCount <= 70;
}

function graphEdgeStatusLabel(edge: LinkEdge) {
  if (edge.targetStatusCode === null || edge.targetStatusCode === undefined) {
    return "Unresolved";
  }
  if (edge.targetStatusCode >= 400) {
    return `Broken ${edge.targetStatusCode}`;
  }
  if (edge.targetStatusCode >= 300) {
    return `Redirect ${edge.targetStatusCode}`;
  }
  return String(edge.targetStatusCode);
}

function graphEdgeColor(edge: LinkEdge, colors: ReturnType<typeof graphPalette>) {
  if (edge.linkType === "external") {
    return colors.muted;
  }
  if (edge.targetStatusCode === null || edge.targetStatusCode === undefined) {
    return colors.border;
  }
  if (edge.targetStatusCode >= 400) {
    return colors.danger;
  }
  if (edge.relNofollow) {
    return colors.warning;
  }
  return colors.border;
}

function graphEdgeSize(edge: LinkEdge) {
  if (
    edge.targetStatusCode !== null &&
    edge.targetStatusCode !== undefined &&
    edge.targetStatusCode >= 400
  ) {
    return 1.8;
  }
  return edge.relNofollow ? 1.3 : 1;
}

function graphInitialPosition(
  node: GraphNode,
  index: number,
  nodes: GraphNode[],
  layoutMode: GraphLayoutMode,
  radius: number,
) {
  const maxKnownDepth = nodes.reduce((maxDepth, current) => {
    if (typeof current.depth === "number") {
      return Math.max(maxDepth, current.depth);
    }
    return maxDepth;
  }, 0);
  const depth =
    typeof node.depth === "number" && node.classification !== "external"
      ? node.depth
      : maxKnownDepth + 1;

  if (layoutMode === "radial") {
    const ring = ((depth + 1) / Math.max(1, maxKnownDepth + 2)) * radius;
    const angle = (index / Math.max(1, nodes.length)) * Math.PI * 2;
    return { x: Math.cos(angle) * ring, y: Math.sin(angle) * ring };
  }

  const x = ((depth / Math.max(1, maxKnownDepth + 1)) * 2 - 1) * radius;
  const y = ((index / Math.max(1, nodes.length - 1)) * 2 - 1) * radius;
  return { x, y };
}

function graphNodePreviewAttributes(
  node: GraphNode & {
    x: number;
    y: number;
    radius: number;
    color: string;
    forceLabel: boolean;
  },
): GraphNodeAttributes {
  return {
    label: node.label || graphLabel(node.url),
    url: node.url,
    status: graphStatusLabel(node),
    depth: node.depth === null || node.depth === undefined ? "Unknown" : String(node.depth),
    inlinks: node.inlinkCount,
    outlinks: node.outlinkCount,
    x: node.x,
    y: node.y,
    size: node.radius,
    color: node.color,
    forceLabel: node.forceLabel,
  };
}

function svgGraphLayout(graph: CrawlGraph, layoutMode: GraphLayoutMode) {
  if (layoutMode === "radial") {
    return svgRadialGraphLayout(graph);
  }

  const colors = graphPalette();
  const width = 1100;
  const height = 660;
  const margin = 58;
  const nodes = graph.nodes.slice(0, 700);
  const nodeByUrl = new Map(nodes.map((node) => [node.url, node]));
  const maxKnownDepth = nodes.reduce((maxDepth, node) => {
    if (typeof node.depth === "number") {
      return Math.max(maxDepth, node.depth);
    }
    return maxDepth;
  }, 0);
  const unknownDepth = maxKnownDepth + 1;
  const groups = new Map<number, GraphNode[]>();

  nodes.forEach((node) => {
    const depth =
      typeof node.depth === "number" && node.classification !== "external"
        ? node.depth
        : unknownDepth;
    const group = groups.get(depth) ?? [];
    group.push(node);
    groups.set(depth, group);
  });

  const depthKeys = [...groups.keys()].sort((left, right) => left - right);
  const xByDepth = new Map<number, number>();
  const columnCount = Math.max(1, depthKeys.length - 1);
  depthKeys.forEach((depth, index) => {
    xByDepth.set(depth, margin + (index / columnCount) * (width - margin * 2));
  });

  const positioned = new Map<
    string,
    GraphNode & {
      x: number;
      y: number;
      radius: number;
      color: string;
      forceLabel: boolean;
    }
  >();

  depthKeys.forEach((depth) => {
    const group = [...(groups.get(depth) ?? [])].sort((left, right) =>
      left.url.localeCompare(right.url),
    );
    const step = (height - margin * 2) / Math.max(1, group.length);
    group.forEach((node, index) => {
      positioned.set(node.url, {
        ...node,
        x: xByDepth.get(depth) ?? margin,
        y: margin + step * (index + 0.5),
        radius: Math.min(13, Math.max(5, graphNodeSize(node))),
        color: graphNodeColor(node, colors),
        forceLabel: graphForceLabel(node, nodes.length),
      });
    });
  });

  const edges = graph.edges
    .slice(0, 1_500)
    .map((edge) => {
      const source = positioned.get(edge.sourceUrl);
      const target = positioned.get(edge.targetUrl);
      if (!source || !target || !nodeByUrl.has(edge.sourceUrl) || !nodeByUrl.has(edge.targetUrl)) {
        return null;
      }
      return {
        id: edge.id,
        source,
        target,
        color: graphEdgeColor(edge, colors),
        width: graphEdgeSize(edge),
        opacity: edge.linkType === "external" ? 0.45 : 0.62,
      };
    })
    .filter((edge): edge is NonNullable<typeof edge> => edge !== null);

  return {
    width,
    height,
    nodes: [...positioned.values()],
    edges,
  };
}

function svgRadialGraphLayout(graph: CrawlGraph) {
  const colors = graphPalette();
  const width = 1100;
  const height = 660;
  const centerX = width / 2;
  const centerY = height / 2;
  const maxRadius = Math.min(width, height) / 2 - 64;
  const nodes = graph.nodes.slice(0, 700);
  const nodeByUrl = new Map(nodes.map((node) => [node.url, node]));
  const maxKnownDepth = nodes.reduce((maxDepth, node) => {
    if (typeof node.depth === "number") {
      return Math.max(maxDepth, node.depth);
    }
    return maxDepth;
  }, 0);
  const unknownDepth = maxKnownDepth + 1;
  const groups = new Map<number, GraphNode[]>();

  nodes.forEach((node) => {
    const depth =
      typeof node.depth === "number" && node.classification !== "external"
        ? node.depth
        : unknownDepth;
    const group = groups.get(depth) ?? [];
    group.push(node);
    groups.set(depth, group);
  });

  const positioned = new Map<
    string,
    GraphNode & {
      x: number;
      y: number;
      radius: number;
      color: string;
      forceLabel: boolean;
    }
  >();

  [...groups.keys()]
    .sort((left, right) => left - right)
    .forEach((depth) => {
      const group = [...(groups.get(depth) ?? [])].sort((left, right) =>
        left.url.localeCompare(right.url),
      );
      const ring = ((depth + 1) / Math.max(1, unknownDepth + 1)) * maxRadius;
      group.forEach((node, index) => {
        const angle = (index / Math.max(1, group.length)) * Math.PI * 2;
        positioned.set(node.url, {
          ...node,
          x: centerX + Math.cos(angle) * ring,
          y: centerY + Math.sin(angle) * ring,
          radius: Math.min(13, Math.max(5, graphNodeSize(node))),
          color: graphNodeColor(node, colors),
          forceLabel: graphForceLabel(node, nodes.length),
        });
      });
    });

  const edges = graph.edges
    .slice(0, 1_500)
    .map((edge) => {
      const source = positioned.get(edge.sourceUrl);
      const target = positioned.get(edge.targetUrl);
      if (!source || !target || !nodeByUrl.has(edge.sourceUrl) || !nodeByUrl.has(edge.targetUrl)) {
        return null;
      }
      return {
        id: edge.id,
        source,
        target,
        color: graphEdgeColor(edge, colors),
        width: graphEdgeSize(edge),
        opacity: edge.linkType === "external" ? 0.45 : 0.62,
      };
    })
    .filter((edge): edge is NonNullable<typeof edge> => edge !== null);

  return {
    width,
    height,
    nodes: [...positioned.values()],
    edges,
  };
}

function graphPalette() {
  const dark = document.documentElement.classList.contains("dark");
  if (dark) {
    return {
      primary: "#f58220",
      success: "#2fbf75",
      warning: "#f0b429",
      danger: "#ef5b45",
      muted: "#8f86ad",
      border: "#5b5278",
      surface: "#211d33",
      text: "#f3f0fb",
    };
  }

  return {
    primary: "#4a4a82",
    success: "#1f9d5a",
    warning: "#c98200",
    danger: "#d33f35",
    muted: "#746c93",
    border: "#c9c3df",
    surface: "#ffffff",
    text: "#2a2346",
  };
}

function supportsWebGl() {
  try {
    const canvas = document.createElement("canvas");
    return Boolean(canvas.getContext("webgl2") || canvas.getContext("webgl"));
  } catch {
    return false;
  }
}

function formatTime(timestamp: number) {
  return new Date(timestamp).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function cssVar(name: string, fallback = "") {
  if (typeof window === "undefined") {
    return fallback;
  }
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function getInitialTheme(): Theme {
  if (typeof window === "undefined") {
    return "light";
  }

  const storedTheme = window.localStorage.getItem(themeStorageKey);
  if (storedTheme === "light" || storedTheme === "dark") {
    return storedTheme;
  }

  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function getInitialStartUrl() {
  if (typeof window === "undefined") {
    return "https://example.com/";
  }

  return window.localStorage.getItem(lastUrlStorageKey) ?? "https://example.com/";
}

function getInitialOverviewWidth() {
  if (typeof window === "undefined") {
    return 306;
  }

  const storedWidth = Number(window.localStorage.getItem(overviewWidthStorageKey));
  if (Number.isFinite(storedWidth)) {
    return clamp(storedWidth, overviewMinWidth, overviewMaxWidth);
  }

  return 306;
}

function patternsToText(patterns: string[]) {
  return patterns.join("\n");
}

function textToPatterns(value: string) {
  return value
    .split(/\r?\n/)
    .map((pattern) => pattern.trim())
    .filter(Boolean);
}

function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.style.colorScheme = theme;
}

function errorMessage(caught: unknown) {
  if (caught instanceof Error) {
    return caught.message;
  }
  if (typeof caught === "string") {
    return caught;
  }
  return "Unexpected application error";
}
