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
  columnSizingFeature,
  columnVisibilityFeature,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Check,
  ChevronDown,
  ChevronUp,
  ChevronsLeft,
  ChevronsRight,
  ChevronLeft,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  FileText,
  Folder,
  GitFork,
  Info,
  ListTree,
  LogOut,
  MoreHorizontal,
  Monitor,
  MousePointer2,
  Moon,
  Network,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings,
  Square,
  Sun,
  Table2,
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
  useEffectEvent,
  useMemo,
  useRef,
  useState,
} from "react";
import { create } from "zustand";

type Theme = "light" | "dark";
type ThemePreference = Theme | "system";
type UpdateCheck = {
  currentVersion: string;
  update: { version: string; releaseUrl: string } | null;
};
type StorageMode = "memory" | "database";
type CrawlMode = "spider" | "list";
type ExtractorKind = "cssText" | "cssAttribute" | "xpath" | "regex";
type JsRenderingBackend = "chromeCdp";
type SubdomainScope = "includeSubdomains" | "exactHost";
type FolderScope = "anywhere" | "startFolder" | "exactFolder";
type UrlClassification = "internal" | "external";
type LinkType = "internal" | "external";
type SortDirection = "asc" | "desc";
type ResultsViewMode = "table" | "tree";
type GraphStatusFilter =
  | "all"
  | "success"
  | "redirect"
  | "broken"
  | "brokenLinks"
  | "redirectLinks"
  | "external"
  | "uncrawled";
type GraphLayoutMode = "depth" | "radial";
type LinkEdgeView = "all" | "internal" | "external" | "broken" | "nofollow";
type LinkReportKind =
  | LinkEdgeView
  | "selectedInlinks"
  | "selectedOutlinks"
  | "redirects"
  | "anchorText"
  | "sitemapValidation";
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
  | "titlePixelTooNarrow"
  | "titlePixelTooWide"
  | "metaMissing"
  | "metaDuplicate"
  | "metaTooShort"
  | "metaTooLong"
  | "metaPixelTooNarrow"
  | "metaPixelTooWide"
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
  | "hreflangMissingReturnLink"
  | "hreflangNonCanonicalTarget"
  | "structuredDataInvalid"
  | "structuredDataWarning"
  | "htmlDeprecatedTags"
  | "htmlDuplicateIds"
  | "renderedDomChanged"
  | "nearDuplicate"
  | "brokenLinks"
  | "sitemapOrphan";

type RedirectHop = {
  url: string;
  statusCode: number;
  location?: string | null;
  dnsLookupTimeMs?: number | null;
  tcpConnectTimeMs?: number | null;
  tlsHandshakeTimeMs?: number | null;
  ttfbMs?: number | null;
  elapsedMs?: number | null;
};

type CrawlRecord = {
  id: number;
  storageKey: string;
  url: string;
  finalUrl: string;
  listPosition?: number | null;
  listDuplicateIndex: number;
  classification: UrlClassification;
  inSitemap: boolean;
  statusCode?: number | null;
  statusText: string;
  contentType?: string | null;
  indexability: string;
  indexabilityStatus: string;
  responseTimeMs: number;
  dnsLookupTimeMs?: number | null;
  tcpConnectTimeMs?: number | null;
  tlsHandshakeTimeMs?: number | null;
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
  titlePixelWidth: number;
  metaDescription?: string | null;
  metaDescriptionLen: number;
  metaDescriptionPixelWidth: number;
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
  hreflangLinks: HreflangLink[];
  jsonLdCount: number;
  jsonLdInvalidCount: number;
  structuredDataErrorCount: number;
  structuredDataWarningCount: number;
  structuredDataIssues: StructuredDataIssue[];
  openGraphCount: number;
  twitterCardCount: number;
  deprecatedHtmlTagCount: number;
  duplicateIdCount: number;
  jsRendered: boolean;
  renderedDomChanged: boolean;
  renderedWordCountDelta: number;
  renderedLinkCountDelta: number;
  nearDuplicateClusterId?: number | null;
  inlinkCount: number;
  firstInlinkSourceUrl?: string | null;
  firstInlinkAnchorText?: string | null;
  firstInlinkSourcePosition?: number | null;
  outlinkCount: number;
  internalOutlinkCount: number;
  externalOutlinkCount: number;
  customExtractions: CustomExtractionValue[];
  customSearches: CustomSearchValue[];
  searchConsoleClicks?: number | null;
  searchConsoleImpressions?: number | null;
  searchConsoleCtr?: number | null;
  searchConsoleAveragePosition?: number | null;
  error?: string | null;
};

type HreflangLink = {
  hreflang: string;
  url: string;
  valid: boolean;
};

type StructuredDataIssue = {
  severity: "warning" | "error" | string;
  message: string;
  path: string;
};

type CustomSearchSource = "rawHtml" | "renderedHtml";

type CustomSearchValue = {
  name: string;
  source: CustomSearchSource;
  matched: boolean;
  matchCount: number;
  snippets: string[];
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
  structuredDataWarnings: number;
  deprecatedHtmlTags: number;
  duplicateIds: number;
  renderedDomChanged: number;
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

type RobotsTxtBatchTestRow = {
  url: string;
  allowed: boolean;
  error?: string | null;
};

type RobotsTxtBatchTestResult = {
  crawlDelayMs?: number | null;
  allowed: number;
  blocked: number;
  invalid: number;
  rows: RobotsTxtBatchTestRow[];
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

type UrlTreeNode = {
  id: string;
  label: string;
  path: string;
  url?: string | null;
  record?: CrawlRecord | null;
  depth: number;
  total: number;
  success: number;
  redirects: number;
  clientErrors: number;
  serverErrors: number;
  noResponse: number;
  broken: number;
  children: UrlTreeNode[];
};

type UrlTreeResponse = {
  nodes: UrlTreeNode[];
  totalUrls: number;
  renderedUrls: number;
  capped: boolean;
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

type CrawlPathResponse = {
  targetUrl: string;
  found: boolean;
  truncated: boolean;
  exploredEdges: number;
  steps: LinkEdge[];
};

type ImageAsset = {
  id: number;
  pageUrl: string;
  imageUrl: string;
  altText?: string | null;
  altLen: number;
  missingAlt: boolean;
  altTooLong: boolean;
  width?: number | null;
  height?: number | null;
  sourcePosition: number;
  sizeBytes?: number | null;
  oversized: boolean;
};

type ImageAssetResponse = {
  images: ImageAsset[];
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

type SitemapValidationRow = {
  url: string;
  finalUrl: string;
  statusCode?: number | null;
  statusText: string;
  indexability: string;
  indexabilityStatus: string;
  inlinkCount: number;
  redirectTarget?: string | null;
  canonical?: string | null;
  issueCount: number;
  severity: "info" | "warning" | "error";
  issues: string[];
};

type SitemapValidationResponse = {
  rows: SitemapValidationRow[];
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

type ExportKind =
  | "csv"
  | "xlsx"
  | "sitemap"
  | "linkEdgesCsv"
  | "redirectChainsCsv"
  | "sitemapValidationCsv"
  | "htmlReport"
  | "graphJson"
  | "graphNodesCsv"
  | "graphEdgesCsv"
  | "crawlArchive";

type ExportFileResult = {
  path: string;
  rowCount: number;
};

type CrawlArchiveImportResult = {
  records: number;
  linkEdges: number;
  imageAssets: number;
  frontierItems: number;
};

type ComparisonMetricDelta = {
  label: string;
  previous: number;
  current: number;
  delta: number;
};

type CrawlComparisonRow = {
  url: string;
  change: "added" | "removed" | "changed" | string;
  previousStatusCode?: number | null;
  currentStatusCode?: number | null;
  previousTitle?: string | null;
  currentTitle?: string | null;
  previousIndexability?: string | null;
  currentIndexability?: string | null;
  previousResponseHash?: string | null;
  currentResponseHash?: string | null;
};

type CrawlComparisonResponse = {
  baselineRecords: number;
  currentRecords: number;
  added: number;
  removed: number;
  changed: number;
  statusChanged: number;
  titleChanged: number;
  metaDescriptionChanged: number;
  indexabilityChanged: number;
  hashChanged: number;
  rows: CrawlComparisonRow[];
  metricDeltas: ComparisonMetricDelta[];
};

type SearchConsoleCredentialStatus = {
  siteUrl?: string | null;
  tokenSaved: boolean;
  keyringAvailable: boolean;
  message?: string | null;
};

type SearchConsoleTestResult = {
  rows: number;
  clicks: number;
  impressions: number;
};

type SearchConsoleMergeResult = {
  fetchedRows: number;
  matchedRows: number;
  clicks: number;
  impressions: number;
};

type DatabaseLocation = {
  path: string;
};

type CrawlRecoveryState = {
  recoverable: boolean;
  queued: number;
  seen: number;
  crawled: number;
};

type CrawlCapacityEstimate = {
  urlLimit: number;
  ramBytes: number;
  diskBytes: number;
  deviceBudgetBytes?: number;
  tone: "success" | "warning" | "danger";
  recommendation: string;
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

type SettingsTab =
  | "crawl"
  | "scope"
  | "resources"
  | "query"
  | "storage"
  | "profiles"
  | "integrations"
  | "rendering"
  | "extraction";

type CrawlConfig = {
  mode: CrawlMode;
  startUrl: string;
  listUrls: string[];
  listSitemapUrls: string[];
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
  retryAttempts: number;
  retryBackoffMs: number;
  nearDuplicateThreshold: number;
  includeUrlPatterns: string[];
  excludeUrlPatterns: string[];
  subdomainScope: SubdomainScope;
  folderScope: FolderScope;
  followNofollow: boolean;
  resourceTypes: ResourceTypeConfig;
  querySettings: QuerySettingsConfig;
  customExtractors: CustomExtractor[];
  customSearches: CustomSearch[];
  rendering: JsRenderingConfig;
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

type CustomSearch = {
  name: string;
  pattern: string;
  regex: boolean;
  caseSensitive: boolean;
  maxSnippets: number;
};

type UrlSegment = {
  id: string;
  name: string;
  pattern: string;
  regex: boolean;
};

type JsRenderingConfig = {
  enabled: boolean;
  backend: JsRenderingBackend;
  waitAfterLoadMs: number;
};

type AppState = {
  theme: ThemePreference;
  resolvedTheme: Theme;
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
  pageIndex: number;
  summary: CrawlSummary;
  progress?: CrawlProgress;
  running: boolean;
  paused: boolean;
  error?: string;
  notice?: string;
  settingsError?: string;
  setConfig: (config: Partial<CrawlConfig>) => void;
  setRows: (response: GridResponse) => void;
  setSelected: (record?: CrawlRecord) => void;
  setView: (view: IssueView) => void;
  setSearch: (search: string) => void;
  setSort: (sortBy: string) => void;
  setPage: (pageIndex: number) => void;
  setProgress: (progress?: CrawlProgress) => void;
  setRunning: (running: boolean) => void;
  setPaused: (paused: boolean) => void;
  setError: (error?: string) => void;
  setNotice: (notice?: string) => void;
  setTheme: (theme: ThemePreference) => void;
  setStorageMode: (storageMode: StorageMode) => void;
  setResumeCrawl: (resumeCrawl: boolean) => void;
};

const themeStorageKey = "ferrous-frog-theme";
const lastUrlStorageKey = "ferrous-frog-last-url";
const overviewWidthStorageKey = "ferrous-frog-overview-width";
const urlSegmentsStorageKey = "ferrous-frog-url-segments";
const settingsStorageKey = "ferrous-frog-settings";
const updateReminderKey = "ferrous-frog-update-reminder-until";
const updateReminderDelay = 24 * 60 * 60 * 1000;
const overviewMinWidth = 260;
const overviewMaxWidth = 560;
const resultsPageSize = 500;
const gridFeatures = tableFeatures({
  columnSizingFeature,
  columnVisibilityFeature,
  rowSortingFeature,
});
const detailTabs = [
  { id: "page", label: "URL details" },
  { id: "inlinks", label: "Inlinks" },
  { id: "outlinks", label: "Outlinks" },
  { id: "links", label: "Links & indexing" },
  { id: "technical", label: "Technical" },
  { id: "custom", label: "Custom data" },
] as const;

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
  structuredDataWarnings: 0,
  deprecatedHtmlTags: 0,
  duplicateIds: 0,
  renderedDomChanged: 0,
  missingViewport: 0,
  missingHsts: 0,
  sitemapOrphans: 0,
};

const defaultConfig: CrawlConfig = {
  mode: "spider",
  startUrl: getInitialStartUrl(),
  listUrls: [],
  listSitemapUrls: [],
  maxUrls: 5000,
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
  retryAttempts: 1,
  retryBackoffMs: 250,
  nearDuplicateThreshold: 6,
  includeUrlPatterns: [],
  excludeUrlPatterns: [],
  subdomainScope: "includeSubdomains",
  folderScope: "anywhere",
  followNofollow: true,
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
  customSearches: [],
  rendering: {
    enabled: false,
    backend: "chromeCdp",
    waitAfterLoadMs: 500,
  },
};

function normalizeCrawlConfig(config: Partial<CrawlConfig>): CrawlConfig {
  return {
    ...defaultConfig,
    ...config,
    listUrls: [...(config.listUrls ?? [])],
    listSitemapUrls: [...(config.listSitemapUrls ?? [])],
    includeUrlPatterns: [...(config.includeUrlPatterns ?? [])],
    excludeUrlPatterns: [...(config.excludeUrlPatterns ?? [])],
    subdomainScope: config.subdomainScope ?? defaultConfig.subdomainScope,
    folderScope: config.folderScope ?? defaultConfig.folderScope,
    followNofollow: config.followNofollow ?? defaultConfig.followNofollow,
    resourceTypes: {
      ...defaultConfig.resourceTypes,
      ...(config.resourceTypes ?? {}),
    },
    querySettings: {
      ...defaultConfig.querySettings,
      ...(config.querySettings ?? {}),
      stripParameterPatterns: [
        ...(config.querySettings?.stripParameterPatterns ?? []),
      ],
    },
    customExtractors: [...(config.customExtractors ?? [])],
    customSearches: [...(config.customSearches ?? [])],
    rendering: {
      ...defaultConfig.rendering,
      ...(config.rendering ?? {}),
    },
  };
}

function getInitialSettings(): Pick<AppState, "config" | "storageMode" | "resumeCrawl" | "settingsError"> {
  const defaults = { config: defaultConfig, storageMode: "memory" as StorageMode, resumeCrawl: false };
  try {
    const raw = window.localStorage.getItem(settingsStorageKey);
    if (!raw) return defaults;
    const saved = JSON.parse(raw);
    if (saved?.version !== 1 || !saved.config || typeof saved.config !== "object" || Array.isArray(saved.config)) {
      throw new Error("Invalid settings format");
    }
    // Older snapshots can omit new fields; malformed values must never reach the controls.
    for (const [value, template] of [[saved.config, defaultConfig], [saved.config.resourceTypes ?? {}, defaultConfig.resourceTypes],
      [saved.config.querySettings ?? {}, defaultConfig.querySettings], [saved.config.rendering ?? {}, defaultConfig.rendering]]) {
      if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid settings section");
      for (const [key, fallback] of Object.entries(template)) {
        const entry = (value as Record<string, unknown>)[key];
        if (entry === undefined) continue;
        if (Array.isArray(fallback) ? !Array.isArray(entry) : entry === null || typeof entry !== typeof fallback ||
          (typeof entry === "number" && !Number.isFinite(entry))) throw new Error("Invalid setting value");
      }
    }
    const config = normalizeCrawlConfig(saved.config);
    if (![config.listUrls, config.listSitemapUrls, config.includeUrlPatterns, config.excludeUrlPatterns,
      config.querySettings.stripParameterPatterns].every((list) => list.every((value) => typeof value === "string")) ||
      !config.customExtractors.every((item) => item && typeof item.name === "string" && typeof item.pattern === "string" &&
        ["cssText", "cssAttribute", "xpath", "regex"].includes(item.kind) && typeof item.allMatches === "boolean" &&
        (item.attribute == null || typeof item.attribute === "string")) ||
      !config.customSearches.every((item) => item && typeof item.name === "string" && typeof item.pattern === "string" &&
        typeof item.regex === "boolean" && typeof item.caseSensitive === "boolean" && Number.isFinite(item.maxSnippets)) ||
      !["spider", "list"].includes(config.mode) || !["includeSubdomains", "exactHost"].includes(config.subdomainScope) ||
      !["anywhere", "startFolder", "exactFolder"].includes(config.folderScope) || config.rendering.backend !== "chromeCdp") {
      throw new Error("Invalid crawl configuration");
    }
    return { config, storageMode: saved.storageMode === "database" ? "database" : "memory", resumeCrawl: saved.resumeCrawl === true };
  } catch {
    return { ...defaults, settingsError: "Saved settings could not be read. Default settings are in use; your next change will save a new copy." };
  }
}

const initialTheme = getInitialTheme();
const initialSettings = getInitialSettings();
const useAppStore = create<AppState>((set, get) => ({
  theme: initialTheme,
  resolvedTheme: resolveTheme(initialTheme),
  ...initialSettings,
  rows: [],
  selectedView: "all",
  globalSearch: "",
  sortDir: "asc",
  total: 0,
  pageIndex: 0,
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
        ? response.rows.find((row) => row.id === state.selected?.id) ?? state.selected
        : undefined,
    })),
  setSelected: (record) => set({ selected: record }),
  setView: (view) => set({ selectedView: view, pageIndex: 0 }),
  setSearch: (search) => set({ globalSearch: search, pageIndex: 0 }),
  setSort: (sortBy) =>
    set((state) => ({
      sortBy,
      pageIndex: 0,
      sortDir:
        state.sortBy === sortBy && state.sortDir === "asc" ? "desc" : "asc",
    })),
  setPage: (pageIndex) => set({ pageIndex }),
  setProgress: (progress) =>
    set({
      progress,
      summary: progress?.summary ?? get().summary,
    }),
  setRunning: (running) => set({ running }),
  setPaused: (paused) => set({ paused }),
  setError: (error) => set({ error }),
  setNotice: (notice) => set({ notice }),
  setTheme: (theme) => {
    const resolvedTheme = resolveTheme(theme);
    applyTheme(resolvedTheme);
    savePreference(themeStorageKey, theme);
    set({ theme, resolvedTheme });
  },
  setStorageMode: (storageMode) => set({ storageMode }),
  setResumeCrawl: (resumeCrawl) => set({ resumeCrawl }),
}));

useAppStore.subscribe((state, previous) => {
  if (state.config === previous.config && state.storageMode === previous.storageMode && state.resumeCrawl === previous.resumeCrawl) return;
  const saved = savePreference(settingsStorageKey, JSON.stringify({
    version: 1, config: state.config, storageMode: state.storageMode, resumeCrawl: state.resumeCrawl,
  }));
  if (saved && state.settingsError) useAppStore.setState({ settingsError: undefined });
});

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
  { id: "titlePixelTooNarrow", label: "Narrow Title px" },
  { id: "titlePixelTooWide", label: "Wide Title px" },
  { id: "titleSameAsH1", label: "Title = H1" },
  { id: "metaMissing", label: "Missing Meta" },
  { id: "metaDuplicate", label: "Duplicate Meta" },
  { id: "metaTooShort", label: "Short Meta" },
  { id: "metaTooLong", label: "Long Meta" },
  { id: "metaPixelTooNarrow", label: "Narrow Meta px" },
  { id: "metaPixelTooWide", label: "Wide Meta px" },
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
  { id: "hreflangMissingReturnLink", label: "Hreflang Return" },
  { id: "hreflangNonCanonicalTarget", label: "Hreflang Canonical" },
  { id: "structuredDataInvalid", label: "Structured Errors" },
  { id: "structuredDataWarning", label: "Structured Warnings" },
  { id: "htmlDeprecatedTags", label: "Deprecated HTML" },
  { id: "htmlDuplicateIds", label: "Duplicate IDs" },
  { id: "renderedDomChanged", label: "Rendered Changes" },
  { id: "nearDuplicate", label: "Near Duplicates" },
  { id: "brokenLinks", label: "Broken Links" },
  { id: "sitemapOrphan", label: "Sitemap Orphans" },
];

const issueGroups: Array<{ label: string; tabLabel?: string; views: IssueView[]; columns: Array<keyof CrawlRecord> }> = [
  { label: "Crawl overview", tabLabel: "URLs", views: ["all", "internal", "external"], columns: ["title", "contentType", "indexability", "depth", "inlinkCount", "outlinkCount", "responseTimeMs"] },
  { label: "Response codes", tabLabel: "Responses", views: ["brokenLinks", "status2xx", "status3xx", "status4xx", "status5xx", "noResponse"], columns: ["finalUrl", "firstInlinkSourceUrl", "redirectTarget", "responseTimeMs", "depth", "inlinkCount"] },
  { label: "Page titles", tabLabel: "Titles", views: ["titleMissing", "titleDuplicate", "titleTooShort", "titleTooLong", "titlePixelTooNarrow", "titlePixelTooWide", "titleSameAsH1"], columns: ["title", "titleLen", "titlePixelWidth", "h1", "indexability"] },
  { label: "Meta descriptions", tabLabel: "Descriptions", views: ["metaMissing", "metaDuplicate", "metaTooShort", "metaTooLong", "metaPixelTooNarrow", "metaPixelTooWide"], columns: ["metaDescription", "metaDescriptionLen", "metaDescriptionPixelWidth", "indexability"] },
  { label: "Headings", views: ["h1Missing", "h1Duplicate", "h1TooLong", "h2Missing", "h2Duplicate", "h2TooLong"], columns: ["h1", "h1Count", "h2", "h2Count", "indexability"] },
  { label: "Canonicals & directives", tabLabel: "Indexing", views: ["canonicalMissing", "canonicalMultiple", "directivesNoindex"], columns: ["canonical", "canonicalCount", "metaRobots", "xRobotsTag", "indexability", "indexabilityStatus"] },
  { label: "Images", views: ["imagesMissingAlt", "imagesAltTooLong"], columns: ["imageCount", "imagesMissingAlt", "imagesAltTooLong", "outlinkCount"] },
  { label: "Security", views: ["securityMixedContent", "securityInsecureForms", "securityMissingHsts", "securityMissingCsp", "securityMissingXFrameOptions", "securityMissingContentTypeOptions"], columns: ["mixedContentCount", "insecureFormCount", "hstsHeader", "contentSecurityPolicyHeader", "xFrameOptionsHeader", "xContentTypeOptionsHeader"] },
  { label: "International", tabLabel: "Hreflang", views: ["hreflangInvalid", "hreflangMissingSelfReference", "hreflangMissingReturnLink", "hreflangNonCanonicalTarget"], columns: ["hreflangCount", "hreflangInvalidCount", "canonical", "indexability"] },
  { label: "Structured data & HTML", tabLabel: "Markup", views: ["structuredDataInvalid", "structuredDataWarning", "htmlDeprecatedTags", "htmlDuplicateIds"], columns: ["jsonLdInvalidCount", "structuredDataErrorCount", "structuredDataWarningCount", "deprecatedHtmlTagCount", "duplicateIdCount"] },
  { label: "Content & rendering", tabLabel: "Content", views: ["nearDuplicate", "renderedDomChanged", "mobileMissingViewport"], columns: ["wordCount", "nearDuplicateClusterId", "jsRendered", "renderedWordCountDelta", "renderedLinkCountDelta", "viewport"] },
  { label: "Sitemaps", views: ["sitemapOrphan"], columns: ["inSitemap", "canonical", "indexability", "inlinkCount"] },
];

const viewSummaryKeys: Partial<Record<IssueView, keyof CrawlSummary>> = {
  all: "total", internal: "internal", external: "external", status2xx: "success", status3xx: "redirects",
  status4xx: "clientErrors", status5xx: "serverErrors", noResponse: "noResponse", brokenLinks: "broken",
  titleMissing: "titleMissing", titleDuplicate: "titleDuplicate", metaMissing: "metaMissing", metaDuplicate: "metaDuplicate",
  h1Missing: "h1Missing", h1Duplicate: "h1Duplicate", h2Missing: "h2Missing", h2Duplicate: "h2Duplicate",
  canonicalMissing: "canonicalMissing", canonicalMultiple: "canonicalMultiple", directivesNoindex: "noindex",
  imagesMissingAlt: "imagesMissingAlt", imagesAltTooLong: "imagesAltTooLong", securityMixedContent: "mixedContent",
  securityInsecureForms: "insecureForms", securityMissingHsts: "missingHsts", mobileMissingViewport: "missingViewport",
  hreflangInvalid: "hreflangInvalid", structuredDataInvalid: "structuredDataInvalid", structuredDataWarning: "structuredDataWarnings",
  htmlDeprecatedTags: "deprecatedHtmlTags", htmlDuplicateIds: "duplicateIds", renderedDomChanged: "renderedDomChanged",
  nearDuplicate: "nearDuplicates", sitemapOrphan: "sitemapOrphans",
};

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
      sortable: boolean;
    }
  | {
      kind: "search";
      name: string;
      id: string;
      label: string;
      width: number;
      grow?: number;
      sortable: boolean;
    };

const nativeColumns: GridColumn[] = [
  { kind: "native", key: "url", label: "URL", width: 360, sortable: true },
  {
    kind: "native",
    key: "listPosition",
    label: "List #",
    width: 72,
    sortable: true,
  },
  { kind: "native", key: "statusCode", label: "Status", width: 76, sortable: true },
  {
    kind: "native",
    key: "finalUrl",
    label: "Final URL",
    width: 360,
    grow: 1.5,
    sortable: true,
  },
  {
    kind: "native",
    key: "firstInlinkSourceUrl",
    label: "Found From",
    width: 320,
    grow: 1,
    sortable: false,
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
    key: "titlePixelWidth",
    label: "Title px",
    width: 86,
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
    key: "metaDescriptionPixelWidth",
    label: "Meta px",
    width: 86,
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
    key: "searchConsoleClicks",
    label: "GSC Clicks",
    width: 98,
    sortable: true,
  },
  {
    kind: "native",
    key: "searchConsoleImpressions",
    label: "GSC Impr.",
    width: 104,
    sortable: true,
  },
  {
    kind: "native",
    key: "searchConsoleCtr",
    label: "GSC CTR",
    width: 86,
    sortable: true,
  },
  {
    kind: "native",
    key: "searchConsoleAveragePosition",
    label: "GSC Pos.",
    width: 90,
    sortable: true,
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
    key: "tcpConnectTimeMs",
    label: "TCP",
    width: 72,
    sortable: true,
  },
  {
    kind: "native",
    key: "tlsHandshakeTimeMs",
    label: "TLS",
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
    key: "listDuplicateIndex",
    label: "List Dup",
    width: 86,
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
  {
    kind: "native",
    key: "structuredDataErrorCount",
    label: "SD Err",
    width: 72,
    sortable: true,
  },
  {
    kind: "native",
    key: "structuredDataWarningCount",
    label: "SD Warn",
    width: 82,
    sortable: true,
  },
  {
    kind: "native",
    key: "deprecatedHtmlTagCount",
    label: "HTML Dep",
    width: 86,
    sortable: true,
  },
  {
    kind: "native",
    key: "duplicateIdCount",
    label: "Dup IDs",
    width: 78,
    sortable: true,
  },
  {
    kind: "native",
    key: "jsRendered",
    label: "Rendered",
    width: 86,
    sortable: true,
  },
  {
    kind: "native",
    key: "renderedWordCountDelta",
    label: "Word Diff",
    width: 88,
    sortable: true,
  },
  {
    kind: "native",
    key: "renderedLinkCountDelta",
    label: "Link Diff",
    width: 82,
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
  { kind: "native", key: "titleLen", label: "Title length", width: 100, sortable: true },
  { kind: "native", key: "metaDescriptionLen", label: "Meta length", width: 100, sortable: true },
  { kind: "native", key: "h1Count", label: "H1 count", width: 90, sortable: true },
  { kind: "native", key: "h2Count", label: "H2 count", width: 90, sortable: true },
  { kind: "native", key: "canonical", label: "Canonical URL", width: 320, sortable: false },
  { kind: "native", key: "redirectTarget", label: "Redirect target", width: 320, sortable: false },
  { kind: "native", key: "indexabilityStatus", label: "Indexability reason", width: 180, sortable: false },
  { kind: "native", key: "metaRobots", label: "Meta robots", width: 160, sortable: false },
  { kind: "native", key: "xRobotsTag", label: "X-Robots-Tag", width: 160, sortable: false },
  { kind: "native", key: "imageCount", label: "Images", width: 90, sortable: true },
  { kind: "native", key: "imagesAltTooLong", label: "Long alt", width: 90, sortable: true },
  { kind: "native", key: "hreflangCount", label: "Hreflang count", width: 120, sortable: true },
  { kind: "native", key: "hstsHeader", label: "HSTS", width: 85, sortable: false },
  { kind: "native", key: "contentSecurityPolicyHeader", label: "CSP", width: 85, sortable: false },
  { kind: "native", key: "xFrameOptionsHeader", label: "X-Frame-Options", width: 140, sortable: false },
  { kind: "native", key: "xContentTypeOptionsHeader", label: "X-Content-Type-Options", width: 180, sortable: false },
  { kind: "native", key: "viewport", label: "Viewport", width: 90, sortable: false },
];

const extractorKinds: Array<{ value: ExtractorKind; label: string }> = [
  { value: "cssText", label: "CSS text" },
  { value: "cssAttribute", label: "CSS attribute" },
  { value: "xpath", label: "XPath" },
  { value: "regex", label: "Regex" },
];

const settingsTabs: Array<{
  id: SettingsTab;
  label: string;
  description: string;
}> = [
  {
    id: "crawl",
    label: "Crawl",
    description: "Limits, robots.txt, and crawl throughput.",
  },
  {
    id: "scope",
    label: "Scope",
    description: "List sources and URL include or exclude rules.",
  },
  {
    id: "resources",
    label: "Resources",
    description: "Choose which asset and URL types can enter the crawl.",
  },
  {
    id: "query",
    label: "Query",
    description: "Normalize, strip, or limit query-string parameters.",
  },
  {
    id: "storage",
    label: "Storage",
    description: "Memory mode, database mode, and crawl sessions.",
  },
  {
    id: "profiles",
    label: "Profiles",
    description: "Save and reuse named crawl configurations.",
  },
  {
    id: "integrations",
    label: "Integrations",
    description: "External metrics and credential status.",
  },
  {
    id: "rendering",
    label: "Rendering",
    description: "Rendered DOM crawling through Chrome DevTools Protocol.",
  },
  {
    id: "extraction",
    label: "Extraction",
    description: "Create custom CSS, XPath, and regex extraction columns.",
  },
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
  { id: "sitemapValidation", label: "Sitemap Validation" },
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
    pageIndex,
    summary,
    progress,
    running,
    paused,
    theme,
    resolvedTheme,
    storageMode,
    resumeCrawl,
    setConfig,
    setRows,
    setSelected,
    setView,
    setSearch,
    setSort,
    setPage,
    setProgress,
    setRunning,
    setPaused,
    setError,
    setNotice,
    setTheme,
    setStorageMode,
    setResumeCrawl,
  } = useAppStore();
  const parentRef = useRef<HTMLDivElement>(null);
  const rowsRequest = useRef(0);
  const [rowsLoading, setRowsLoading] = useState(false);
  const [startupReady, setStartupReady] = useState(false);
  const [quitOpen, setQuitOpen] = useState(false);
  const [quitting, setQuitting] = useState(false);
  const [quitError, setQuitError] = useState<string>();
  const cancelQuitRef = useRef<HTMLButtonElement>(null);
  const quitOriginRef = useRef<HTMLElement | null>(null);
  const requestQuit = useCallback(() => {
    if (!cancelQuitRef.current) quitOriginRef.current = document.activeElement as HTMLElement;
    setQuitError(undefined);
    setQuitOpen(true);
  }, []);
  const [showAllColumns, setShowAllColumns] = useState(false);
  const [issuesOpen, setIssuesOpen] = useState(false);
  const [overviewOpen, setOverviewOpen] = useState(() => window.innerWidth > 1000);
  const [activeIssueGroup, setActiveIssueGroup] = useState(issueGroups[0]);
  const desktopRuntime = isTauri();
  const [updateOpen, setUpdateOpen] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheck>();
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [openingUpdate, setOpeningUpdate] = useState(false);
  const [updateError, setUpdateError] = useState<string>();
  const updateCheckState = useRef({ running: false, checked: false, manual: false });
  const updateDismissRef = useRef<HTMLButtonElement>(null);
  const updateOriginRef = useRef<HTMLElement | null>(null);
  const checkForUpdates = useCallback(async (manual: boolean) => {
    if (!desktopRuntime) return;
    if (manual) {
      updateCheckState.current.manual = true;
      updateOriginRef.current = document.activeElement as HTMLElement;
      setUpdateOpen(true);
    } else {
      const reminder = Number(readPreference(updateReminderKey));
      if (updateCheckState.current.checked || (Number.isFinite(reminder) && reminder > Date.now())) return;
    }
    if (updateCheckState.current.running) return;
    updateCheckState.current = { running: true, checked: true, manual };
    setCheckingUpdates(true);
    setUpdateResult(undefined);
    setUpdateError(undefined);
    try {
      const result = await invoke<UpdateCheck>("check_for_updates");
      setUpdateResult(result);
      if (result.update && !updateCheckState.current.manual) {
        updateOriginRef.current = document.activeElement as HTMLElement;
        setUpdateOpen(true);
      }
    } catch (caught) {
      setUpdateError(errorMessage(caught));
    } finally {
      updateCheckState.current.running = false;
      setCheckingUpdates(false);
    }
  }, [desktopRuntime]);
  const dismissUpdate = () => {
    if (updateResult?.update) savePreference(updateReminderKey, String(Date.now() + updateReminderDelay));
    setUpdateOpen(false);
  };
  const openUpdateDownload = async () => {
    if (!updateResult?.update || openingUpdate) return;
    setOpeningUpdate(true);
    setUpdateError(undefined);
    try {
      await invoke("open_external_url", { url: updateResult.update.releaseUrl });
      dismissUpdate();
    } catch (caught) {
      setUpdateError(errorMessage(caught));
    } finally {
      setOpeningUpdate(false);
    }
  };

  const columns = useMemo<GridColumn[]>(() => {
    const customColumns = config.customExtractors
      .filter((extractor) => extractor.name.trim().length > 0)
      .map<GridColumn>((extractor, index) => ({
        kind: "custom",
        name: extractor.name,
        id: `custom:${extractor.name}:${index}`,
        label: extractor.name,
        width: 180,
        sortable: true,
      }));
    const searchColumns = config.customSearches
      .filter((search) => search.name.trim().length > 0)
      .map<GridColumn>((search, index) => ({
        kind: "search",
        name: search.name,
        id: `search:${search.name}:${index}`,
        label: search.name,
        width: 150,
        sortable: true,
      }));

    const relevantKeys = ["statusCode", "url", ...activeIssueGroup.columns];
    const visibleColumns = showAllColumns
      ? nativeColumns
      : relevantKeys.flatMap((key) => nativeColumns.filter((column) => column.kind === "native" && column.key === key));
    const listColumns = config.mode === "list" && !showAllColumns
      ? nativeColumns.filter((column) => column.kind === "native" && column.key === "listPosition")
      : [];
    return [...listColumns, ...visibleColumns, ...customColumns, ...searchColumns];
  }, [activeIssueGroup, showAllColumns, config.mode, config.customExtractors, config.customSearches]);

  const tableColumns = useMemo<ColumnDef<typeof gridFeatures, CrawlRecord>[]>(
    () =>
      columns.map((column) => ({
        id: columnKey(column),
        header: column.label,
        size: column.width,
        minSize: column.width,
        enableSorting: column.sortable,
        accessorFn: (row) => formatCell(row, column),
        cell: ({ getValue }) => getValue<string>(),
      })),
    [columns],
  );
  const gridWidth = useMemo(
    () => columns.reduce((totalWidth, column) => totalWidth + column.width, 0),
    [columns],
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [detailTab, setDetailTab] = useState<(typeof detailTabs)[number]["id"]>("page");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("crawl");
  const [aboutOpen, setAboutOpen] = useState(false);
  const [comparisonOpen, setComparisonOpen] = useState(false);
  const [comparisonArchivePath, setComparisonArchivePath] = useState("");
  const [comparisonLoading, setComparisonLoading] = useState(false);
  const [comparisonResult, setComparisonResult] = useState<CrawlComparisonResponse>();
  const [linkReportsOpen, setLinkReportsOpen] = useState(false);
  const [linkReportPage, setLinkReportPage] = useState(0);
  const linkReportRequest = useRef(0);
  const [selectedLinkReport, setSelectedLinkReport] = useState<LinkReportKind>("all");
  const [linkEdges, setLinkEdges] = useState<LinkEdge[]>([]);
  const [linkEdgeTotal, setLinkEdgeTotal] = useState(0);
  const [anchorTextRows, setAnchorTextRows] = useState<AnchorTextRow[]>([]);
  const [anchorTextTotal, setAnchorTextTotal] = useState(0);
  const [redirectReportRows, setRedirectReportRows] = useState<CrawlRecord[]>([]);
  const [redirectReportTotal, setRedirectReportTotal] = useState(0);
  const [sitemapValidationRows, setSitemapValidationRows] = useState<SitemapValidationRow[]>([]);
  const [sitemapValidationTotal, setSitemapValidationTotal] = useState(0);
  const [selectedImages, setSelectedImages] = useState<ImageAsset[]>([]);
  const [selectedImageTotal, setSelectedImageTotal] = useState(0);
  const [selectedCrawlPath, setSelectedCrawlPath] = useState<CrawlPathResponse>();
  const [crawlPathLoading, setCrawlPathLoading] = useState(false);
  const [linkReportLoading, setLinkReportLoading] = useState(false);
  const [linkReportSearch, setLinkReportSearch] = useState("");
  const [linkReportSortBy, setLinkReportSortBy] = useState("sourceUrl");
  const [linkReportSortDir, setLinkReportSortDir] = useState<SortDirection>("asc");
  const [anchorTextSortBy, setAnchorTextSortBy] = useState("linkCount");
  const [anchorTextSortDir, setAnchorTextSortDir] = useState<SortDirection>("desc");
  const [sitemapValidationSortBy, setSitemapValidationSortBy] = useState("severity");
  const [sitemapValidationSortDir, setSitemapValidationSortDir] =
    useState<SortDirection>("desc");
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
  const [searchConsoleStatus, setSearchConsoleStatus] =
    useState<SearchConsoleCredentialStatus>({
      siteUrl: null,
      tokenSaved: false,
      keyringAvailable: true,
    });
  const [searchConsoleSiteUrl, setSearchConsoleSiteUrl] = useState("");
  const [searchConsoleAccessToken, setSearchConsoleAccessToken] = useState("");
  const [searchConsoleStartDate, setSearchConsoleStartDate] = useState(() =>
    isoDateDaysAgo(30),
  );
  const [searchConsoleEndDate, setSearchConsoleEndDate] = useState(() =>
    isoDateDaysAgo(3),
  );
  const [searchConsoleRowLimit, setSearchConsoleRowLimit] = useState(1000);
  const [searchConsoleLoading, setSearchConsoleLoading] = useState(false);
  const [searchConsoleTestResult, setSearchConsoleTestResult] =
    useState<SearchConsoleTestResult>();
  const [searchConsoleMergeResult, setSearchConsoleMergeResult] =
    useState<SearchConsoleMergeResult>();
  const [databasePath, setDatabasePath] = useState("");
  const [customDatabasePath, setCustomDatabasePath] = useState("");
  const [archiveImportPath, setArchiveImportPath] = useState("");
  const [recoveryState, setRecoveryState] = useState<CrawlRecoveryState>({
    recoverable: false,
    queued: 0,
    seen: 0,
    crawled: 0,
  });
  const [robotsTestUrl, setRobotsTestUrl] = useState("");
  const [robotsTestResult, setRobotsTestResult] = useState<string>();
  const [robotsBatchUrls, setRobotsBatchUrls] = useState("");
  const [robotsBatchResult, setRobotsBatchResult] =
    useState<RobotsTxtBatchTestResult>();
  const [overviewWidth, setOverviewWidth] = useState(getInitialOverviewWidth);
  const [overviewResizing, setOverviewResizing] = useState(false);
  const [urlSegments, setUrlSegments] = useState<UrlSegment[]>(getInitialUrlSegments);
  const [activeSegmentId, setActiveSegmentId] = useState("all");
  const [resultsViewMode, setResultsViewMode] = useState<ResultsViewMode>("table");
  const [urlTree, setUrlTree] = useState<UrlTreeResponse>({
    nodes: [],
    totalUrls: 0,
    renderedUrls: 0,
    capped: false,
  });
  const [urlTreeLoading, setUrlTreeLoading] = useState(false);
  const exportDisabled = summary.total === 0 || !desktopRuntime;
  const filteredExportDisabled = exportDisabled || total === 0;
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
  const listSourceCount =
    (config.listUrls?.length ?? 0) + (config.listSitemapUrls?.length ?? 0);
  const hasCrawlTarget = Boolean(config.startUrl.trim()) || (config.mode === "list" &&
    [...config.listUrls, ...config.listSitemapUrls].some((url) => url.trim()));
  const activeSettingsTab =
    settingsTabs.find((tab) => tab.id === settingsTab) ?? settingsTabs[0];
  const crawlTargetLabel =
    config.mode === "list"
      ? `${listSourceCount || 1} list source${listSourceCount === 1 ? "" : "s"}`
      : config.startUrl;
  const capacityEstimate = useMemo(
    () => estimateCrawlCapacity(config, storageMode),
    [
      config.maxUrls,
      config.resourceTypes.css,
      config.resourceTypes.external,
      config.resourceTypes.images,
      config.resourceTypes.javascript,
      config.resourceTypes.other,
      storageMode,
    ],
  );
  const selectedUrl = selected?.finalUrl;
  const activeSegment = useMemo(
    () => urlSegments.find((segment) => segment.id === activeSegmentId),
    [activeSegmentId, urlSegments],
  );
  const table = useTable({
    features: gridFeatures,
    data: rows,
    columns: tableColumns,
    manualSorting: true,
    getRowId: (row) => String(row.id),
    enableSortingRemoval: false,
  });
  const tableRows = table.getRowModel().rows;

  const rowVirtualizer = useVirtualizer({
    count: tableRows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 28,
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
      return;
    }

    const requestId = ++rowsRequest.current;
    setRowsLoading(true);
    try {
      const response = await invoke<GridResponse>("get_rows", {
        query: {
          offset: pageIndex * resultsPageSize,
          limit: resultsPageSize,
          globalSearch,
          ...segmentQuery(activeSegment),
          sortBy,
          sortDir,
          view: selectedView,
        },
      });
      if (requestId !== rowsRequest.current) return;
      const lastPage = Math.max(0, Math.ceil(response.total / resultsPageSize) - 1);
      if (pageIndex > lastPage) {
        setPage(lastPage);
        return;
      }
      setRows(response);
    } catch (caught) {
      if (requestId === rowsRequest.current) setError(errorMessage(caught));
    } finally {
      if (requestId === rowsRequest.current) {
        setRowsLoading(false);
        setStartupReady(true);
      }
    }
  }, [
    desktopRuntime,
    activeSegment,
    globalSearch,
    pageIndex,
    selectedView,
    setError,
    setRows,
    setPage,
    sortBy,
    sortDir,
  ]);

  const loadUrlTree = useCallback(
    async (silent = false) => {
      if (!desktopRuntime) {
        return;
      }

      if (!silent) {
        setUrlTreeLoading(true);
      }
      try {
        const response = await invoke<UrlTreeResponse>("get_url_tree", {
          query: {
            offset: 0,
            limit: 10_000,
            globalSearch,
            ...segmentQuery(activeSegment),
            sortBy,
            sortDir,
            view: selectedView,
          },
        });
        setUrlTree(response);
      } catch (caught) {
        setError(errorMessage(caught));
      } finally {
        if (!silent) {
          setUrlTreeLoading(false);
        }
      }
    },
    [desktopRuntime, activeSegment, globalSearch, selectedView, setError, sortBy, sortDir],
  );

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

  const loadSearchConsoleStatus = useCallback(async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const status = await invoke<SearchConsoleCredentialStatus>(
        "get_search_console_credential_status",
      );
      setSearchConsoleStatus(status);
      setSearchConsoleSiteUrl((current) => current || status.siteUrl || "");
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }, [desktopRuntime, setError]);

  const loadDatabaseLocation = useCallback(async () => {
    if (!desktopRuntime) {
      return;
    }
    try {
      const location = await invoke<DatabaseLocation>("get_database_location");
      setDatabasePath(location.path);
      setCustomDatabasePath((current) => current || location.path);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }, [desktopRuntime, setError]);

  const loadRecoveryState = useCallback(async () => {
    if (!desktopRuntime) {
      return undefined;
    }
    try {
      const state = await invoke<CrawlRecoveryState>("get_recovery_state");
      setRecoveryState(state);
      if (storageMode === "database" && state.recoverable) {
        setResumeCrawl(true);
      }
      return state;
    } catch (caught) {
      setError(errorMessage(caught));
      return undefined;
    }
  }, [desktopRuntime, setError, setResumeCrawl, storageMode]);

  const loadLinkReport = useCallback(
    async (report: LinkReportKind) => {
      if (!desktopRuntime) {
        return;
      }

      setLinkReportLoading(true);
      const requestId = ++linkReportRequest.current;
      const acceptPage = (total: number) => {
        if (requestId !== linkReportRequest.current) return false;
        const lastPage = Math.max(0, Math.ceil(total / resultsPageSize) - 1);
        if (linkReportPage > lastPage) { setLinkReportPage(lastPage); return false; }
        return true;
      };
      try {
        if (report === "redirects") {
          const response = await invoke<GridResponse>("get_rows", {
            query: {
              offset: linkReportPage * resultsPageSize,
              limit: resultsPageSize,
              globalSearch: linkReportSearch.trim() || null,
              sortBy: "finalUrl",
              sortDir: "asc",
              view: "status3xx",
            },
          });
          if (!acceptPage(response.total)) return;
          setRedirectReportRows(response.rows);
          setRedirectReportTotal(response.total);
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setAnchorTextRows([]);
          setAnchorTextTotal(0);
          setSitemapValidationRows([]);
          setSitemapValidationTotal(0);
          return;
        }

        if (report === "anchorText") {
          const response = await invoke<AnchorTextResponse>("get_anchor_texts", {
            query: {
              offset: linkReportPage * resultsPageSize,
              limit: resultsPageSize,
              globalSearch: linkReportSearch.trim() || null,
              sortBy: anchorTextSortBy,
              sortDir: anchorTextSortDir,
              view: "all",
              internalOnly: false,
            },
          });
          if (!acceptPage(response.total)) return;
          setAnchorTextRows(response.rows);
          setAnchorTextTotal(response.total);
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setRedirectReportRows([]);
          setRedirectReportTotal(0);
          setSitemapValidationRows([]);
          setSitemapValidationTotal(0);
          return;
        }

        if (report === "sitemapValidation") {
          const response = await invoke<SitemapValidationResponse>(
            "get_sitemap_validation",
            {
              query: {
                offset: linkReportPage * resultsPageSize,
                limit: resultsPageSize,
                globalSearch: linkReportSearch.trim() || null,
                sortBy: sitemapValidationSortBy,
                sortDir: sitemapValidationSortDir,
              },
            },
          );
          if (!acceptPage(response.total)) return;
          setSitemapValidationRows(response.rows);
          setSitemapValidationTotal(response.total);
          setLinkEdges([]);
          setLinkEdgeTotal(0);
          setAnchorTextRows([]);
          setAnchorTextTotal(0);
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
          setSitemapValidationRows([]);
          setSitemapValidationTotal(0);
          return;
        }

        const response = await invoke<LinkEdgeResponse>("get_link_edges", {
          query: {
            offset: linkReportPage * resultsPageSize,
            limit: resultsPageSize,
            globalSearch: linkReportSearch.trim() || null,
            sortBy: linkReportSortBy,
            sortDir: linkReportSortDir,
            view: linkReportEdgeView(report),
            sourceUrl: report === "selectedOutlinks" ? selectedUrl : null,
            targetUrl: report === "selectedInlinks" ? selected?.url : null,
            internalOnly: false,
          },
        });
        if (!acceptPage(response.total)) return;
        setLinkEdges(response.edges);
        setLinkEdgeTotal(response.total);
        setAnchorTextRows([]);
        setAnchorTextTotal(0);
        setRedirectReportRows([]);
        setRedirectReportTotal(0);
        setSitemapValidationRows([]);
        setSitemapValidationTotal(0);
      } catch (caught) {
        if (requestId === linkReportRequest.current) setError(errorMessage(caught));
      } finally {
        if (requestId === linkReportRequest.current) setLinkReportLoading(false);
      }
    },
    [
      desktopRuntime,
      anchorTextSortBy,
      anchorTextSortDir,
      linkReportSearch,
      linkReportPage,
      linkReportSortBy,
      linkReportSortDir,
      selectedUrl,
      selected?.url,
      setError,
      sitemapValidationSortBy,
      sitemapValidationSortDir,
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

  const loadSelectedCrawlPath = useCallback(
    async (silent = false) => {
      if (!desktopRuntime || !selectedUrl) {
        setSelectedCrawlPath(undefined);
        return;
      }

      if (!silent) {
        setCrawlPathLoading(true);
      }
      try {
        const response = await invoke<CrawlPathResponse>("get_crawl_path", {
          query: {
            targetUrl: selectedUrl,
            maxEdges: 100_000,
            internalOnly: true,
          },
        });
        setSelectedCrawlPath(response);
      } catch (caught) {
        setError(errorMessage(caught));
      } finally {
        if (!silent) {
          setCrawlPathLoading(false);
        }
      }
    },
    [desktopRuntime, selectedUrl, setError],
  );

  const openBrokenLinkReport = useCallback(() => {
    setSelectedLinkReport("broken");
    setLinkReportSearch("");
    setLinkReportsOpen(true);
  }, []);

  const openRedirectReport = useCallback(() => {
    setSelectedLinkReport("redirects");
    setLinkReportSearch("");
    setLinkReportsOpen(true);
  }, []);

  useEffect(() => {
    void loadRows();
    return () => { rowsRequest.current += 1; };
  }, [loadRows]);

  useEffect(() => {
    if (!desktopRuntime || !startupReady) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        unlisten = await listen("quit-requested", requestQuit);
        if (disposed) { unlisten(); return; }
        await invoke("complete_startup");
      } catch (caught) {
        if (!disposed) setError(errorMessage(caught));
      }
    })();
    return () => { disposed = true; unlisten?.(); };
  }, [desktopRuntime, startupReady, requestQuit, setError]);

  useEffect(() => {
    if (!desktopRuntime || !startupReady) return;
    const timer = window.setTimeout(() => void checkForUpdates(false), 2000);
    return () => window.clearTimeout(timer);
  }, [desktopRuntime, startupReady, checkForUpdates]);

  useEffect(() => {
    if (!running || !desktopRuntime) return;
    let disposed = false;
    let timer: number;
    const refresh = async () => {
      await loadRows();
      if (!disposed) timer = window.setTimeout(refresh, 750);
    };
    timer = window.setTimeout(refresh, 750);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [desktopRuntime, running, loadRows]);

  useEffect(() => {
    parentRef.current?.scrollTo({ top: 0 });
  }, [pageIndex, selectedView, globalSearch, sortBy, sortDir, activeSegment]);

  useEffect(() => {
    if (resultsViewMode === "tree") {
      void loadUrlTree();
    }
  }, [loadUrlTree, resultsViewMode]);

  useEffect(() => {
    if (resultsViewMode !== "tree" || !running) {
      return;
    }

    const intervalId = window.setInterval(() => {
      void loadUrlTree(true);
    }, 1_500);

    return () => window.clearInterval(intervalId);
  }, [loadUrlTree, resultsViewMode, running]);

  useEffect(() => {
    if (settingsOpen) {
      void loadSessions();
      void loadProfiles();
      void loadSearchConsoleStatus();
      void loadDatabaseLocation();
      void loadRecoveryState();
    }
  }, [
    loadDatabaseLocation,
    loadProfiles,
    loadRecoveryState,
    loadSearchConsoleStatus,
    loadSessions,
    settingsOpen,
  ]);

  useEffect(() => {
    setLinkReportPage(0);
  }, [selectedLinkReport, selectedUrl, selected?.url, linkReportSearch, linkReportSortBy, linkReportSortDir,
    anchorTextSortBy, anchorTextSortDir, sitemapValidationSortBy, sitemapValidationSortDir]);

  useEffect(() => {
    if (linkReportsOpen) {
      void loadLinkReport(selectedLinkReport);
    }
    return () => { linkReportRequest.current += 1; };
  }, [linkReportsOpen, loadLinkReport, selectedLinkReport]);

  useEffect(() => {
    if (!desktopRuntime || !selectedUrl) {
      setSelectedImages([]);
      setSelectedImageTotal(0);
      setSelectedCrawlPath(undefined);
      return;
    }

    let cancelled = false;
    void (async () => {
      try {
        const response = await invoke<ImageAssetResponse>("get_image_assets", {
          query: {
            offset: 0,
            limit: 12,
            sortBy: "sourcePosition",
            sortDir: "asc",
            pageUrl: selectedUrl,
            oversizedOnly: false,
            missingAltOnly: false,
          },
        });
        if (!cancelled) {
          setSelectedImages(response.images);
          setSelectedImageTotal(response.total);
        }
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught));
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [desktopRuntime, progress?.crawled, selectedUrl, setError]);

  useEffect(() => {
    void loadSelectedCrawlPath();
  }, [loadSelectedCrawlPath]);

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
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const followSystem = () => {
      if (useAppStore.getState().theme === "system") setTheme("system");
    };
    followSystem();
    media.addEventListener("change", followSystem);
    return () => media.removeEventListener("change", followSystem);
  }, [setTheme]);

  useEffect(() => {
    savePreference(urlSegmentsStorageKey, JSON.stringify(urlSegments));
    if (
      activeSegmentId !== "all" &&
      !urlSegments.some((segment) => segment.id === activeSegmentId)
    ) {
      setActiveSegmentId("all");
    }
  }, [activeSegmentId, urlSegments]);

  useEffect(() => {
    const trimmedUrl = config.startUrl.trim();
    if (trimmedUrl.length > 0) {
      savePreference(lastUrlStorageKey, trimmedUrl);
    }
  }, [config.startUrl]);

  const onCrawlEvent = useEffectEvent((payload: CrawlerEvent) => {
    if (payload.kind === "started") {
      setRunning(true);
      setPaused(false);
      setError(undefined);
      setNotice(undefined);
    }
    const terminal = payload.kind === "finished" || payload.kind === "failed";
    if (terminal) {
      setRunning(false);
      setPaused(false);
    }
    if (payload.kind === "error" || payload.kind === "failed") {
      setError(payload.message ?? "Crawler error");
    }
    if (payload.progress) {
      setProgress(payload.progress);
      appendProgressSample(payload.progress);
    }
    if (payload.record && selected?.id === payload.record.id) setSelected(payload.record);
    if (terminal) {
      void loadRows();
      void loadRecoveryState();
      if (resultsViewMode === "tree") void loadUrlTree(true);
      if (graphOpen) void loadGraph(true);
    }
  });

  useEffect(() => {
    if (!desktopRuntime) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<CrawlerEvent>("crawl-event", (event) => {
      if (!disposed) onCrawlEvent(event.payload);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch((caught) => setError(errorMessage(caught)));
    return () => { disposed = true; unlisten?.(); };
  }, [desktopRuntime, setError]);

  const startCrawl = async () => {
    if (!desktopRuntime) {
      setError("Open the desktop app with make dev to run crawls.");
      return;
    }

    setError(undefined);
    setNotice(undefined);
    setRunning(true);
    setPaused(false);
    setProgressHistory([]);
    setProgress(undefined);
    setSelected(undefined);
    try {
      await invoke("start_crawl", { config, storageMode, resume: resumeCrawl });
      setPage(0);
      await loadRows();
      await loadRecoveryState();
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
      await loadRecoveryState();
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
    savePreference(overviewWidthStorageKey, String(nextWidth));
  }, []);

  const setOverviewPanelWidth = useCallback((width: number) => {
    const nextWidth = clamp(width, overviewMinWidth, overviewMaxWidth);
    setOverviewWidth(nextWidth);
    savePreference(overviewWidthStorageKey, String(nextWidth));
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
    if (paused) {
      void resumeActiveCrawl();
      return;
    }
    void pauseCrawl();
  };

  const resetFilters = () => {
    setView("all");
    setActiveIssueGroup(issueGroups[0]);
    setSearch("");
    setActiveSegmentId("all");
  };

  const selectAuditView = (view: IssueView) => {
    setView(view);
    setActiveIssueGroup(issueGroups.find((group) => group.views.includes(view)) ?? issueGroups[0]);
    if (window.innerWidth <= 1200) setIssuesOpen(false);
    if (window.innerWidth <= 1000) setOverviewOpen(false);
  };

  const addUrlSegment = () => {
    const segment: UrlSegment = {
      id: newSegmentId(),
      name: "New Segment",
      pattern: "/blog",
      regex: false,
    };
    setUrlSegments((segments) => [...segments, segment]);
    setActiveSegmentId(segment.id);
  };

  const updateUrlSegment = (id: string, patch: Partial<UrlSegment>) => {
    setUrlSegments((segments) =>
      segments.map((segment) =>
        segment.id === id ? { ...segment, ...patch } : segment,
      ),
    );
  };

  const removeUrlSegment = (id: string) => {
    setUrlSegments((segments) => segments.filter((segment) => segment.id !== id));
    if (activeSegmentId === id) {
      setActiveSegmentId("all");
    }
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

  const addCustomSearch = () => {
    setConfig({
      customSearches: [
        ...config.customSearches,
        {
          name: `search_${config.customSearches.length + 1}`,
          pattern: "analytics",
          regex: false,
          caseSensitive: false,
          maxSnippets: 3,
        },
      ],
    });
  };

  const updateCustomSearch = (index: number, patch: Partial<CustomSearch>) => {
    setConfig({
      customSearches: config.customSearches.map((customSearch, currentIndex) =>
        currentIndex === index ? { ...customSearch, ...patch } : customSearch,
      ),
    });
  };

  const removeCustomSearch = (index: number) => {
    setConfig({
      customSearches: config.customSearches.filter(
        (_, currentIndex) => currentIndex !== index,
      ),
    });
  };

  const importListUrlsFromFile = async (file?: File | null) => {
    if (!file) {
      return;
    }
    try {
      const text = await file.text();
      const importedUrls = extractUrlsFromText(text);
      if (importedUrls.length === 0) {
        setNotice(`No URLs found in ${file.name}`);
        return;
      }
      setConfig({
        mode: "list",
        listUrls: mergeUniqueValues(config.listUrls, importedUrls),
      });
      setNotice(
        `Imported ${importedUrls.length.toLocaleString()} URL${
          importedUrls.length === 1 ? "" : "s"
        } from ${file.name}`,
      );
    } catch (caught) {
      setError(errorMessage(caught));
    }
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

  const updateRendering = (patch: Partial<JsRenderingConfig>) => {
    setConfig({
      rendering: {
        ...config.rendering,
        ...patch,
      },
    });
  };

  const applySafePreset = () => {
    setConfig({
      concurrency: 4,
      requestsPerSecond: 2,
      requestDelayMs: 250,
      retryAttempts: 1,
      retryBackoffMs: 250,
      respectRobots: true,
      useRobotsTxtOverride: false,
      followNofollow: true,
    });
  };

  const applyBenchmarkPreset = () => {
    setConfig({
      maxUrls: Math.max(config.maxUrls, 50_000),
      maxDepth: Math.max(config.maxDepth, 10),
      concurrency: 64,
      requestsPerSecond: 0,
      requestDelayMs: 0,
      retryAttempts: 0,
      retryBackoffMs: 0,
      respectRobots: false,
      useRobotsTxtOverride: false,
      followNofollow: true,
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

  const testRobotsTxtBatch = async () => {
    const urls = textToPatterns(robotsBatchUrls);
    if (!desktopRuntime || urls.length === 0 || !config.robotsTxtOverride.trim()) {
      return;
    }
    try {
      const result = await invoke<RobotsTxtBatchTestResult>("test_robots_txt_batch", {
        request: {
          userAgent: config.userAgent,
          robotsTxt: config.robotsTxtOverride,
          urls,
        },
      });
      setRobotsBatchResult(result);
    } catch (caught) {
      setRobotsBatchResult(undefined);
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
      setDatabasePath(session.databasePath);
      setCustomDatabasePath(session.databasePath);
      setNewSessionName("");
      await loadSessions();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const resetCrawlResults = async () => {
    setSelected(undefined);
    setProgress(undefined);
    setProgressHistory([]);
    setPage(0);
    await loadRows();
    if (resultsViewMode === "tree") await loadUrlTree();
  };

  const openSession = async (sessionId: string) => {
    if (!desktopRuntime || !sessionId || running) {
      return;
    }
    try {
      const session = await invoke<CrawlSession>("open_crawl_session", { sessionId });
      setStorageMode("database");
      setResumeCrawl(true);
      setSelectedSessionId(session.id);
      setDatabasePath(session.databasePath);
      setCustomDatabasePath(session.databasePath);
      if (session.startUrl.trim()) {
        setConfig({ startUrl: session.startUrl });
      }
      await loadSessions();
      await resetCrawlResults();
      const recovery = await loadRecoveryState();
      if (recovery?.recoverable) {
        setNotice(
          `Opened recoverable crawl state with ${recovery.queued.toLocaleString()} queued URLs.`,
        );
      }
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
      await loadDatabaseLocation();
      await resetCrawlResults();
      await loadRecoveryState();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const openCustomDatabasePath = async () => {
    if (!desktopRuntime || !customDatabasePath.trim() || running) {
      return;
    }
    try {
      const location = await invoke<DatabaseLocation>("open_database_path", {
        path: customDatabasePath.trim(),
      });
      setStorageMode("database");
      setSelectedSessionId("");
      setDatabasePath(location.path);
      setCustomDatabasePath(location.path);
      await resetCrawlResults();
      const recovery = await loadRecoveryState();
      setNotice(
        recovery?.recoverable
          ? `Opened database ${location.path}. Recoverable crawl state has ${recovery.queued.toLocaleString()} queued URLs.`
          : `Opened database ${location.path}`,
      );
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
      setConfig(normalizeCrawlConfig(profile.config));
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

  const saveSearchConsoleCredentials = async () => {
    if (!desktopRuntime || !searchConsoleSiteUrl.trim()) {
      return;
    }
    setSearchConsoleLoading(true);
    setSearchConsoleTestResult(undefined);
    setSearchConsoleMergeResult(undefined);
    try {
      const status = await invoke<SearchConsoleCredentialStatus>(
        "save_search_console_credentials",
        {
          request: {
            siteUrl: searchConsoleSiteUrl.trim(),
            accessToken: searchConsoleAccessToken.trim() || null,
          },
        },
      );
      setSearchConsoleStatus(status);
      setSearchConsoleAccessToken("");
      setNotice("Google Search Console credentials saved.");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSearchConsoleLoading(false);
    }
  };

  const clearSearchConsoleCredentials = async () => {
    if (!desktopRuntime) {
      return;
    }
    setSearchConsoleLoading(true);
    setSearchConsoleTestResult(undefined);
    setSearchConsoleMergeResult(undefined);
    try {
      const status = await invoke<SearchConsoleCredentialStatus>(
        "clear_search_console_credentials",
      );
      setSearchConsoleStatus(status);
      setSearchConsoleSiteUrl("");
      setSearchConsoleAccessToken("");
      setNotice("Google Search Console credentials cleared.");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSearchConsoleLoading(false);
    }
  };

  const testSearchConsoleCredentials = async () => {
    if (!desktopRuntime) {
      return;
    }
    setSearchConsoleLoading(true);
    setSearchConsoleTestResult(undefined);
    setSearchConsoleMergeResult(undefined);
    try {
      const result = await invoke<SearchConsoleTestResult>(
        "test_search_console_credentials",
        {
          request: {
            startDate: searchConsoleStartDate,
            endDate: searchConsoleEndDate,
            rowLimit: searchConsoleRowLimit,
          },
        },
      );
      setSearchConsoleTestResult(result);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSearchConsoleLoading(false);
    }
  };

  const mergeSearchConsoleMetrics = async () => {
    if (!desktopRuntime) {
      return;
    }
    setSearchConsoleLoading(true);
    setSearchConsoleTestResult(undefined);
    setSearchConsoleMergeResult(undefined);
    try {
      const result = await invoke<SearchConsoleMergeResult>(
        "merge_search_console_metrics",
        {
          request: {
            startDate: searchConsoleStartDate,
            endDate: searchConsoleEndDate,
            rowLimit: searchConsoleRowLimit,
          },
        },
      );
      setSearchConsoleMergeResult(result);
      await loadRows();
      setNotice(
        `Merged ${result.matchedRows.toLocaleString()} Search Console rows into the current crawl.`,
      );
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSearchConsoleLoading(false);
    }
  };

  const currentGridExportQuery = () => ({
    offset: 0,
    limit: 1_000_000,
    globalSearch,
    ...segmentQuery(activeSegment),
    sortBy,
    sortDir,
    view: selectedView,
  });

  const exportFile = async (kind: ExportKind) => {
    if (!desktopRuntime) {
      setError("Open the desktop app with make dev to export files.");
      return;
    }
    try {
      setError(undefined);
      const result = await invoke<ExportFileResult>("export_file", {
        request: {
          kind,
          query:
            kind === "csv" || kind === "xlsx" || kind === "sitemap"
              ? currentGridExportQuery()
              : null,
          graphQuery:
            kind === "graphJson" || kind === "graphNodesCsv" || kind === "graphEdgesCsv"
              ? { maxNodes: 5_000, maxEdges: 10_000, internalOnly: false }
              : null,
        },
      });
      setNotice(
        `Exported ${result.rowCount.toLocaleString()} row${
          result.rowCount === 1 ? "" : "s"
        } to ${result.path}`,
      );
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const importCrawlArchive = async () => {
    if (!desktopRuntime || !archiveImportPath.trim() || running) {
      return;
    }
    try {
      const result = await invoke<CrawlArchiveImportResult>("import_crawl_archive", {
        request: {
          path: archiveImportPath.trim(),
          storageMode,
        },
      });
      setNotice(
        `Imported ${result.records.toLocaleString()} URLs, ${result.linkEdges.toLocaleString()} links, ${result.imageAssets.toLocaleString()} images, and ${result.frontierItems.toLocaleString()} frontier items.`,
      );
      await resetCrawlResults();
      const recovery = await loadRecoveryState();
      if (storageMode === "database" && recovery?.recoverable) {
        setResumeCrawl(true);
      }
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const compareCrawlArchive = async () => {
    if (!desktopRuntime || !comparisonArchivePath.trim()) {
      return;
    }
    setComparisonLoading(true);
    setComparisonResult(undefined);
    try {
      const result = await invoke<CrawlComparisonResponse>("compare_crawl_archive", {
        request: { path: comparisonArchivePath.trim() },
      });
      setComparisonResult(result);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setComparisonLoading(false);
    }
  };

  const openSelectedUrl = async () => {
    if (!desktopRuntime || !selected?.finalUrl) {
      return;
    }
    try {
      await invoke("open_external_url", { url: selected.finalUrl });
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const openSourceUrl = async () => {
    if (!desktopRuntime || !selected?.firstInlinkSourceUrl) {
      return;
    }
    try {
      await invoke("open_external_url", { url: selected.firstInlinkSourceUrl });
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const copyText = async (value: string, label: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(value);
      } else {
        const textArea = document.createElement("textarea");
        textArea.value = value;
        textArea.style.position = "fixed";
        textArea.style.left = "-9999px";
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        document.execCommand("copy");
        document.body.removeChild(textArea);
      }
      setNotice(`${label} copied.`);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const copySelectedUrl = async () => {
    if (selected?.finalUrl) {
      await copyText(selected.finalUrl, "URL");
    }
  };

  const copySourceUrl = async () => {
    if (selected?.firstInlinkSourceUrl) {
      await copyText(selected.firstInlinkSourceUrl, "Source URL");
    }
  };

  const confirmQuit = async () => {
    if (quitting || !desktopRuntime) return;
    setQuitting(true);
    setQuitError(undefined);
    try {
      await invoke("quit_app");
    } catch (caught) {
      setQuitError(errorMessage(caught));
      setQuitting(false);
    }
  };

  return (
    <main className="app-shell">
      <header className="toolbar">
        <div className="brand">
          <img className="brand-mark" src="/brand/ferrous-frog.png" width="28" height="28" alt="" />
          <div>
            <h1>Ferrous Frog</h1>
            <p>SEO Spider</p>
          </div>
        </div>
        <form id="crawl-target" className="url-control" noValidate={config.mode === "list"} onSubmit={(event) => {
          event.preventDefault();
          if (!running && desktopRuntime && hasCrawlTarget) void startCrawl();
        }}>
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
            type="url"
            required={config.mode === "spider"}
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
        </form>
        <div className="crawl-controls">
          <button
            className="primary"
            type={running ? "button" : "submit"}
            form="crawl-target"
            onClick={running ? primaryCrawlAction : undefined}
            disabled={!desktopRuntime || (!running && !hasCrawlTarget)}
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
              <button className="export-trigger" aria-label="Export crawl data" title="Export crawl data" disabled={exportDisabled}>
                <Download size={16} />
                <span>Export</span>
                <ChevronDown size={14} />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content className="dropdown-content" align="end" sideOffset={8}>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={filteredExportDisabled}
                  onSelect={() => void exportFile("csv")}
                >
                  CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={filteredExportDisabled}
                  onSelect={() => void exportFile("xlsx")}
                >
                  XLSX
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={filteredExportDisabled}
                  onSelect={() => void exportFile("sitemap")}
                >
                  XML Sitemap
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("linkEdgesCsv")}
                >
                  Link Edges CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("redirectChainsCsv")}
                >
                  Redirect Chains CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("sitemapValidationCsv")}
                >
                  Sitemap Validation CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("htmlReport")}
                >
                  HTML Report
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("graphJson")}
                >
                  Graph JSON
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("graphNodesCsv")}
                >
                  Graph Nodes CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("graphEdgesCsv")}
                >
                  Graph Edges CSV
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item"
                  disabled={exportDisabled}
                  onSelect={() => void exportFile("crawlArchive")}
                >
                  Crawl Archive
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
          <button className="settings-trigger" aria-label="Crawl settings" title="Crawl settings" onClick={() => setSettingsOpen(true)}>
            <Settings size={16} /><span>Settings</span>
          </button>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button className="compact-menu-trigger" title="More tools" aria-label="More tools">
                <MoreHorizontal size={16} />
                <span>More</span>
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content className="dropdown-content toolbar-menu" align="end" sideOffset={8}
                onCloseAutoFocus={(event) => { if (cancelQuitRef.current) event.preventDefault(); }}>
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
                  onSelect={() => setComparisonOpen(true)}
                >
                  <FileText size={15} />
                  <span>Crawl Comparison</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  disabled={!desktopRuntime}
                  onSelect={() => setGraphOpen(true)}
                >
                  <GitFork size={15} />
                  <span>Crawl Graph</span>
                </DropdownMenu.Item>
                <DropdownMenu.Separator className="dropdown-separator" />
                <DropdownMenu.Label className="dropdown-label">Appearance</DropdownMenu.Label>
                <DropdownMenu.RadioGroup value={theme} onValueChange={(value) => setTheme(value as ThemePreference)} aria-label="Color theme">
                  {([
                    { value: "system", label: "System Theme", icon: Monitor },
                    { value: "light", label: "Light Theme", icon: Sun },
                    { value: "dark", label: "Dark Theme", icon: Moon },
                  ] as const).map(({ value, label, icon: Icon }) => (
                    <DropdownMenu.RadioItem key={value} value={value} className="dropdown-item toolbar-menu-item">
                      <Icon size={15} /><span>{label}</span>
                      {value === "system" ? <small>{theme === "system" ? resolvedTheme === "dark" ? "Dark" : "Light" : "Auto"}</small> : null}
                      <DropdownMenu.ItemIndicator className="theme-check"><Check size={15} /></DropdownMenu.ItemIndicator>
                    </DropdownMenu.RadioItem>
                  ))}
                </DropdownMenu.RadioGroup>
                <DropdownMenu.Separator className="dropdown-separator" />
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  onSelect={() => setAboutOpen(true)}
                >
                  <Info size={15} />
                  <span>About</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  disabled={!desktopRuntime}
                  onSelect={() => void checkForUpdates(true)}
                >
                  <RefreshCw size={15} />
                  <span>Check for updates</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="dropdown-item toolbar-menu-item"
                  disabled={!desktopRuntime}
                  onSelect={requestQuit}
                >
                  <LogOut size={15} />
                  <span>Quit</span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
      </header>

      <Dialog.Root open={quitOpen} onOpenChange={(open) => { if (!quitting) { setQuitOpen(open); setQuitError(undefined); } }}>
        <Dialog.Portal>
          <Dialog.Overlay className="modal-backdrop quit-backdrop" />
          <Dialog.Content
            className="quit-modal"
            role="alertdialog"
            onOpenAutoFocus={(event) => { event.preventDefault(); cancelQuitRef.current?.focus(); }}
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              const origin = quitOriginRef.current;
              (origin?.isConnected ? origin : document.querySelector<HTMLButtonElement>('[aria-label="More tools"]'))?.focus();
            }}
            onInteractOutside={(event) => event.preventDefault()}
            onEscapeKeyDown={(event) => { if (quitting) event.preventDefault(); }}
          >
            <Dialog.Title>Quit Ferrous Frog?</Dialog.Title>
            <Dialog.Description>
              {running ? "A crawl is running. Quitting will stop it." : "Are you sure you want to quit?"}
              {storageMode === "memory" && (summary.total > 0 || running) ? " In-memory results will be lost. Export any results you need before quitting." : ""}
            </Dialog.Description>
            {quitError ? <p className="error-bar" role="alert">{quitError}</p> : null}
            {quitting ? <p role="status">Stopping the crawl and closing…</p> : null}
            <div className="quit-actions">
              <Dialog.Close asChild><button ref={cancelQuitRef} disabled={quitting}>No</button></Dialog.Close>
              <button className="destructive" disabled={quitting} onClick={() => void confirmQuit()}>Yes</button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <Dialog.Root
        open={updateOpen && !quitOpen && !settingsOpen && !aboutOpen && !comparisonOpen && !linkReportsOpen && !graphOpen}
        onOpenChange={(open) => { if (!open) dismissUpdate(); }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="modal-backdrop" />
          <Dialog.Content
            className="about-modal update-modal"
            onOpenAutoFocus={(event) => { event.preventDefault(); updateDismissRef.current?.focus(); }}
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              if (quitOpen || settingsOpen || aboutOpen || comparisonOpen || linkReportsOpen || graphOpen) return;
              const origin = updateOriginRef.current;
              (origin?.isConnected ? origin : document.querySelector<HTMLButtonElement>('[aria-label="More tools"]'))?.focus();
            }}
          >
            <div className="update-heading">
              <img src="/brand/ferrous-frog.png" width="56" height="56" alt="" />
              <div>
                <p>Ferrous Frog</p>
                <Dialog.Title>{checkingUpdates ? "Checking for updates…" : updateResult?.update ? "Update available" : updateError ? "Update check failed" : "No updates available"}</Dialog.Title>
              </div>
              <Dialog.Close asChild><button className="update-close" aria-label="Close update notice"><X size={16} /></button></Dialog.Close>
            </div>
            <Dialog.Description>
              {checkingUpdates ? "Checking GitHub for the latest stable release." : updateResult?.update ? "A new version of Ferrous Frog is ready to download." : updateError ? "The update check could not be completed." : "No newer stable release has been published."}
            </Dialog.Description>
            {checkingUpdates ? <progress aria-label="Checking for updates" /> : null}
            {updateResult ? (
              <dl className="update-versions" data-state={updateResult.update ? "available" : "current"}>
                <div><dt>Installed version</dt><dd>{updateResult.currentVersion}</dd></div>
                {updateResult.update ? <div><dt>Available version</dt><dd className="update-version-new">{updateResult.update.version}</dd></div> : null}
              </dl>
            ) : null}
            {updateResult?.update ? <p className="update-note">Release notes and installers open on GitHub in your browser.</p> : null}
            {updateError ? <p className="error-bar" role="alert">{updateError}</p> : null}
            <div className="update-actions">
              <button ref={updateDismissRef} data-action="remind" onClick={dismissUpdate} title={updateResult?.update ? "Remind me in 24 hours" : undefined}>
                {updateResult?.update ? "Remind me later" : "Close"}
              </button>
              {updateResult?.update ? (
                <button className="primary" data-action="download" disabled={openingUpdate} onClick={() => void openUpdateDownload()}>
                  <Download size={16} />{openingUpdate ? "Opening…" : "Download update"}
                </button>
              ) : updateError ? <button className="primary" data-action="retry" onClick={() => void checkForUpdates(true)}>Try again</button> : null}
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

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
                <span>Benchmark preset</span>
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
              <div>
                <Dialog.Title asChild>
                  <h2 id="settings-title">Crawl Settings</h2>
                </Dialog.Title>
                <Dialog.Description className="settings-save-note">Crawl options are saved automatically on this device.</Dialog.Description>
              </div>
              <Dialog.Close asChild>
                <button title="Close settings">
                  <X size={16} />
                </button>
              </Dialog.Close>
              <FeedbackMessages />
            </div>

            <div className="settings-layout">
              <aside className="settings-sidebar">
                <span className="settings-sidebar-label">Sections</span>
                <nav className="settings-tabs" aria-label="Settings sections">
                  {settingsTabs.map((tab) => (
                    <button
                      key={tab.id}
                      className={`settings-tab-button ${
                        settingsTab === tab.id ? "active" : ""
                      }`}
                      onClick={() => setSettingsTab(tab.id)}
                      aria-current={settingsTab === tab.id ? "page" : undefined}
                    >
                      <span>{tab.label}</span>
                    </button>
                  ))}
                </nav>
              </aside>

              <div className="settings-content">
                <div className="settings-panel-heading">
                  <div>
                    <p>Settings</p>
                    <h3>{activeSettingsTab.label}</h3>
                    <span>{activeSettingsTab.description}</span>
                  </div>
                </div>
            <section className="settings-section" hidden={settingsTab !== "crawl"}>
              <h3>Limits and Throughput</h3>
              <div className="settings-grid">
                <div className="settings-preset-row settings-wide">
                  <button
                    className="settings-action-button secondary"
                    onClick={applySafePreset}
                    type="button"
                  >
                    Safe preset
                  </button>
                  <button
                    className="settings-action-button warning"
                    onClick={applyBenchmarkPreset}
                    type="button"
                  >
                    Benchmark preset: ignore robots
                  </button>
                </div>
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
                    min={0}
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
                  Retries
                  <input
                    type="number"
                    min={0}
                    max={5}
                    value={config.retryAttempts}
                    onChange={(event) => setConfig({ retryAttempts: Number(event.target.value) })}
                  />
                </label>
                <label>
                  Backoff ms
                  <input
                    type="number"
                    min={0}
                    max={30000}
                    value={config.retryBackoffMs}
                    onChange={(event) => setConfig({ retryBackoffMs: Number(event.target.value) })}
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
                <div className="robots-test-row settings-wide">
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
                  <button
                    className="settings-action-button secondary"
                    onClick={() => void downloadRobotsTxt()}
                    disabled={!desktopRuntime || !config.startUrl.trim()}
                  >
                    <Download size={15} />
                    <span>Download</span>
                  </button>
                  <button
                    className="settings-action-button primary"
                    onClick={() => void testRobotsTxt()}
                    disabled={
                      !desktopRuntime ||
                      !config.respectRobots ||
                      !config.useRobotsTxtOverride ||
                      !robotsTestUrl.trim() ||
                      !config.robotsTxtOverride.trim()
                    }
                  >
                    <Search size={15} />
                    <span>Test</span>
                  </button>
                  <span className="settings-result">{robotsTestResult ?? "No result"}</span>
                </div>
                <label className="settings-wide">
                  Batch robots test URLs
                  <textarea
                    rows={4}
                    disabled={!config.respectRobots || !config.useRobotsTxtOverride}
                    value={robotsBatchUrls}
                    placeholder="https://example.com/path-one&#10;https://example.com/private/path"
                    onChange={(event) => {
                      setRobotsBatchUrls(event.target.value);
                      setRobotsBatchResult(undefined);
                    }}
                  />
                </label>
                <div className="robots-batch-row settings-wide">
                  <button
                    className="settings-action-button primary"
                    onClick={() => void testRobotsTxtBatch()}
                    disabled={
                      !desktopRuntime ||
                      !config.respectRobots ||
                      !config.useRobotsTxtOverride ||
                      !robotsBatchUrls.trim() ||
                      !config.robotsTxtOverride.trim()
                    }
                  >
                    <Search size={15} />
                    <span>Batch Test</span>
                  </button>
                  <span className="settings-result">
                    {robotsBatchResult
                      ? `${robotsBatchResult.allowed.toLocaleString()} allowed, ${robotsBatchResult.blocked.toLocaleString()} blocked, ${robotsBatchResult.invalid.toLocaleString()} invalid`
                      : "No batch result"}
                  </span>
                </div>
                {robotsBatchResult ? (
                  <div className="robots-batch-results settings-wide">
                    {robotsBatchResult.rows.slice(0, 24).map((row, index) => (
                      <div
                        key={`${row.url}-${index}`}
                        className={`robots-batch-result ${
                          row.error ? "invalid" : row.allowed ? "allowed" : "blocked"
                        }`}
                      >
                        <span>{row.error ? "Invalid" : row.allowed ? "Allowed" : "Blocked"}</span>
                        <code>{row.url}</code>
                      </div>
                    ))}
                    {robotsBatchResult.rows.length > 24 ? (
                      <span className="settings-result">
                        Showing 24 of {robotsBatchResult.rows.length.toLocaleString()} tested URLs.
                      </span>
                    ) : null}
                  </div>
                ) : null}
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "scope"}>
              <h3>Scope</h3>
              <div className="settings-grid">
                <label>
                  Subdomains
                  <select
                    value={config.subdomainScope}
                    onChange={(event) =>
                      setConfig({ subdomainScope: event.target.value as SubdomainScope })
                    }
                  >
                    <option value="includeSubdomains">Include subdomains</option>
                    <option value="exactHost">Exact host only</option>
                  </select>
                </label>
                <label>
                  Folder scope
                  <select
                    value={config.folderScope}
                    onChange={(event) =>
                      setConfig({ folderScope: event.target.value as FolderScope })
                    }
                  >
                    <option value="anywhere">Anywhere on host</option>
                    <option value="startFolder">Start folder and children</option>
                    <option value="exactFolder">Exact start folder</option>
                  </select>
                </label>
                <CheckboxField
                  checked={config.folderScope === "anywhere"}
                  onCheckedChange={(checked) =>
                    setConfig({ folderScope: checked ? "anywhere" : "startFolder" })
                  }
                >
                  Crawl outside start folder
                </CheckboxField>
                <CheckboxField
                  checked={config.followNofollow}
                  onCheckedChange={(checked) => setConfig({ followNofollow: checked })}
                >
                  Follow nofollow links
                </CheckboxField>
                <label className="settings-wide">
                  List URLs
                  <textarea
                    aria-label="List URLs"
                    rows={4}
                    value={patternsToText(config.listUrls)}
                    onChange={(event) =>
                      setConfig({ listUrls: textToPatterns(event.target.value) })
                    }
                  />
                </label>
                <label className="settings-wide">
                  Import URL file
                  <input
                    type="file"
                    accept=".txt,.csv,.xml,.html,.log,text/plain,text/csv,application/xml,text/xml"
                    onChange={(event) => {
                      const file = event.currentTarget.files?.[0] ?? null;
                      event.currentTarget.value = "";
                      void importListUrlsFromFile(file);
                    }}
                  />
                </label>
                <label className="settings-wide">
                  List sitemap URLs
                  <textarea
                    rows={3}
                    value={patternsToText(config.listSitemapUrls)}
                    onChange={(event) =>
                      setConfig({ listSitemapUrls: textToPatterns(event.target.value) })
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
                <div className="settings-wide segment-settings">
                  <div className="settings-section-title-row">
                    <div>
                      <h4>URL Segments</h4>
                      <p>Filter any result view by a named URL contains pattern or regex.</p>
                    </div>
                    <button
                      className="settings-action-button secondary"
                      onClick={addUrlSegment}
                      type="button"
                    >
                      <Plus size={15} />
                      <span>Add Segment</span>
                    </button>
                  </div>
                  {urlSegments.length === 0 ? (
                    <p className="settings-empty">No URL segments have been defined.</p>
                  ) : (
                    urlSegments.map((segment) => (
                      <div className="segment-row" key={segment.id}>
                        <label>
                          Name
                          <input
                            value={segment.name}
                            onChange={(event) =>
                              updateUrlSegment(segment.id, { name: event.target.value })
                            }
                          />
                        </label>
                        <label>
                          Pattern
                          <input
                            value={segment.pattern}
                            placeholder="/blog or ^https://example.com/(blog|news)/"
                            onChange={(event) =>
                              updateUrlSegment(segment.id, { pattern: event.target.value })
                            }
                          />
                        </label>
                        <CheckboxField
                          checked={segment.regex}
                          onCheckedChange={(checked) =>
                            updateUrlSegment(segment.id, { regex: checked })
                          }
                        >
                          Regex
                        </CheckboxField>
                        <button
                          className="settings-icon-danger"
                          onClick={() => removeUrlSegment(segment.id)}
                          title="Remove segment"
                          type="button"
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "resources"}>
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

            <section className="settings-section" hidden={settingsTab !== "query"}>
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

            <section className="settings-section" hidden={settingsTab !== "storage"}>
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
                    disabled={storageMode !== "database" || running}
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
                <label className="settings-wide">
                  Current database path
                  <input value={databasePath} readOnly />
                </label>
                <label className="settings-wide">
                  Custom database path
                  <input
                    value={customDatabasePath}
                    disabled={!desktopRuntime || running}
                    placeholder="/path/to/ferrous-frog.sqlite3"
                    onChange={(event) => setCustomDatabasePath(event.target.value)}
                  />
                </label>
                <div className="settings-actions">
                  <button
                    className="settings-action-button primary"
                    onClick={() => void createSession()}
                    disabled={storageMode !== "database"}
                  >
                    <Check size={15} />
                    <span>Save Session</span>
                  </button>
                  <button
                    className="settings-action-button danger"
                    onClick={() => void deleteSession()}
                    disabled={storageMode !== "database" || !selectedSessionId || running}
                  >
                    <Trash2 size={15} />
                    <span>Delete</span>
                  </button>
                  <button
                    className="settings-action-button"
                    onClick={() => void openCustomDatabasePath()}
                    disabled={!desktopRuntime || running || !customDatabasePath.trim()}
                  >
                    <Folder size={15} />
                    <span>Open Database</span>
                  </button>
                </div>
                <label className="settings-wide">
                  Import archive path
                  <input
                    value={archiveImportPath}
                    disabled={!desktopRuntime || running}
                    placeholder="/path/to/ferrous-frog-crawl-archive.ffcrawl.json"
                    onChange={(event) => setArchiveImportPath(event.target.value)}
                  />
                </label>
                <div className="settings-actions settings-wide">
                  <button
                    className="settings-action-button"
                    onClick={() => void importCrawlArchive()}
                    disabled={!desktopRuntime || running || !archiveImportPath.trim()}
                  >
                    <FileText size={15} />
                    <span>Import Archive</span>
                  </button>
                </div>
                <div className={`capacity-estimate ${capacityEstimate.tone}`}>
                  <div>
                    <span>Configured URLs</span>
                    <strong>{capacityEstimate.urlLimit.toLocaleString()}</strong>
                  </div>
                  <div>
                    <span>RAM estimate</span>
                    <strong>{formatBytes(capacityEstimate.ramBytes)}</strong>
                  </div>
                  <div>
                    <span>Disk estimate</span>
                    <strong>{formatBytes(capacityEstimate.diskBytes)}</strong>
                  </div>
                  <div>
                    <span>Suggested mode</span>
                    <strong>{capacityEstimate.recommendation}</strong>
                  </div>
                  {capacityEstimate.deviceBudgetBytes ? (
                    <div>
                      <span>RAM budget</span>
                      <strong>{formatBytes(capacityEstimate.deviceBudgetBytes)}</strong>
                    </div>
                  ) : null}
                </div>
                <div
                  className={`recovery-state ${
                    recoveryState.recoverable ? "warning" : ""
                  }`}
                >
                  <div>
                    <span>Recovery state</span>
                    <strong>
                      {recoveryState.recoverable
                        ? "Queued crawl can be resumed"
                        : "No queued crawl state"}
                    </strong>
                  </div>
                  <div>
                    <span>Queued</span>
                    <strong>{recoveryState.queued.toLocaleString()}</strong>
                  </div>
                  <div>
                    <span>Crawled</span>
                    <strong>{recoveryState.crawled.toLocaleString()}</strong>
                  </div>
                  <div>
                    <span>Seen</span>
                    <strong>{recoveryState.seen.toLocaleString()}</strong>
                  </div>
                </div>
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "profiles"}>
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
                  <button
                    className="settings-action-button primary"
                    onClick={() => void saveProfile()}
                    disabled={running}
                  >
                    <Check size={15} />
                    <span>Save Profile</span>
                  </button>
                  <button
                    className="settings-action-button danger"
                    onClick={() => void deleteProfile()}
                    disabled={!selectedProfileId || running}
                  >
                    <Trash2 size={15} />
                    <span>Delete</span>
                  </button>
                </div>
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "integrations"}>
              <h3>Google Search Console</h3>
              <div className="settings-grid compact">
                <label className="settings-wide">
                  Site URL
                  <input
                    value={searchConsoleSiteUrl}
                    placeholder="https://example.com/ or sc-domain:example.com"
                    onChange={(event) => setSearchConsoleSiteUrl(event.target.value)}
                  />
                </label>
                <label className="settings-wide">
                  Access token
                  <input
                    type="password"
                    value={searchConsoleAccessToken}
                    placeholder={
                      searchConsoleStatus.tokenSaved
                        ? "Token saved in OS credential store"
                        : "Paste OAuth access token"
                    }
                    onChange={(event) => setSearchConsoleAccessToken(event.target.value)}
                  />
                </label>
                <div className="integration-status settings-wide">
                  <span className={searchConsoleStatus.tokenSaved ? "ok" : "muted"}>
                    {searchConsoleStatus.tokenSaved ? "Token saved" : "No saved token"}
                  </span>
                  <span className={searchConsoleStatus.keyringAvailable ? "ok" : "danger"}>
                    {searchConsoleStatus.keyringAvailable
                      ? "OS credential store available"
                      : "OS credential store unavailable"}
                  </span>
                  {searchConsoleStatus.siteUrl ? (
                    <span>{searchConsoleStatus.siteUrl}</span>
                  ) : null}
                  {searchConsoleStatus.message ? (
                    <span className="danger">{searchConsoleStatus.message}</span>
                  ) : null}
                </div>
                <div className="settings-actions settings-wide">
                  <button
                    className="settings-action-button primary"
                    onClick={() => void saveSearchConsoleCredentials()}
                    disabled={
                      searchConsoleLoading ||
                      !desktopRuntime ||
                      !searchConsoleSiteUrl.trim()
                    }
                  >
                    <Check size={15} />
                    <span>Save Credentials</span>
                  </button>
                  <button
                    className="settings-action-button danger"
                    onClick={() => void clearSearchConsoleCredentials()}
                    disabled={searchConsoleLoading || !desktopRuntime}
                  >
                    <Trash2 size={15} />
                    <span>Clear</span>
                  </button>
                </div>
                <label>
                  Start date
                  <input
                    type="date"
                    value={searchConsoleStartDate}
                    onChange={(event) => setSearchConsoleStartDate(event.target.value)}
                  />
                </label>
                <label>
                  End date
                  <input
                    type="date"
                    value={searchConsoleEndDate}
                    onChange={(event) => setSearchConsoleEndDate(event.target.value)}
                  />
                </label>
                <label>
                  Row limit
                  <input
                    type="number"
                    min={1}
                    max={25000}
                    value={searchConsoleRowLimit}
                    onChange={(event) =>
                      setSearchConsoleRowLimit(Number(event.target.value))
                    }
                  />
                </label>
                <div className="settings-actions settings-wide">
                  <button
                    className="settings-action-button secondary"
                    onClick={() => void testSearchConsoleCredentials()}
                    disabled={
                      searchConsoleLoading ||
                      !desktopRuntime ||
                      !searchConsoleStatus.tokenSaved ||
                      !searchConsoleSiteUrl.trim()
                    }
                  >
                    <Search size={15} />
                    <span>{searchConsoleLoading ? "Testing" : "Test Search Analytics"}</span>
                  </button>
                  <button
                    className="settings-action-button primary"
                    onClick={() => void mergeSearchConsoleMetrics()}
                    disabled={
                      searchConsoleLoading ||
                      !desktopRuntime ||
                      !searchConsoleStatus.tokenSaved ||
                      !searchConsoleSiteUrl.trim()
                    }
                  >
                    <Download size={15} />
                    <span>{searchConsoleLoading ? "Fetching" : "Fetch and Merge"}</span>
                  </button>
                </div>
                {searchConsoleTestResult ? (
                  <div className="integration-result settings-wide">
                    <div>
                      <span>Rows</span>
                      <strong>{searchConsoleTestResult.rows.toLocaleString()}</strong>
                    </div>
                    <div>
                      <span>Clicks</span>
                      <strong>{searchConsoleTestResult.clicks.toLocaleString()}</strong>
                    </div>
                    <div>
                      <span>Impressions</span>
                      <strong>{searchConsoleTestResult.impressions.toLocaleString()}</strong>
                    </div>
                  </div>
                ) : null}
                {searchConsoleMergeResult ? (
                  <div className="integration-result settings-wide">
                    <div>
                      <span>Fetched</span>
                      <strong>
                        {searchConsoleMergeResult.fetchedRows.toLocaleString()}
                      </strong>
                    </div>
                    <div>
                      <span>Matched</span>
                      <strong>
                        {searchConsoleMergeResult.matchedRows.toLocaleString()}
                      </strong>
                    </div>
                    <div>
                      <span>Clicks</span>
                      <strong>{searchConsoleMergeResult.clicks.toLocaleString()}</strong>
                    </div>
                    <div>
                      <span>Impressions</span>
                      <strong>
                        {searchConsoleMergeResult.impressions.toLocaleString()}
                      </strong>
                    </div>
                  </div>
                ) : null}
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "rendering"}>
              <h3>JavaScript Rendering</h3>
              <div className="settings-grid compact">
                <CheckboxField
                  checked={config.rendering.enabled}
                  disabled={running}
                  onCheckedChange={(enabled) => updateRendering({ enabled })}
                >
                  Render DOM
                </CheckboxField>
                <label>
                  Backend
                  <select
                    value={config.rendering.backend}
                    disabled={running || !config.rendering.enabled}
                    onChange={(event) =>
                      updateRendering({
                        backend: event.target.value as JsRenderingBackend,
                      })
                    }
                  >
                    <option value="chromeCdp">Chrome CDP</option>
                  </select>
                </label>
                <label>
                  Wait after load
                  <input
                    type="number"
                    min={0}
                    step={100}
                    value={config.rendering.waitAfterLoadMs}
                    disabled={running || !config.rendering.enabled}
                    onChange={(event) =>
                      updateRendering({
                        waitAfterLoadMs: Number(event.target.value),
                      })
                    }
                  />
                </label>
              </div>
            </section>

            <section className="settings-section" hidden={settingsTab !== "extraction"}>
              <div className="section-heading">
                <h3>Custom Extraction</h3>
                <button className="settings-action-button primary" onClick={addExtractor}>
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
                        className="settings-icon-danger"
                        onClick={() => removeExtractor(index)}
                        title="Remove extractor"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  ))
                )}
              </div>
              <div className="section-heading">
                <h3>Custom Search</h3>
                <button className="settings-action-button primary" onClick={addCustomSearch}>
                  <Plus size={16} />
                  <span>Add</span>
                </button>
              </div>
              <div className="extractor-list">
                {config.customSearches.length === 0 ? (
                  <span className="extractor-empty">No custom searches</span>
                ) : (
                  config.customSearches.map((customSearch, index) => (
                    <div className="extractor-row search-row" key={`${customSearch.name}-${index}`}>
                      <input
                        aria-label="Search name"
                        value={customSearch.name}
                        onChange={(event) =>
                          updateCustomSearch(index, { name: event.target.value })
                        }
                        placeholder="Column name"
                      />
                      <input
                        aria-label="Search pattern"
                        value={customSearch.pattern}
                        onChange={(event) =>
                          updateCustomSearch(index, { pattern: event.target.value })
                        }
                        placeholder="Text or regex"
                      />
                      <label className="compact-number-field">
                        Snippets
                        <input
                          min={0}
                          max={20}
                          type="number"
                          value={customSearch.maxSnippets}
                          onChange={(event) =>
                            updateCustomSearch(index, {
                              maxSnippets: Number(event.target.value),
                            })
                          }
                        />
                      </label>
                      <CheckboxField
                        checked={customSearch.regex}
                        onCheckedChange={(checked) =>
                          updateCustomSearch(index, { regex: checked })
                        }
                      >
                        Regex
                      </CheckboxField>
                      <CheckboxField
                        checked={customSearch.caseSensitive}
                        onCheckedChange={(checked) =>
                          updateCustomSearch(index, { caseSensitive: checked })
                        }
                      >
                        Case
                      </CheckboxField>
                      <button
                        className="settings-icon-danger"
                        onClick={() => removeCustomSearch(index)}
                        title="Remove search"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  ))
                )}
              </div>
            </section>
              </div>
            </div>
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
        sitemapRows={sitemapValidationRows}
        sitemapTotal={sitemapValidationTotal}
        loading={linkReportLoading}
        selectedUrl={selectedLinkReport === "selectedInlinks" ? selected?.url : selectedUrl}
        pageIndex={linkReportPage}
        onPageChange={setLinkReportPage}
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
        sitemapSortBy={sitemapValidationSortBy}
        sitemapSortDir={sitemapValidationSortDir}
        onSitemapSortChange={(column) => {
          setSitemapValidationSortDir((currentDirection) =>
            sitemapValidationSortBy === column && currentDirection === "asc" ? "desc" : "asc",
          );
          setSitemapValidationSortBy(column);
        }}
        onRefresh={() => void loadLinkReport(selectedLinkReport)}
      />

      <GraphDialog
        open={graphOpen}
        onOpenChange={setGraphOpen}
        graph={graph}
        loading={graphLoading}
        theme={resolvedTheme}
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
        onOpenBrokenLinks={openBrokenLinkReport}
        onOpenRedirects={openRedirectReport}
        onRefresh={() => void loadGraph()}
      />

      <ComparisonDialog
        open={comparisonOpen}
        onOpenChange={setComparisonOpen}
        archivePath={comparisonArchivePath}
        onArchivePathChange={setComparisonArchivePath}
        loading={comparisonLoading}
        result={comparisonResult}
        onCompare={() => void compareCrawlArchive()}
      />

      {!settingsOpen && !linkReportsOpen && !graphOpen && !comparisonOpen ? <FeedbackMessages /> : null}

      <section
        className={`workspace${overviewResizing ? " resizing" : ""}${issuesOpen ? " with-issues" : ""}${overviewOpen ? " with-overview" : ""}`}
        style={{ "--overview-width": `${overviewWidth}px` } as CSSProperties}
      >
        {issuesOpen ? (
          <nav className="issue-sidebar" aria-label="Issue views">
            <div className="issue-sidebar-heading">
              <strong>Audit views</strong>
              <button aria-label="Close audit views" onClick={() => setIssuesOpen(false)}><X size={15} /></button>
            </div>
            <p>Counts across the full crawl</p>
            {issueGroups.map((group) => (
              <details key={group.label} open={group === activeIssueGroup}>
                <summary>{group.label}</summary>
                {group.views.map((id) => {
                  const view = views.find((item) => item.id === id)!;
                  const summaryKey = viewSummaryKeys[id];
                  return (
                    <button key={id} data-view={id} aria-current={selectedView === id ? "page" : undefined}
                      className={selectedView === id ? "active" : ""} onClick={() => selectAuditView(id)}>
                      <span>{view.label}</span>
                      {summaryKey ? <span className="issue-count">{summary[summaryKey].toLocaleString()}</span> : null}
                    </button>
                  );
                })}
              </details>
            ))}
          </nav>
        ) : null}
        <section className="results-pane">
          <nav className="audit-categories" aria-label="Audit categories">
            <button className="audit-tree-toggle" aria-label="Toggle audit views" title="Browse all audit views" aria-expanded={issuesOpen} onClick={() => setIssuesOpen(!issuesOpen)}>
              <ListTree size={16} />
            </button>
            <div className="audit-category-list">
              {issueGroups.map((group) => <button key={group.label} title={group.label} data-category={group.label} aria-pressed={activeIssueGroup === group}
                onClick={() => { selectAuditView("all"); setActiveIssueGroup(group); }}>{group.tabLabel ?? group.label}</button>)}
            </div>
            <button className="overview-toggle" aria-label="Toggle overview" title="Overview and issues" aria-expanded={overviewOpen} onClick={() => setOverviewOpen(!overviewOpen)}>
              <Info size={16} />
            </button>
          </nav>
          <div className="grid-status">
            <select aria-label="Audit view" value={selectedView} onChange={(event) => setView(event.target.value as IssueView)}>
              <option value="all">All URLs</option>
              {activeIssueGroup.views.filter((id) => id !== "all").map((id) => <option key={id} value={id}>{views.find((view) => view.id === id)!.label}</option>)}
            </select>
            <div className="grid-status-left">
              <div className="view-toggle" aria-label="Result view mode">
                <button
                  className={resultsViewMode === "table" ? "active" : ""}
                  onClick={() => setResultsViewMode("table")}
                  aria-pressed={resultsViewMode === "table"}
                  aria-label="Table view" title="Table view"
                >
                  <Table2 size={15} />
                </button>
                <button
                  className={resultsViewMode === "tree" ? "active" : ""}
                  onClick={() => setResultsViewMode("tree")}
                  aria-pressed={resultsViewMode === "tree"}
                  aria-label="Tree view" title="Tree view"
                >
                  <ListTree size={15} />
                </button>
              </div>
              {urlSegments.length > 0 ? <label className="segment-filter">
                <span>Segment</span>
                <select
                  value={activeSegmentId}
                  onChange={(event) => { setActiveSegmentId(event.target.value); setPage(0); }}
                >
                  <option value="all">All URLs</option>
                  {urlSegments.map((segment) => (
                    <option key={segment.id} value={segment.id}>
                      {segment.name || segment.pattern}
                    </option>
                  ))}
                </select>
              </label> : null}
            </div>
            <div className="search-control grid-search">
              <Search size={16} />
              <input
                aria-label="Search results"
                value={globalSearch}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="Search current view"
              />
              {globalSearch ? <button aria-label="Clear search" title="Clear search" onClick={(event) => {
                setSearch("");
                event.currentTarget.parentElement?.querySelector("input")?.focus();
              }}><X size={15} /></button> : null}
            </div>
            {selectedView !== "all" || globalSearch || activeSegmentId !== "all" ? <button className="reset-filters" onClick={resetFilters}>Reset filters</button> : null}
            <select aria-label="Visible columns" value={showAllColumns ? "all" : "relevant"} onChange={(event) => setShowAllColumns(event.target.value === "all")}>
              <option value="relevant">Relevant columns</option>
              <option value="all">All columns</option>
            </select>
          </div>
          {resultsViewMode === "table" ? (
            <div className="grid" ref={parentRef} aria-busy={rowsLoading}>
              <table
                className="data-table"
                aria-label={`${views.find((view) => view.id === selectedView)?.label} results`}
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
                          className={header.column.id === "url" ? "url-column" : undefined}
                          aria-sort={sortBy === header.column.id ? (sortDir === "asc" ? "ascending" : "descending") : undefined}
                          style={{
                            width: header.getSize(),
                            minWidth: header.getSize(),
                            maxWidth: header.getSize(),
                          }}
                        >
                          <button
                            title={header.column.getCanSort() ? `Sort by ${header.column.columnDef.header}` : undefined}
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
                            {sortBy === header.column.id ? sortDir === "asc" ? <ChevronUp size={14} /> : <ChevronDown size={14} /> : null}
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
                        tabIndex={0}
                        aria-selected={selected?.id === row.id}
                        className={[
                          selected?.id === row.id ? "selected" : "",
                          statusRowClass(row),
                        ]
                          .filter(Boolean)
                          .join(" ")}
                        onClick={() => setSelected(row)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" || event.key === " ") {
                            event.preventDefault();
                            setSelected(row);
                          }
                        }}
                      >
                        {tableRow.getVisibleCells().map((cell) => (
                          <td
                            key={cell.id}
                            className={cell.column.id === "url" ? "url-column" : undefined}
                            title={cell.getValue<string>()}
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
              {rows.length === 0 ? (
                <div className="grid-empty" role="status">
                  <Search size={28} />
                  <strong>{rowsLoading ? "Loading results…" : summary.total > 0 ? "No matching URLs" : "Start your first crawl"}</strong>
                  <p>{summary.total > 0
                    ? "Choose another audit view or clear the search and segment filters."
                    : desktopRuntime ? "Enter a website URL above and select Start. Results and issues will appear as pages are crawled."
                    : "Open the desktop app with make dev to start crawling. This browser preview shows the workspace."}</p>
                  {!rowsLoading && summary.total > 0 ? <button onClick={resetFilters}>Reset filters</button> : null}
                </div>
              ) : null}
            </div>
          ) : (
            <UrlTreeView
              nodes={urlTree.nodes}
              totalUrls={urlTree.totalUrls}
              renderedUrls={urlTree.renderedUrls}
              capped={urlTree.capped}
              loading={urlTreeLoading}
              onRefresh={() => void loadUrlTree()}
              onSelectRecord={setSelected}
            />
          )}

          {resultsViewMode === "table" ? (
            <ResultPagination pageIndex={pageIndex} total={total} visible={rows.length} loading={rowsLoading} onPageChange={setPage} />
          ) : <div />}

          <aside className="detail-panel" aria-label="URL inspector">
            <div className="detail-tabs" role="tablist" aria-label="URL details" onKeyDown={handleTabKeys}>
              {detailTabs.map((tab) => <button key={tab.id} id={`detail-tab-${tab.id}`} role="tab"
                aria-controls={`detail-panel-${tab.id}`} aria-selected={detailTab === tab.id}
                tabIndex={detailTab === tab.id ? 0 : -1} onClick={() => setDetailTab(tab.id)}>{tab.label}</button>)}
            </div>
            {selected ? <>
              <div className="detail-header">
                <div>
                  <h2>{selected.statusCode ?? "No response"} {selected.statusText}</h2>
                  <p className="detail-url" title={selected.url}>{selected.url}</p>
                </div>
                <div className="detail-actions">
                  <button
                    onClick={() => void openSelectedUrl()}
                    title="Open URL in external browser"
                    disabled={!desktopRuntime}
                  >
                    <ExternalLink size={16} />
                  </button>
                  <button onClick={() => void copySelectedUrl()} title="Copy URL">
                    <Copy size={16} />
                  </button>
                  <button onClick={() => setSelected(undefined)} title="Clear selection" aria-label="Clear URL selection">
                    <X size={16} />
                  </button>
                </div>
              </div>
              {detailTab === "inlinks" || detailTab === "outlinks" ? (
                <div className="detail-link-panel" id={`detail-panel-${detailTab}`} role="tabpanel" aria-labelledby={`detail-tab-${detailTab}`}>
                  <SelectedLinksPanel key={`${selected.storageKey}-${detailTab}`} record={selected} direction={detailTab}
                    live={running} enabled={desktopRuntime} onOpenReport={() => {
                      setSelectedLinkReport(detailTab === "inlinks" ? "selectedInlinks" : "selectedOutlinks");
                      setLinkReportSearch(""); setLinkReportsOpen(true);
                    }} />
                </div>
              ) : (
              <div className="detail-content" key={`${selected.id}-${detailTab}`}>
                {selected.error ? <p className="detail-error">{selected.error}</p> : null}
                <div id="detail-panel-page" role="tabpanel" aria-labelledby="detail-tab-page" tabIndex={0} hidden={detailTab !== "page"}>
                  <dl>
                    {selected.url !== selected.finalUrl ? <><dt>Final URL</dt><dd>{selected.finalUrl}</dd></> : null}
                    <dt>Title</dt>
                    <dd>
                      {selected.title || "Missing"}{" "}
                      <span className="detail-muted">
                        ({selected.titleLen} chars, {selected.titlePixelWidth} px)
                      </span>
                    </dd>
                    <dt>Meta Description</dt>
                    <dd>
                      {selected.metaDescription || "Missing"}{" "}
                      <span className="detail-muted">
                        ({selected.metaDescriptionLen} chars,{" "}
                        {selected.metaDescriptionPixelWidth} px)
                      </span>
                    </dd>
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
                    <dt>Content</dt>
                    <dd>
                      {selected.wordCount.toLocaleString()} words,{" "}
                      {(selected.textToCodeRatio * 100).toFixed(1)}% text/code
                    </dd>
                    <dt>Indexability</dt>
                    <dd>{selected.indexabilityStatus}</dd>
                    <dt>Canonical</dt>
                    <dd>
                      {selected.canonical || "Missing"}{" "}
                      <span className="detail-muted">({selected.canonicalCount})</span>
                    </dd>
                  </dl>
                </div>
                <div id="detail-panel-links" role="tabpanel" aria-labelledby="detail-tab-links" tabIndex={0} hidden={detailTab !== "links"}>
                  <dl>
                    <dt>Found From</dt>
                    <dd>
                      <FoundFromDetail
                        record={selected}
                        desktopRuntime={desktopRuntime}
                        onOpenSource={() => void openSourceUrl()}
                        onCopySource={() => void copySourceUrl()}
                      />
                    </dd>
                    <dt>Crawl Path</dt>
                    <dd>
                      <CrawlPathDetail
                        response={selectedCrawlPath}
                        loading={crawlPathLoading}
                        onRefresh={() => void loadSelectedCrawlPath()}
                      />
                    </dd>
                    <dt>Links</dt>
                    <dd>
                      {selected.internalOutlinkCount} internal,{" "}
                      {selected.externalOutlinkCount} external
                    </dd>
                    <dt>Redirects</dt>
                    <dd>
                      {selected.redirectChain.length > 0 ? (
                        <ol className="redirect-chain">
                          {selected.redirectChain.map((hop, index) => (
                            <li key={`${hop.url}-${index}`}>
                              <span>{hop.statusCode}</span>
                              <span>
                                {hop.url} (DNS {formatMs(hop.dnsLookupTimeMs)}, TCP{" "}
                                {formatMs(hop.tcpConnectTimeMs)}, TLS{" "}
                                {formatMs(hop.tlsHandshakeTimeMs)}, TTFB{" "}
                                {formatMs(hop.ttfbMs ?? hop.elapsedMs)})
                              </span>
                              {hop.location ? <span>{hop.location}</span> : null}
                            </li>
                          ))}
                        </ol>
                      ) : (
                        "None"
                      )}
                    </dd>
                    <dt>Directives</dt>
                    <dd>
                      Meta robots: {selected.metaRobots || "None"}; X-Robots-Tag:{" "}
                      {selected.xRobotsTag || "None"}
                    </dd>
                    <dt>Pagination</dt>
                    <dd>
                      Next: {selected.relNext || "None"}; Prev: {selected.relPrev || "None"}
                    </dd>
                    <dt>AMP</dt>
                    <dd>{selected.amphtml || "None"}</dd>
                  </dl>
                </div>
                <div id="detail-panel-technical" role="tabpanel" aria-labelledby="detail-tab-technical" tabIndex={0} hidden={detailTab !== "technical"}>
                  <dl>
                    <dt>Images</dt>
                    <dd>
                      {selected.imageCount.toLocaleString()} images,{" "}
                      {selected.imagesMissingAlt.toLocaleString()} missing alt,{" "}
                      {selected.imagesAltTooLong.toLocaleString()} long alt
                      {selectedImages.length > 0 ? (
                        <ul className="detail-mini-list image-asset-list">
                          {selectedImages.map((image) => (
                            <li key={`${image.pageUrl}-${image.sourcePosition}-${image.imageUrl}`}>
                              <span>{imageAssetStatus(image)}</span>
                              <span>{image.imageUrl}</span>
                              <span>{imageAssetMeta(image)}</span>
                            </li>
                          ))}
                          {selectedImageTotal > selectedImages.length ? (
                            <li>
                              <span>More</span>
                              <span>
                                {(selectedImageTotal - selectedImages.length).toLocaleString()} additional
                                image references
                              </span>
                              <span>{selectedImageTotal.toLocaleString()} total</span>
                            </li>
                          ) : null}
                        </ul>
                      ) : null}
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
                    <dt>HTML validation</dt>
                    <dd>
                      {selected.deprecatedHtmlTagCount.toLocaleString()} deprecated tag instances,{" "}
                      {selected.duplicateIdCount.toLocaleString()} duplicate id instances
                    </dd>
                    <dt>Rendering</dt>
                    <dd>
                      {selected.jsRendered ? "Rendered DOM captured" : "Raw HTML only"}; DOM{" "}
                      {selected.renderedDomChanged ? "changed" : "unchanged"}; word diff{" "}
                      {selected.renderedWordCountDelta.toLocaleString()}, link diff{" "}
                      {selected.renderedLinkCountDelta.toLocaleString()}
                    </dd>
                    <dt>Hreflang</dt>
                    <dd>
                      {selected.hreflangCount.toLocaleString()} alternates,{" "}
                      {selected.hreflangInvalidCount.toLocaleString()} invalid, self-reference{" "}
                      {selected.hreflangMissingSelfReference ? "missing" : "ok"}
                      {selected.hreflangLinks.length > 0 ? (
                        <ul className="detail-mini-list">
                          {selected.hreflangLinks.slice(0, 8).map((link) => (
                            <li key={`${link.hreflang}-${link.url}`}>
                              <span>{link.hreflang}</span>
                              <span>{link.url}</span>
                              {!link.valid ? <span>Invalid</span> : null}
                            </li>
                          ))}
                          {selected.hreflangLinks.length > 8 ? (
                            <li>
                              <span>More</span>
                              <span>
                                {(selected.hreflangLinks.length - 8).toLocaleString()} additional
                                alternates
                              </span>
                            </li>
                          ) : null}
                        </ul>
                      ) : null}
                    </dd>
                    <dt>Structured Data</dt>
                    <dd>
                      {selected.jsonLdCount.toLocaleString()} JSON-LD blocks,{" "}
                      {selected.jsonLdInvalidCount.toLocaleString()} syntax invalid,{" "}
                      {selected.structuredDataErrorCount.toLocaleString()} errors,{" "}
                      {selected.structuredDataWarningCount.toLocaleString()} warnings
                      {selected.structuredDataIssues.length > 0 ? (
                        <ul className="detail-mini-list">
                          {selected.structuredDataIssues.slice(0, 8).map((issue, index) => (
                            <li key={`${issue.path}-${issue.message}-${index}`}>
                              <span>{issue.severity}</span>
                              <span>{issue.message}</span>
                              <span>{issue.path}</span>
                            </li>
                          ))}
                          {selected.structuredDataIssues.length > 8 ? (
                            <li>
                              <span>More</span>
                              <span>
                                {(selected.structuredDataIssues.length - 8).toLocaleString()} additional
                                issues
                              </span>
                            </li>
                          ) : null}
                        </ul>
                      ) : null}
                    </dd>
                    <dt>Social</dt>
                    <dd>
                      {selected.openGraphCount.toLocaleString()} Open Graph tags,{" "}
                      {selected.twitterCardCount.toLocaleString()} Twitter tags
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
                  </dl>
                </div>
                <div id="detail-panel-custom" role="tabpanel" aria-labelledby="detail-tab-custom" tabIndex={0} hidden={detailTab !== "custom"}>
                  <dl>
                    <dt>Search Console</dt>
                    <dd>
                      {selected.searchConsoleClicks !== null &&
                      selected.searchConsoleClicks !== undefined ? (
                        <>
                          {selected.searchConsoleClicks.toLocaleString()} clicks,{" "}
                          {selected.searchConsoleImpressions?.toLocaleString() ?? "0"} impressions,{" "}
                          {selected.searchConsoleCtr !== null &&
                          selected.searchConsoleCtr !== undefined
                            ? `${(selected.searchConsoleCtr * 100).toFixed(2)}% CTR`
                            : "No CTR"}
                          , position{" "}
                          {selected.searchConsoleAveragePosition !== null &&
                          selected.searchConsoleAveragePosition !== undefined
                            ? selected.searchConsoleAveragePosition.toFixed(2)
                            : "None"}
                        </>
                      ) : (
                        "Not merged"
                      )}
                    </dd>
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
                    <dt>Custom Searches</dt>
                    <dd>
                      {selected.customSearches.length > 0 ? (
                        <ul className="extraction-values search-values">
                          {selected.customSearches.map((customSearch) => (
                            <li key={`${customSearch.source}-${customSearch.name}`}>
                              <strong>
                                {customSearch.name} · {customSearchSourceLabel(customSearch.source)} ·{" "}
                                {customSearch.matched ? customSearch.matchCount : 0}
                              </strong>
                              <span>
                                {customSearch.snippets.length > 0
                                  ? customSearch.snippets.join(" | ")
                                  : customSearch.matched
                                    ? "Matched"
                                    : "No match"}
                              </span>
                            </li>
                          ))}
                        </ul>
                      ) : (
                        "None"
                      )}
                    </dd>
                  </dl>
                </div>
              </div>
              )}
            </> : <div className="detail-empty" id={`detail-panel-${detailTab}`} role="tabpanel" aria-labelledby={`detail-tab-${detailTab}`}>
              <MousePointer2 size={22} />
              <strong>No URL selected</strong>
              <span>Select a result to inspect page details, inlinks and outlinks.</span>
            </div>}
          </aside>
        </section>
        {overviewOpen ? <><div
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
          onViewSelect={selectAuditView}
          statusLabel={crawlStateLabel}
          onClose={() => setOverviewOpen(false)}
        />
        </> : null}
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

function handleTabKeys(event: KeyboardEvent<HTMLElement>) {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
  const current = tabs.indexOf(event.target as HTMLButtonElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
    : (current + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
  tabs[next]?.focus();
  tabs[next]?.click();
}

function ResultPagination({ pageIndex, total, visible, loading, onPageChange }: {
  pageIndex: number; total: number; visible: number; loading: boolean; onPageChange: (page: number) => void;
}) {
  const lastPage = Math.max(0, Math.ceil(total / resultsPageSize) - 1);
  return <div className="grid-pagination" aria-label="Result pages">
    <span role="status">{loading ? "Loading…" : total > 0
      ? `${(pageIndex * resultsPageSize + 1).toLocaleString()}–${Math.min(total, pageIndex * resultsPageSize + visible).toLocaleString()} of ${total.toLocaleString()}`
      : "0 results"}</span>
    <div>
      <button aria-label="First page" disabled={pageIndex === 0 || loading} onClick={() => onPageChange(0)}><ChevronsLeft size={16} /></button>
      <button aria-label="Previous page" disabled={pageIndex === 0 || loading} onClick={() => onPageChange(pageIndex - 1)}><ChevronLeft size={16} /></button>
      <span>Page {pageIndex + 1} / {lastPage + 1}</span>
      <button aria-label="Next page" disabled={pageIndex >= lastPage || loading} onClick={() => onPageChange(pageIndex + 1)}><ChevronRight size={16} /></button>
      <button aria-label="Last page" disabled={pageIndex >= lastPage || loading} onClick={() => onPageChange(lastPage)}><ChevronsRight size={16} /></button>
    </div>
  </div>;
}

function FoundFromDetail({
  record,
  desktopRuntime,
  onOpenSource,
  onCopySource,
}: {
  record: CrawlRecord;
  desktopRuntime: boolean;
  onOpenSource: () => void;
  onCopySource: () => void;
}) {
  if (!record.firstInlinkSourceUrl) {
    return (
      <div className="source-detail source-detail-empty">
        <span>{missingFoundFromReason(record)}</span>
      </div>
    );
  }

  return (
    <div className="source-detail">
      <span className="source-summary">
        Found this URL on <strong>{compactUrl(record.firstInlinkSourceUrl)}</strong>.
      </span>
      <span className="source-url">{record.firstInlinkSourceUrl}</span>
      <span className="detail-muted">
        {foundFromContext(record)}
      </span>
      <span className="source-actions">
        <button onClick={onOpenSource} disabled={!desktopRuntime}>
          <ExternalLink size={14} />
          <span>Open source page</span>
        </button>
        <button onClick={onCopySource}>
          <Copy size={14} />
          <span>Copy source URL</span>
        </button>
      </span>
    </div>
  );
}

function CrawlPathDetail({
  response,
  loading,
  onRefresh,
}: {
  response?: CrawlPathResponse;
  loading: boolean;
  onRefresh: () => void;
}) {
  const emptyMessage = loading
    ? "Finding internal crawl path..."
    : "No internal crawl path has been loaded yet.";

  return (
    <div className="crawl-path-detail">
      <div className="crawl-path-head">
        <span>
          {response?.found
            ? response.steps.length === 0
              ? "This URL is a crawl start node."
              : `${response.steps.length.toLocaleString()} internal hop${
                  response.steps.length === 1 ? "" : "s"
                }`
            : response
              ? "No internal path found from the crawl start."
              : emptyMessage}
        </span>
        <button onClick={onRefresh} disabled={loading}>
          <RefreshCw size={14} />
          <span>{loading ? "Loading" : "Refresh"}</span>
        </button>
      </div>
      {response?.truncated ? (
        <p className="crawl-path-warning">
          Path search inspected {response.exploredEdges.toLocaleString()} edges and was
          capped before the full edge set was loaded.
        </p>
      ) : null}
      {response?.found && response.steps.length > 0 ? (
        <ol className="crawl-path-list">
          {response.steps.map((step, index) => (
            <li key={`${step.id}-${index}`}>
              <span>{index + 1}</span>
              <div>
                <strong>
                  {compactUrl(step.sourceUrl)} to {compactUrl(step.targetUrl)}
                </strong>
                <em>
                  {step.anchorText || "No anchor text"}; target status{" "}
                  {statusCell(step.targetStatusCode)}; source position{" "}
                  {step.sourcePosition.toLocaleString()}; depth {step.sourceDepth}
                  {step.targetDepth !== null && step.targetDepth !== undefined
                    ? ` to ${step.targetDepth}`
                    : ""}
                </em>
              </div>
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
}

function UrlTreeView({
  nodes,
  totalUrls,
  renderedUrls,
  capped,
  loading,
  onRefresh,
  onSelectRecord,
}: {
  nodes: UrlTreeNode[];
  totalUrls: number;
  renderedUrls: number;
  capped: boolean;
  loading: boolean;
  onRefresh: () => void;
  onSelectRecord: (record: CrawlRecord) => void;
}) {
  return (
    <section className="url-tree-view" aria-label="URL tree view">
      <header className="url-tree-toolbar">
        <div>
          <strong>URL Tree</strong>
          <span>
            {renderedUrls.toLocaleString()} of {totalUrls.toLocaleString()} URLs
            {capped ? " shown" : ""}
          </span>
        </div>
        <button onClick={onRefresh} disabled={loading}>
          <RefreshCw size={15} className={loading ? "spin" : ""} />
          <span>Refresh</span>
        </button>
      </header>
      {capped ? (
        <div className="url-tree-note">
          Tree rendering is capped to keep the desktop UI responsive. Narrow the
          current view or search to inspect deeper branches.
        </div>
      ) : null}
      <div className="url-tree-list">
        {nodes.length > 0 ? (
          nodes.map((node) => (
            <UrlTreeNodeRow
              key={node.id}
              node={node}
              onSelectRecord={onSelectRecord}
            />
          ))
        ) : (
          <div className="url-tree-empty">
            {loading ? "Loading tree..." : "No URLs match the current view."}
          </div>
        )}
      </div>
    </section>
  );
}

function UrlTreeNodeRow({
  node,
  onSelectRecord,
}: {
  node: UrlTreeNode;
  onSelectRecord: (record: CrawlRecord) => void;
}) {
  const hasChildren = node.children.length > 0;
  const [open, setOpen] = useState(node.depth < 2 || node.broken > 0);
  const tone = treeNodeTone(node);

  return (
    <div className="url-tree-node">
      <div
        className={["url-tree-row", tone].filter(Boolean).join(" ")}
        style={{ "--tree-depth": node.depth } as CSSProperties}
      >
        <button
          className="url-tree-expander"
          onClick={() => setOpen((value) => !value)}
          disabled={!hasChildren}
          title={hasChildren ? (open ? "Collapse" : "Expand") : "Leaf URL"}
        >
          {hasChildren ? (
            open ? (
              <ChevronDown size={15} />
            ) : (
              <ChevronRight size={15} />
            )
          ) : (
            <span />
          )}
        </button>
        <button
          className="url-tree-main"
          onClick={() => {
            if (node.record) {
              onSelectRecord(node.record);
              return;
            }
            if (hasChildren) {
              setOpen((value) => !value);
            }
          }}
          title={node.url ?? node.path}
        >
          {hasChildren ? <Folder size={16} /> : <FileText size={16} />}
          <span className="url-tree-label">{node.label}</span>
          <span className="url-tree-path">{node.path}</span>
        </button>
        <div className="url-tree-pills">{treeStatusPills(node)}</div>
      </div>
      {hasChildren && open ? (
        <div className="url-tree-children">
          {node.children.map((child) => (
            <UrlTreeNodeRow
              key={child.id}
              node={child}
              onSelectRecord={onSelectRecord}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function treeStatusPills(node: UrlTreeNode) {
  const pills: ReactNode[] = [
    <span key="total" className="url-tree-pill">
      {node.total.toLocaleString()}
    </span>,
  ];

  if (node.record?.statusCode !== null && node.record?.statusCode !== undefined) {
    pills.push(
      <span
        key="status"
        className={["url-tree-pill", treeNodeTone(node)].filter(Boolean).join(" ")}
      >
        {statusCell(node.record.statusCode)}
      </span>,
    );
    return pills;
  }

  if (node.clientErrors > 0) {
    pills.push(
      <span key="4xx" className="url-tree-pill client-error">
        4xx {node.clientErrors.toLocaleString()}
      </span>,
    );
  }
  if (node.serverErrors > 0) {
    pills.push(
      <span key="5xx" className="url-tree-pill server-error">
        5xx {node.serverErrors.toLocaleString()}
      </span>,
    );
  }
  if (node.noResponse > 0) {
    pills.push(
      <span key="no-response" className="url-tree-pill no-response">
        No response {node.noResponse.toLocaleString()}
      </span>,
    );
  }
  if (node.redirects > 0) {
    pills.push(
      <span key="redirects" className="url-tree-pill redirect">
        3xx {node.redirects.toLocaleString()}
      </span>,
    );
  }

  return pills;
}

function treeNodeTone(node: UrlTreeNode) {
  if (node.serverErrors > 0) {
    return "server-error";
  }
  if (node.clientErrors > 0) {
    return "client-error";
  }
  if (node.noResponse > 0) {
    return "no-response";
  }
  return "";
}

function FeedbackMessages() {
  const { error, notice, settingsError, setError, setNotice } = useAppStore();
  if (!error && !notice && !settingsError) return null;
  return (
    <div className="feedback-messages">
      {settingsError ? <div className="error-bar" role="alert">
        <span>{settingsError}</span>
        <button aria-label="Dismiss settings error" onClick={() => useAppStore.setState({ settingsError: undefined })}><X size={16} /></button>
      </div> : null}
      {error ? <div className="error-bar" role="alert">
        <span>{error}</span>
        <button aria-label="Dismiss error" onClick={() => setError(undefined)}><X size={16} /></button>
      </div> : null}
      {notice ? <div className="notice-bar" role="status">
        <span>{notice}</span>
        <button aria-label="Dismiss notification" onClick={() => setNotice(undefined)}><X size={16} /></button>
      </div> : null}
    </div>
  );
}

function Metric({
  label,
  value,
  tone,
  onClick,
}: {
  label: string;
  value: string | number;
  tone?: "danger";
  onClick?: () => void;
}) {
  const Element = onClick ? "button" : "div";
  return (
    <Element className={tone === "danger" ? "metric danger" : "metric"} onClick={onClick} title={onClick ? `Show ${label.toLowerCase()} URLs` : undefined}>
      <span>{label}</span>
      <strong>{typeof value === "number" ? value.toLocaleString() : value}</strong>
    </Element>
  );
}

function ComparisonDialog({
  open,
  onOpenChange,
  archivePath,
  onArchivePathChange,
  loading,
  result,
  onCompare,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  archivePath: string;
  onArchivePathChange: (path: string) => void;
  loading: boolean;
  result?: CrawlComparisonResponse;
  onCompare: () => void;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-backdrop" />
        <Dialog.Content className="comparison-modal">
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>Crawl Comparison</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button title="Close crawl comparison">
                <X size={16} />
              </button>
            </Dialog.Close>
            <FeedbackMessages />
          </div>

          <div className="comparison-controls">
            <label>
              Baseline crawl archive
              <input
                value={archivePath}
                placeholder="/path/to/ferrous-frog-crawl-archive.ffcrawl.json"
                onChange={(event) => onArchivePathChange(event.target.value)}
              />
            </label>
            <button
              className="settings-action-button primary"
              onClick={onCompare}
              disabled={loading || !archivePath.trim()}
            >
              <FileText size={15} />
              <span>{loading ? "Comparing" : "Compare"}</span>
            </button>
          </div>

          {result ? (
            <>
              <div className="comparison-summary">
                <Metric label="Baseline" value={result.baselineRecords} />
                <Metric label="Current" value={result.currentRecords} />
                <Metric label="Added" value={result.added} />
                <Metric label="Removed" value={result.removed} tone="danger" />
                <Metric label="Changed" value={result.changed} />
                <Metric label="Status" value={result.statusChanged} tone="danger" />
              </div>
              <div className="comparison-deltas">
                {result.metricDeltas.map((metric) => (
                  <div key={metric.label}>
                    <span>{metric.label}</span>
                    <strong>{formatSignedDelta(metric.delta)}</strong>
                    <em>
                      {metric.previous.toLocaleString()} to{" "}
                      {metric.current.toLocaleString()}
                    </em>
                  </div>
                ))}
              </div>
              <div className="link-report-table-wrap comparison-table-wrap">
                <table className="link-report-table comparison-table">
                  <thead>
                    <tr>
                      <th>Change</th>
                      <th>URL</th>
                      <th>Status</th>
                      <th>Title</th>
                      <th>Indexability</th>
                      <th>Hash</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.rows.map((row) => (
                      <tr key={`${row.change}-${row.url}`}>
                        <td>
                          <span className={`severity-pill ${comparisonTone(row.change)}`}>
                            {row.change}
                          </span>
                        </td>
                        <td>{row.url}</td>
                        <td>
                          {statusCell(row.previousStatusCode)} to{" "}
                          {statusCell(row.currentStatusCode)}
                        </td>
                        <td>
                          {row.previousTitle || "None"} to {row.currentTitle || "None"}
                        </td>
                        <td>
                          {row.previousIndexability || "None"} to{" "}
                          {row.currentIndexability || "None"}
                        </td>
                        <td>
                          {compactHash(row.previousResponseHash)} to{" "}
                          {compactHash(row.currentResponseHash)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          ) : (
            <p className="link-report-empty">
              Compare the current crawl against a previously exported crawl archive.
            </p>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
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
  sitemapRows,
  sitemapTotal,
  loading,
  selectedUrl,
  pageIndex,
  onPageChange,
  searchValue,
  onSearchValueChange,
  sortBy,
  sortDir,
  onSortChange,
  anchorSortBy,
  anchorSortDir,
  onAnchorSortChange,
  sitemapSortBy,
  sitemapSortDir,
  onSitemapSortChange,
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
  sitemapRows: SitemapValidationRow[];
  sitemapTotal: number;
  loading: boolean;
  selectedUrl?: string;
  pageIndex: number;
  onPageChange: (page: number) => void;
  searchValue: string;
  onSearchValueChange: (value: string) => void;
  sortBy: string;
  sortDir: SortDirection;
  onSortChange: (column: string) => void;
  anchorSortBy: string;
  anchorSortDir: SortDirection;
  onAnchorSortChange: (column: string) => void;
  sitemapSortBy: string;
  sitemapSortDir: SortDirection;
  onSitemapSortChange: (column: string) => void;
  onRefresh: () => void;
}) {
  const isRedirectReport = selectedReport === "redirects";
  const isAnchorReport = selectedReport === "anchorText";
  const isSitemapReport = selectedReport === "sitemapValidation";
  const total = isRedirectReport
    ? redirectTotal
    : isAnchorReport
      ? anchorTotal
      : isSitemapReport
        ? sitemapTotal
        : edgeTotal;
  const visible = isRedirectReport
    ? redirectRows.length
    : isAnchorReport
      ? anchorRows.length
      : isSitemapReport
        ? sitemapRows.length
        : edges.length;
  const selectedReportNeedsUrl =
    selectedReport === "selectedInlinks" || selectedReport === "selectedOutlinks";

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-backdrop" />
        <Dialog.Content className="link-report-modal">
          <Dialog.Description className="sr-only">Inspect links, sources and redirect chains from the current crawl.</Dialog.Description>
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>Link Reports</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button title="Close link reports">
                <X size={16} />
              </button>
            </Dialog.Close>
            <FeedbackMessages />
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
          ) : isSitemapReport ? (
            <SitemapValidationReportTable
              rows={sitemapRows}
              sortBy={sitemapSortBy}
              sortDir={sitemapSortDir}
              onSortChange={onSitemapSortChange}
            />
          ) : (
            <LinkEdgeReportTable
              edges={edges}
              sortBy={sortBy}
              sortDir={sortDir}
              onSortChange={onSortChange}
            />
          )}
          <ResultPagination pageIndex={pageIndex} total={total} visible={visible} loading={loading} onPageChange={onPageChange} />
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

function SitemapValidationReportTable({
  rows,
  sortBy,
  sortDir,
  onSortChange,
}: {
  rows: SitemapValidationRow[];
  sortBy: string;
  sortDir: SortDirection;
  onSortChange: (column: string) => void;
}) {
  if (rows.length === 0) {
    return <p className="link-report-empty">No sitemap URLs match this report.</p>;
  }

  const columns = [
    { key: "severity", label: "Severity" },
    { key: "issueCount", label: "Issues" },
    { key: "statusCode", label: "Status" },
    { key: "finalUrl", label: "URL" },
    { key: "indexabilityStatus", label: "Indexability" },
    { key: "inlinkCount", label: "Inlinks" },
    { key: "canonical", label: "Canonical" },
    { key: "redirectTarget", label: "Redirect Target" },
  ];

  return (
    <div className="link-report-table-wrap">
      <table className="link-report-table sitemap-validation-table">
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
            <tr key={row.url} className={`sitemap-${row.severity}`}>
              <td>
                <span className={`severity-pill ${row.severity}`}>{row.severity}</span>
              </td>
              <td>
                <div className="issue-list">
                  {row.issues.map((issue) => (
                    <span key={issue}>{issue}</span>
                  ))}
                </div>
              </td>
              <td>{statusCell(row.statusCode)}</td>
              <td>{row.finalUrl}</td>
              <td>{row.indexabilityStatus}</td>
              <td>{row.inlinkCount.toLocaleString()}</td>
              <td>{row.canonical || "None"}</td>
              <td>{row.redirectTarget || "None"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SelectedLinksPanel({ record, direction, live, enabled, onOpenReport }: {
  record: CrawlRecord; direction: "inlinks" | "outlinks"; live: boolean; enabled: boolean; onOpenReport: () => void;
}) {
  const [response, setResponse] = useState<LinkEdgeResponse>({ edges: [], total: 0 });
  const [page, setPage] = useState(0);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState({ by: direction === "inlinks" ? "sourceUrl" : "targetUrl", dir: "asc" as SortDirection });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    setResponse({ edges: [], total: 0 });
    setError(undefined);
    const load = async () => {
      if (!enabled) { setLoading(false); return; }
      setLoading(true);
      try {
        const result = await invoke<LinkEdgeResponse>("get_link_edges", { query: {
          offset: page * resultsPageSize, limit: resultsPageSize,
          globalSearch: search.trim() || null, sortBy: sort.by, sortDir: sort.dir, view: "all",
          sourceUrl: direction === "outlinks" ? record.finalUrl : null,
          targetUrl: direction === "inlinks" ? record.url : null, internalOnly: false,
        } });
        if (cancelled) return;
        const last = Math.max(0, Math.ceil(result.total / resultsPageSize) - 1);
        if (page > last) { setPage(last); return; }
        setResponse(result);
        setError(undefined);
      } catch (caught) {
        if (!cancelled) setError(errorMessage(caught));
      } finally {
        if (!cancelled) {
          setLoading(false);
          if (live) timer = window.setTimeout(load, 1000);
        }
      }
    };
    void load();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [record.url, record.finalUrl, direction, page, search, sort, live, enabled, revision]);

  return <section className={`selected-links ${direction}`} aria-label={`Selected URL ${direction}`} aria-busy={loading}>
    <div className="selected-links-controls">
      <label className="search-control"><Search size={14} /><input aria-label={`Search ${direction}`} placeholder={`Search ${direction}`}
        value={search} onChange={(event) => { setSearch(event.target.value); setPage(0); }} /></label>
      <button onClick={onOpenReport} title="Open full link report" disabled={!enabled}><ExternalLink size={14} /><span>Full report</span></button>
    </div>
    {error ? <div className="link-report-empty" role="alert"><p>{error}</p><button onClick={() => setRevision((value) => value + 1)}>Retry</button></div> :
      loading && response.edges.length === 0 ? <p className="link-report-empty">Loading links…</p> :
      <LinkEdgeReportTable edges={response.edges} sortBy={sort.by} sortDir={sort.dir} onSortChange={(by) => {
        setSort({ by, dir: by === sort.by && sort.dir === "asc" ? "desc" : "asc" }); setPage(0);
      }} />}
    <ResultPagination pageIndex={page} total={response.total} visible={response.edges.length} loading={loading} onPageChange={setPage} />
  </section>;
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
  onOpenBrokenLinks,
  onOpenRedirects,
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
  onOpenBrokenLinks: () => void;
  onOpenRedirects: () => void;
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
  const brokenTargets = useMemo(() => brokenGraphTargets(graph), [graph]);
  const sourceBrokenLinkCount = graph?.edges.filter((edge) => isBrokenGraphEdge(edge, brokenTargets)).length ?? 0;
  const sourceRedirectLinkCount = graph?.edges.filter(isRedirectGraphEdge).length ?? 0;
  const visibleBrokenLinkCount =
    visibleGraph?.edges.filter((edge) => isBrokenGraphEdge(edge, brokenTargets)).length ?? 0;
  const visibleRedirectLinkCount =
    visibleGraph?.edges.filter(isRedirectGraphEdge).length ?? 0;

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
          <Dialog.Description className="sr-only">Explore crawled pages and the links connecting them.</Dialog.Description>
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>Crawl Graph</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button title="Close graph">
                <X size={16} />
              </button>
            </Dialog.Close>
            <FeedbackMessages />
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
              <button
                onClick={onOpenBrokenLinks}
                disabled={sourceBrokenLinkCount === 0}
                title="Open source-to-target rows for broken links"
              >
                Broken Report
              </button>
              <button
                onClick={onOpenRedirects}
                disabled={sourceRedirectLinkCount === 0}
                title="Open redirect-chain rows"
              >
                Redirect Report
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
            <button
              className={statusFilter === "brokenLinks" ? "active" : ""}
              onClick={() => {
                onStatusFilterChange("brokenLinks");
                onDepthFilterChange("all");
              }}
              disabled={sourceBrokenLinkCount === 0}
              title="Show broken source-to-target edges with both endpoint nodes"
            >
              Broken Links
              {sourceBrokenLinkCount > 0 ? ` (${sourceBrokenLinkCount})` : ""}
            </button>
            <button
              className={statusFilter === "redirectLinks" ? "active" : ""}
              onClick={() => {
                onStatusFilterChange("redirectLinks");
                onDepthFilterChange("all");
              }}
              disabled={sourceRedirectLinkCount === 0}
              title="Show redirect source-to-target edges with both endpoint nodes"
            >
              Redirect Links
              {sourceRedirectLinkCount > 0 ? ` (${sourceRedirectLinkCount})` : ""}
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
                <option value="brokenLinks">Broken Links</option>
                <option value="redirectLinks">Redirect Links</option>
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
            <span className="warning-link">Redirect Link</span>
            <span className="danger">Broken</span>
            <span className="danger-link">Broken Link</span>
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
              <span>
                Hover or select a node for details.
                {visibleBrokenLinkCount > 0
                  ? ` ${visibleBrokenLinkCount.toLocaleString()} broken link edges visible.`
                  : ""}
                {visibleRedirectLinkCount > 0
                  ? ` ${visibleRedirectLinkCount.toLocaleString()} redirect link edges visible.`
                  : ""}
              </span>
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
                theme={theme}
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
  theme,
  onNodeSelect,
}: {
  graph: CrawlGraph;
  layoutMode: GraphLayoutMode;
  theme: Theme;
  onNodeSelect: (node: GraphNodeAttributes) => void;
}) {
  const colors = graphPalette();
  const layout = useMemo(() => svgGraphLayout(graph, layoutMode), [graph, layoutMode, theme]);

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
  onClose,
  statusLabel,
}: {
  summary: CrawlSummary;
  progress?: CrawlProgress;
  progressPercent: number;
  progressHistory: ProgressSample[];
  onViewSelect: (view: IssueView) => void;
  onClose: () => void;
  statusLabel: string;
}) {
  const [tab, setTab] = useState<"overview" | "issues">("overview");
  const findings = issueGroups.slice(1).flatMap((group) => group.views.flatMap((id) => {
    const key = viewSummaryKeys[id];
    return key && !id.startsWith("status") && summary[key] > 0
      ? [{ id, group: group.label, label: views.find((view) => view.id === id)!.label, count: summary[key] }] : [];
  })).sort((a, b) => b.count - a.count);
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
      label: "Structured data errors",
      value: summary.structuredDataInvalid,
      tone: "danger" as const,
      view: "structuredDataInvalid",
    },
    {
      label: "Structured data warnings",
      value: summary.structuredDataWarnings,
      tone: "warning" as const,
      view: "structuredDataWarning",
    },
    {
      label: "Deprecated HTML",
      value: summary.deprecatedHtmlTags,
      tone: "warning" as const,
      view: "htmlDeprecatedTags",
    },
    {
      label: "Duplicate IDs",
      value: summary.duplicateIds,
      tone: "warning" as const,
      view: "htmlDuplicateIds",
    },
    {
      label: "Rendered changes",
      value: summary.renderedDomChanged,
      tone: "muted" as const,
      view: "renderedDomChanged",
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
      <div className="inspector-heading">
        <div className="detail-tabs" role="tablist" aria-label="Crawl inspection" onKeyDown={handleTabKeys}>
          {(["overview", "issues"] as const).map((id) => <button key={id} id={`inspection-tab-${id}`} role="tab"
            aria-controls={`inspection-panel-${id}`} aria-selected={tab === id} tabIndex={tab === id ? 0 : -1} onClick={() => setTab(id)}>
            {id === "overview" ? "Overview" : "Issues"}{id === "issues" && findings.length > 0 ? <span className="issue-count">{findings.length}</span> : null}
          </button>)}
        </div>
        <button aria-label="Close overview" onClick={onClose}><X size={15} /></button>
      </div>
      <div className="overview-content" id="inspection-panel-overview" role="tabpanel" aria-labelledby="inspection-tab-overview" tabIndex={0} hidden={tab !== "overview"}>
      <div className="overview-state"><span>{statusLabel}</span><strong>{summary.total.toLocaleString()} URLs</strong></div>
      {progress ? <section className="overview-section">
        <div className="overview-progress">
          <span style={{ width: `${progressPercent}%` }} />
        </div>
      </section> : null}

      {progressHistory.length > 1 ? <OverviewTrend history={progressHistory} /> : null}

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
      </div>
      <div className="overview-content" id="inspection-panel-issues" role="tabpanel" aria-labelledby="inspection-tab-issues" tabIndex={0} hidden={tab !== "issues"}>
        <div className="overview-state"><span>Checks with findings</span><strong>{findings.length}</strong></div>
        {findings.length > 0 ? <table className="issue-summary-table">
          <thead><tr><th>Audit</th><th>URLs</th></tr></thead>
          <tbody>{findings.map((finding) => <tr key={finding.id}>
            <td><button onClick={() => onViewSelect(finding.id)} title={`Show ${finding.label.toLowerCase()} URLs`}>
              <small>{finding.group}</small><span>{finding.label}</span>
            </button></td>
            <td>{finding.count.toLocaleString()}</td>
          </tr>)}</tbody>
        </table> : <div className="detail-empty"><Check size={22} /><strong>No findings in this summary</strong><span>Browse audit views for additional checks.</span></div>}
      </div>
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
        <span>TCP {formatMs(record.tcpConnectTimeMs)}</span>
        <span>TLS {formatMs(record.tlsHandshakeTimeMs)}</span>
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
  if (column.kind === "search") {
    return customSearchCell(row, column.name);
  }

  const value = row[column.key];
  if (column.key === "firstInlinkSourceUrl") {
    return foundFromCell(row);
  }
  if (value === null || value === undefined || value === "") {
    return " ";
  }
  if (
    column.key === "responseTimeMs" ||
    column.key === "dnsLookupTimeMs" ||
    column.key === "tcpConnectTimeMs" ||
    column.key === "tlsHandshakeTimeMs" ||
    column.key === "ttfbMs" ||
    column.key === "downloadTimeMs" ||
    column.key === "totalNetworkTimeMs"
  ) {
    return `${value} ms`;
  }
  if (column.key === "transferRateBytesPerSec") {
    return `${formatBytes(Number(value))}/s`;
  }
  if (column.key === "searchConsoleCtr") {
    return `${(Number(value) * 100).toFixed(2)}%`;
  }
  if (
    column.key === "searchConsoleClicks" ||
    column.key === "searchConsoleImpressions"
  ) {
    return Number(value).toLocaleString();
  }
  if (column.key === "searchConsoleAveragePosition") {
    return Number(value).toFixed(2);
  }
  if (column.key === "sizeBytes") {
    return formatBytes(Number(value));
  }
  if (column.key === "inSitemap") {
    return value ? "Yes" : "No";
  }
  if (column.key === "listDuplicateIndex" && !row.listPosition) {
    return " ";
  }
  return String(value);
}

function foundFromCell(record: CrawlRecord) {
  if (!record.firstInlinkSourceUrl) {
    return missingFoundFromReason(record);
  }

  const context = sourceAnchorLabel(record);
  return context
    ? `Found on ${compactUrl(record.firstInlinkSourceUrl)} via ${context}`
    : `Found on ${compactUrl(record.firstInlinkSourceUrl)}`;
}

function foundFromContext(record: CrawlRecord) {
  const parts = [];
  const anchor = sourceAnchorLabel(record);
  if (anchor) {
    parts.push(`Matched link/resource: ${anchor}`);
  }
  if (record.firstInlinkSourcePosition !== null && record.firstInlinkSourcePosition !== undefined) {
    parts.push(`DOM position: ${record.firstInlinkSourcePosition}`);
  }
  return parts.length > 0
    ? parts.join("; ")
    : "The source page linked to this URL, but no anchor text was captured.";
}

function sourceAnchorLabel(record: CrawlRecord) {
  const anchor = record.firstInlinkAnchorText?.trim();
  if (!anchor) {
    return "";
  }
  return `"${anchor.length > 90 ? `${anchor.slice(0, 87)}...` : anchor}"`;
}

function missingFoundFromReason(record: CrawlRecord) {
  if (record.inSitemap && record.listPosition !== null && record.listPosition !== undefined) {
    return `Discovered from a sitemap imported in List mode, input row #${record.listPosition}`;
  }
  if (record.inSitemap && record.depth === 0) {
    return "Discovered from the site's /sitemap.xml, not from a page link";
  }
  if (record.inSitemap && record.inlinkCount === 0) {
    return "Discovered from a sitemap, but no crawled page linked to it";
  }
  if (record.inSitemap) {
    return "Discovered from a sitemap; page-link source not recorded yet";
  }
  if (record.listPosition !== null && record.listPosition !== undefined) {
    return `List mode input row #${record.listPosition}, not found from a crawled page`;
  }
  if (record.depth === 0) {
    return "Start URL entered in the crawl toolbar, not found from another page";
  }
  return "No source page recorded yet";
}

function compactUrl(value: string) {
  try {
    const url = new URL(value);
    const path = `${url.pathname}${url.search}`;
    return `${url.hostname}${path === "/" ? "" : path}`;
  } catch {
    return value;
  }
}

function flagLabel(value: boolean) {
  return value ? "present" : "missing";
}

function imageAssetStatus(image: ImageAsset) {
  if (image.oversized) {
    return "Oversized";
  }
  if (image.missingAlt) {
    return "Missing alt";
  }
  if (image.altTooLong) {
    return "Long alt";
  }
  return "OK";
}

function imageAssetMeta(image: ImageAsset) {
  const dimensions =
    image.width && image.height
      ? `${image.width}x${image.height}`
      : image.width
        ? `${image.width}px wide`
        : image.height
          ? `${image.height}px high`
          : "dimensions n/a";
  const size =
    typeof image.sizeBytes === "number" ? formatBytes(image.sizeBytes) : "size n/a";
  return `${dimensions}, ${size}`;
}

function estimateCrawlCapacity(
  config: CrawlConfig,
  storageMode: StorageMode,
): CrawlCapacityEstimate {
  const urlLimit = Math.max(1, Number(config.maxUrls) || 1);
  const resourceMultiplier =
    1 +
    (config.resourceTypes.images ? 0.35 : 0) +
    (config.resourceTypes.css ? 0.12 : 0) +
    (config.resourceTypes.javascript ? 0.16 : 0) +
    (config.resourceTypes.external ? 0.2 : 0) +
    (config.resourceTypes.other ? 0.1 : 0);
  const memoryBytesPerUrl =
    storageMode === "memory" ? 5_600 * resourceMultiplier : 1_100 * resourceMultiplier;
  const diskBytesPerUrl = storageMode === "database" ? 8_200 * resourceMultiplier : 0;
  const ramBytes = Math.ceil(urlLimit * memoryBytesPerUrl);
  const diskBytes = Math.ceil(urlLimit * diskBytesPerUrl);
  const deviceMemoryGb =
    typeof navigator !== "undefined"
      ? Number((navigator as Navigator & { deviceMemory?: number }).deviceMemory)
      : Number.NaN;
  const deviceBudgetBytes =
    Number.isFinite(deviceMemoryGb) && deviceMemoryGb > 0
      ? Math.floor(deviceMemoryGb * 1024 * 1024 * 1024 * 0.25)
      : undefined;

  const memoryRisk =
    storageMode === "memory" &&
    ((deviceBudgetBytes !== undefined && ramBytes > deviceBudgetBytes) || urlLimit > 100_000);
  const warningRisk =
    storageMode === "memory" &&
    ((deviceBudgetBytes !== undefined && ramBytes > deviceBudgetBytes * 0.65) ||
      urlLimit > 25_000);
  const tone = memoryRisk ? "danger" : warningRisk ? "warning" : "success";
  const recommendation = memoryRisk || warningRisk ? "Database" : storageMode === "database" ? "Database" : "Memory";

  return {
    urlLimit,
    ramBytes,
    diskBytes,
    deviceBudgetBytes,
    tone,
    recommendation,
  };
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

function customSearchCell(record: CrawlRecord, name: string) {
  const matches = record.customSearches.filter((search) => search.name === name);
  if (matches.length === 0) {
    return " ";
  }
  return matches
    .map((search) => `${customSearchSourceLabel(search.source)} ${search.matchCount}`)
    .join(" / ");
}

function customSearchSourceLabel(source: CustomSearchSource) {
  return source === "renderedHtml" ? "Rendered" : "Raw";
}

function isStatusBetween(status: number | null | undefined, min: number, max: number) {
  return typeof status === "number" && status >= min && status <= max;
}

function statusRowClass(record: CrawlRecord) {
  if (isStatusBetween(record.statusCode, 500, 599)) {
    return "status-server-error";
  }
  if (isStatusBetween(record.statusCode, 400, 499)) {
    return "status-client-error";
  }
  if (record.statusCode === null || record.statusCode === undefined) {
    return "status-no-response";
  }
  return "";
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

function formatSignedDelta(value: number) {
  if (value > 0) {
    return `+${value.toLocaleString()}`;
  }
  return value.toLocaleString();
}

function comparisonTone(change: string) {
  if (change === "removed") {
    return "error";
  }
  if (change === "changed") {
    return "warning";
  }
  return "info";
}

function compactHash(value?: string | null) {
  if (!value) {
    return "None";
  }
  return value.length > 10 ? `${value.slice(0, 10)}...` : value;
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
  if (statusFilter === "brokenLinks" || statusFilter === "redirectLinks") {
    const brokenTargets = brokenGraphTargets(graph);
    const matchingEdges = graph.edges.filter((edge) =>
      statusFilter === "brokenLinks"
        ? isBrokenGraphEdge(edge, brokenTargets)
        : isRedirectGraphEdge(edge),
    );
    const endpointUrls = new Set<string>();
    matchingEdges.forEach((edge) => {
      endpointUrls.add(edge.sourceUrl);
      endpointUrls.add(edge.targetUrl);
    });
    const nodes = graph.nodes.filter((node) => {
      if (!endpointUrls.has(node.url)) {
        return false;
      }
      if (selectedDepth !== undefined) {
        return node.depth === selectedDepth;
      }
      return true;
    });
    const nodeUrls = new Set(nodes.map((node) => node.url));
    const edges = matchingEdges.filter(
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
      return (typeof node.statusCode === "number" && node.statusCode >= 400) || (node.crawled && node.statusCode == null);
    case "brokenLinks":
      return true;
    case "redirectLinks":
      return true;
    case "external":
      return node.classification === "external";
    case "uncrawled":
      return !node.crawled;
    default:
      return true;
  }
}

function brokenGraphTargets(graph?: CrawlGraph) {
  return new Set(graph?.nodes.filter((node) => nodeMatchesGraphStatus(node, "broken")).map((node) => node.url));
}

function isBrokenGraphEdge(edge: LinkEdge, brokenTargets: Set<string>) {
  return (typeof edge.targetStatusCode === "number" && edge.targetStatusCode >= 400) || brokenTargets.has(edge.targetUrl);
}

function isRedirectGraphEdge(edge: LinkEdge) {
  return isStatusBetween(edge.targetStatusCode, 300, 399);
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
  if (!node.crawled) {
    return colors.border;
  }
  if (nodeMatchesGraphStatus(node, "broken")) return colors.danger;
  if (node.statusCode == null) return colors.border;
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
        opacity: edge.linkType === "external" ? 0.75 : 0.85,
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
        opacity: edge.linkType === "external" ? 0.75 : 0.85,
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
  return {
    primary: cssVar("--color-primary"),
    success: cssVar("--ui-success"),
    warning: cssVar("--ui-warning"),
    danger: cssVar("--ui-danger"),
    muted: cssVar("--ui-text-muted"),
    border: cssVar("--ui-text-toned"),
    surface: cssVar("--ui-bg-elevated"),
    text: cssVar("--ui-text-highlighted"),
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

function readPreference(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function savePreference(key: string, value: string): boolean {
  try {
    window.localStorage.setItem(key, value);
    return true;
  } catch {
    useAppStore.setState({ settingsError: "Settings could not be saved on this device. Your changes are active for this session; check available storage and change a setting to retry." });
    return false;
  }
}

function getInitialTheme(): ThemePreference {
  if (typeof window === "undefined") {
    return "system";
  }

  const storedTheme = readPreference(themeStorageKey);
  if (storedTheme === "light" || storedTheme === "dark") {
    return storedTheme;
  }

  return "system";
}

function resolveTheme(theme: ThemePreference): Theme {
  return theme === "system"
    ? (typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
    : theme;
}

function getInitialStartUrl() {
  if (typeof window === "undefined") {
    return "https://example.com/";
  }

  return readPreference(lastUrlStorageKey) ?? "https://example.com/";
}

function getInitialOverviewWidth() {
  if (typeof window === "undefined") {
    return 360;
  }

  const storedWidth = Number(readPreference(overviewWidthStorageKey) ?? 360);
  if (Number.isFinite(storedWidth)) {
    return clamp(storedWidth, overviewMinWidth, overviewMaxWidth);
  }

  return 360;
}

function getInitialUrlSegments(): UrlSegment[] {
  if (typeof window === "undefined") {
    return [];
  }

  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(urlSegmentsStorageKey) ?? "[]",
    );
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed
      .filter(
        (segment): segment is UrlSegment =>
          typeof segment?.id === "string" &&
          typeof segment?.name === "string" &&
          typeof segment?.pattern === "string",
      )
      .map((segment) => ({
        id: segment.id,
        name: segment.name,
        pattern: segment.pattern,
        regex: Boolean(segment.regex),
      }));
  } catch {
    return [];
  }
}

function newSegmentId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `segment-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function isoDateDaysAgo(days: number) {
  const date = new Date();
  date.setDate(date.getDate() - days);
  return date.toISOString().slice(0, 10);
}

function segmentQuery(segment?: UrlSegment) {
  const pattern = segment?.pattern.trim();
  if (!pattern) {
    return {};
  }
  return {
    segmentPattern: pattern,
    segmentRegex: Boolean(segment?.regex),
  };
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

function extractUrlsFromText(value: string) {
  const matches = value.match(/https?:\/\/[^\s"'<>),]+/gi) ?? [];
  return mergeUniqueValues(
    [],
    matches.map((url) => url.replace(/[.;\]]+$/g, "")),
  );
}

function mergeUniqueValues(existing: string[], incoming: string[]) {
  const seen = new Set<string>();
  const merged: string[] = [];
  for (const value of [...existing, ...incoming]) {
    const trimmed = value.trim();
    if (!trimmed || seen.has(trimmed)) {
      continue;
    }
    seen.add(trimmed);
    merged.push(trimmed);
  }
  return merged;
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
