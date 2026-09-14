// Run with: node scripts/smoke-ui.mjs (Chrome/Chromium required; CHROME_BIN overrides its path).
// This tests the real React screen against Tauri's IPC mock. Rust integration tests cover the engine.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createServer, preview } from "vite";

for (const file of (await readdir("dist/assets")).filter((name) => name.endsWith(".js"))) {
  const bytes = (await readFile(join("dist/assets", file))).length;
  assert.ok(bytes <= 500_000, `${file} is ${bytes.toLocaleString()} bytes; load optional tools separately instead of increasing the 500 kB limit`);
}

function setupFixture(mockIPC, emit) {
  window.isTauri = true;
  window.testStartupTheme = document.documentElement.classList.contains("dark") ? "dark" : "light";
  const summary = Object.fromEntries(`total internal external success redirects clientErrors serverErrors
    noResponse broken nearDuplicates exactDuplicates indexable nonIndexable titleMissing titleDuplicate titleMultiple metaMissing
    metaDuplicate metaMultiple h1Missing h1Duplicate h2Missing h2Duplicate canonicalMissing canonicalMultiple noindex
    canonicalUncrawled canonicalToRedirect canonicalToError canonicalNonIndexable canonicalChain canonicalLoop
    paginationNextToError paginationPrevToError paginationNextLoop paginationPrevLoop
    paginationNextNonReciprocal paginationPrevNonReciprocal paginationMultipleTargets ampToError ampNonReciprocal
    imagesMissingAlt imagesAltTooLong mixedContent insecureForms hreflangInvalid structuredDataInvalid
    structuredDataWarnings deprecatedHtmlTags duplicateIds renderedDomChanged missingViewport missingHsts
    sitemapOrphans`.split(/\s+/).map((key) => [key, 0]));
  Object.assign(summary, { total: 1205, internal: 1205, success: 1205, indexable: 1205 });
  const numericFields = `titleLen titlePixelWidth metaDescriptionLen metaDescriptionPixelWidth h1Len h1Count
    h2Len h2Count canonicalCount wordCount textToCodeRatio imageCount imagesMissingAlt imagesAltTooLong
    mixedContentCount insecureFormCount hreflangCount hreflangInvalidCount jsonLdCount jsonLdInvalidCount
    structuredDataErrorCount structuredDataWarningCount openGraphCount twitterCardCount deprecatedHtmlTagCount
    duplicateIdCount renderedWordCountDelta renderedLinkCountDelta resolvedIpCount inlinkCount outlinkCount
    internalOutlinkCount externalOutlinkCount listDuplicateIndex sizeBytes`.split(/\s+/);
  const records = Array.from({ length: summary.total }, (_, i) => ({
    ...Object.fromEntries(numericFields.map((key) => [key, 0])),
    id: i + 1, storageKey: `https://example.test/page-${i + 1}`,
    url: `https://example.test/page-${i + 1}`, finalUrl: `https://example.test/page-${i + 1}`,
    title: `Page ${i + 1}`, titleLen: 8, contentType: "text/html", classification: "internal",
    statusCode: 200, statusText: "OK", indexability: "Indexable", indexabilityStatus: "Indexable",
    responseTimeMs: 42, depth: 1, redirectChain: [], customExtractions: [], customSearches: [],
    hreflangLinks: [], structuredDataIssues: [],
  }));
  records[0].relNext = "https://example.test/missing-next";
  records[0].relPrev = "https://example.test/missing-previous";
  records[1].relNext = "https://example.test/unavailable-next";
  records[0].relNextTargets = ["https://example.test/missing-next", "https://example.test/other-next", "https://example.test/missing-next",
    ...Array.from({ length: 202 }, (_, index) => `https://example.test/extra-next-${index + 1}`)];
  records[0].relPrevTargets = ["https://example.test/missing-previous"];
  records[0].amphtml = "https://example.test/missing-amp";
  window.testQueries = [];
  window.testLinkQueries = [];
  window.testLinkDelays = {};
  window.testReferenceQueries = [];
  window.testReferenceDelays = {};
  window.testCaptureQueries = [];
  window.testCaptureDelays = {};
  window.testComparisonRequests = [];
  window.testAuditReports = [];
  window.testAuditRequests = [];
  window.testAuditCancels = [];
  window.testAuditComparisons = [];
  window.testPendingComparisons = [];
  window.testDuplicates = false;
  window.testEmptyDataset = false;
  window.testSearchDelays = {};
  window.testSettingsWrites = 0;
  window.testStartupCalls = 0;
  window.testQuitCalls = 0;
  window.testRenderingStatus = { available: true, browserPath: "/opt/chrome/chrome", message: "Compatible browser found." };
  window.testRenderingChecks = 0;
  window.testStartCalls = 0;
  window.testSessions = [
    { id: "fixture-current", name: "Example site", startUrl: "https://example.test/", mode: "spider", status: "completed", crawled: 1205, createdAtMs: 1788937200000, updatedAtMs: 1788937260000, databasePath: "/tmp/current.sqlite3", isCurrent: false },
    { id: "fixture-baseline", name: "Example baseline", startUrl: "https://example.test/", mode: "spider", status: "completed", crawled: 1200, createdAtMs: 1788850800000, updatedAtMs: 1788850860000, databasePath: "/tmp/baseline.sqlite3", isCurrent: false },
    { id: "fixture-other", name: "Another site", startUrl: "https://another.test/docs/", mode: "list", status: "stopped", crawled: 18, createdAtMs: 1788764400000, updatedAtMs: 1788764460000, databasePath: "/tmp/other.sqlite3", isCurrent: false },
  ];
  window.testUpdateChecks = 0;
  window.testUpdateFailure = new URL(location.href).searchParams.get("updates") === "error";
  window.testUpdateResult = {
    currentVersion: "0.1.0",
    update: new URL(location.href).searchParams.get("updates") === "available"
      ? { version: "0.2.0", releaseUrl: "https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/v0.2.0" }
      : null,
  };
  window.testOpenedUrls = [];
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (value) => {
    if (window.testClipboardFailure) throw new Error("Clipboard denied");
    window.testCopiedText = value;
  } } });
  const saveItem = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {
    if (key === "ferrous-frog-settings") window.testSettingsWrites++;
    if (window.testSettingsWriteFailure) throw new DOMException("Storage full", "QuotaExceededError");
    return saveItem.call(this, key, value);
  };
  window.testEmit = (payload) => emit("crawl-event", payload);
  window.testSetSummary = (patch) => Object.assign(summary, patch);
  window.testEmitProgress = (queued) => emit("crawl-event", { kind: "progress", progress: {
    status: "running", crawled: 1205, queued, discovered: 1205 + queued, elapsedMs: 1000, pagesPerSecond: 10, summary,
  } });
  window.testRequestQuit = () => emit("quit-requested");
  mockIPC(async (cmd, args) => {
    if (window.testHeldQueries?.[cmd]) {
      const response = window.testHeldQueries[cmd];
      delete window.testHeldQueries[cmd];
      return new Promise((resolve) => {
        (window.testPendingQueries ??= {})[cmd] = () => resolve(response);
      });
    }
    if (cmd === "get_rendering_status") {
      window.testRenderingChecks++;
      const result = { ...window.testRenderingStatus };
      const fail = window.testRenderingFailure;
      await new Promise((resolve) => setTimeout(resolve, window.testRenderingDelay ?? 15));
      if (fail) throw new Error("Could not check browser availability");
      return result;
    }
    if (cmd === "preview_content_area") {
      window.testContentPreviewRequest = args.request;
      if (window.testHoldContentPreview) await new Promise((resolve) => { window.testFinishContentPreview = resolve; });
      if (window.testContentPreviewFailure) throw new Error("Invalid content include selector at row 1");
      return { text: "Hello world", wordCount: 2, textToCodeRatio: 0.25, textTruncated: false };
    }
    if (cmd === "preview_custom_extractor") {
      window.testExtractionPreviewRequest = args.request;
      window.testExtractionPreviewCalls = (window.testExtractionPreviewCalls ?? 0) + 1;
      if (window.testHoldExtractionPreview) await new Promise((resolve) => { window.testFinishExtractionPreview = resolve; });
      if (args.request.extractor.pattern === '[') throw new Error("Invalid CSS selector for extraction");
      return { values: ['<img src=x onerror=alert(1)> Sample heading'], valuesTruncated: false, textTruncated: false };
    }
    if (cmd === "check_for_updates") {
      window.testUpdateChecks++;
      await new Promise((resolve) => setTimeout(resolve, window.testUpdateDelay ?? 15));
      if (window.testUpdateFailure) throw new Error("GitHub is unavailable. Please try again later.");
      return window.testUpdateResult;
    }
    if (cmd === "open_external_url") {
      if (window.testOpenUrlFailure) throw new Error("Could not open the browser");
      window.testOpenedUrls.push(args.url);
      return;
    }
    if (cmd === "complete_startup") {
      window.testStartupCalls++;
      window.testStartupHadHome = Boolean(document.querySelector('.crawl-home'));
      return;
    }
    if (cmd === "quit_app") {
      window.testQuitCalls++;
      await new Promise((resolve) => setTimeout(resolve, 100));
      if (window.testQuitFailure) throw new Error("Could not stop the active crawl");
      return;
    }
    if (cmd === "get_rows") {
      if (new URL(location.href).searchParams.has("startup-error")) throw new Error("Initial results could not be loaded");
      const query = { ...args.query };
      window.testQueries.push(query);
      if (window.testDuplicates) {
        for (const row of records.slice(0, 2)) { row.title = "Shared page title"; row.titleLen = 17; }
      }
      let matching = query.view === "titleDuplicate" ? (window.testDuplicates ? records.slice(0, 2) : []) : records;
      if (query.view === "paginationNextToError" || query.view === "paginationPrevToError") matching = window.testPaginationErrors ? records.slice(0, query.view === "paginationNextToError" ? 2 : 1) : [];
      if (query.view === "paginationNextLoop" || query.view === "paginationPrevLoop") matching = window.testPaginationLoops
        ? records.slice(0, query.view === "paginationNextLoop" ? 2 : 1).map((row, index) => ({ ...row,
          relNext: records[1 - index].url, relPrev: row.url })) : [];
      if (query.view === "paginationNextNonReciprocal" || query.view === "paginationPrevNonReciprocal") matching = window.testPaginationReturns
        ? records.slice(0, query.view === "paginationNextNonReciprocal" ? 2 : 1).map((row) => ({ ...row,
          relNext: 'https://example.test/next-without-return', relPrev: 'https://example.test/prev-without-return' })) : [];
      if (query.view === "paginationMultipleTargets") matching = window.testPaginationMultiple ? records.slice(0, 1) : [];
      if (query.view === "ampToError") matching = window.testAmpError ? records.slice(0, 1) : [];
      if (query.view === "ampNonReciprocal") matching = window.testAmpReturns
        ? records.slice(0, 1).map((row) => ({ ...row, amphtml: 'https://example.test/amp-without-return' })) : [];
      if (query.view === "titleMultiple" || query.view === "metaMultiple") matching = window.testMultipleMetadata
        ? [{ ...records[0], titleCount: 3, metaDescriptionCount: 2 }] : [];
      if (window.testEmptyDataset) matching = [];
      if (query.globalSearch) matching = matching.filter((row) => row.finalUrl.includes(query.globalSearch));
      if (query.filters?.rules.length) matching = matching.filter((row) => {
        const checks = query.filters.rules.map((rule) => rule.operator === 'greaterThan'
          ? row[rule.field] > Number(rule.value)
          : String(row[rule.field] ?? '').toLowerCase().includes(rule.value.toLowerCase()));
        return query.filters.match === 'any' ? checks.some(Boolean) : checks.every(Boolean);
      });
      if (query.sortBy === "titleCount" || query.sortBy === "metaDescriptionCount") {
        matching = [...matching].sort((a, b) => ((a[query.sortBy] ?? -1) - (b[query.sortBy] ?? -1)) * (query.sortDir === "desc" ? -1 : 1));
      } else if (query.sortBy) matching = [...matching].reverse();
      const response = { rows: matching.slice(query.offset, query.offset + query.limit), total: matching.length,
        summary: { ...summary, titleDuplicate: window.testDuplicates ? 2 : 0, exactDuplicates: window.testExactDuplicates ?? 0,
          paginationNextToError: window.testPaginationErrors ? 2 : 0, paginationPrevToError: window.testPaginationErrors ? 1 : 0,
          paginationNextLoop: window.testPaginationLoops ? 2 : 0, paginationPrevLoop: window.testPaginationLoops ? 1 : 0,
          paginationNextNonReciprocal: window.testPaginationReturns ? 2 : 0, paginationPrevNonReciprocal: window.testPaginationReturns ? 1 : 0,
          paginationMultipleTargets: window.testPaginationMultiple ? 1 : 0,
          ampToError: window.testAmpError ? 1 : 0, ampNonReciprocal: window.testAmpReturns ? 1 : 0,
          titleMultiple: window.testMultipleMetadata ? 1 : 0, metaMultiple: window.testMultipleMetadata ? 1 : 0 } };
      await new Promise((resolve) => setTimeout(resolve, window.testSearchDelays[query.globalSearch] ?? 15));
      return response;
    }
    if (cmd === "get_recovery_state") return window.testRecovery ?? { recoverable: false, queued: 0, seen: 0, crawled: 0 };
    if (cmd === "get_image_assets") return { images: [], total: 0 };
    if (cmd === "get_crawl_path") return { found: false, hops: [], capped: false };
    if (cmd === "get_url_tree") {
      window.testTreeQuery = args.query;
      const totalUrls = window.testTreeTotal ?? 1205;
      await new Promise((resolve) => setTimeout(resolve, window.testTreeDelay ?? 0));
      return { nodes: [], totalUrls, renderedUrls: 0, capped: true };
    }
    if (cmd === "validate_result_filters") {
      window.testFilterValidation = args.filters;
      if (window.testHoldFilterValidation) await new Promise((resolve) => { window.testFinishFilterValidation = resolve; });
      if (window.testFilterValidationFailure) throw new Error("Invalid filter value");
      return;
    }
    if (cmd === "get_crawl_graph") {
      if (window.testGraphFailure) throw new Error("Graph query failed");
      const totalNodes = window.testGraphTotalNodes ?? 4;
      await new Promise((resolve) => setTimeout(resolve, window.testGraphDelay ?? 0));
      return window.testGraphSnapshot ?? {
      nodes: ["root", "offline", "blocked", "pending"].map((label) => ({
        url: `https://example.test/${label}`, label, statusCode: label === "root" ? 200 : null,
        crawled: label === "root" || label === "offline", classification: "internal", depth: 1,
        inlinkCount: 1, outlinkCount: 0,
      })),
      edges: ["offline", "blocked", "pending"].map((label, id) => ({
        id, sourceUrl: "https://example.test/root", targetUrl: `https://example.test/${label}`,
        anchorText: label, linkType: "internal", sourceStatusCode: 200, targetStatusCode: null,
        sourceDepth: 1, targetDepth: 1, rel: "", relNofollow: false,
      })), totalNodes, totalEdges: 3,
      };
    }
    if (cmd === "get_page_capture") {
      window.testCaptureQueries.push(args);
      const retained = window.testCaptureRetained && args.kind !== "renderedHtml";
      const result = retained ? { ...args, sourceUrl: args.sourceStorageKey, finalUrl: "https://example.test/final", storedTruncated: args.kind === "rawHtml", previewTruncated: args.kind === "visibleText",
        text: args.kind === "responseHeaders" ? '[{"name":"set-cookie","value":"[redacted]"},{"name":"x-fixture","value":"one"},{"name":"x-fixture","value":"two"}]'
          : args.kind === "rawHtml" ? '<main><img src=x onerror="window.testCaptureExecuted = true">Saved ' + args.sourceStorageKey + '</main>' : 'Visible text for ' + args.sourceStorageKey } : null;
      const fail = window.testCaptureFailure;
      await new Promise((resolve) => setTimeout(resolve, window.testCaptureDelays[args.sourceStorageKey + '|' + args.kind] ?? 15));
      if (fail) throw new Error("Captured data could not be read");
      return result;
    }
    if (cmd === "page_references") {
      window.testReferenceQueries.push(args.query);
      const sourceStorageKey = args.query.sourceStorageKey;
      const session = window.testOpenedSession ?? "new-crawl";
      await new Promise((resolve) => setTimeout(resolve, window.testReferenceDelays[sourceStorageKey] ?? 15));
      if (window.testReferenceFailure) throw new Error("Reference query failed");
      const total = window.testReferenceTotal ?? 205;
      const kinds = ["canonical", "hreflang", "pagination", "amp", "metaRefresh", "iframe"];
      return { references: Array.from({ length: Math.max(0, Math.min(args.query.limit, total - args.query.offset)) }, (_, index) => {
        const id = args.query.offset + index + 1;
        return { id, sourceStorageKey, sourceUrl: "https://example.test/reference-source",
          targetUrl: `https://references.test/${session}/${encodeURIComponent(sourceStorageKey)}/${id}`,
          kind: kinds[(id - 1) % kinds.length], relNofollow: id % 2 === 0 };
      }), total };
    }
    if (cmd === "get_link_edges") {
      window.testLinkQueries.push(args.query);
      await new Promise((resolve) => setTimeout(resolve, window.testLinkDelays[args.query.targetUrl] ?? 15));
      if (window.testLinkFailure) throw new Error("Link query failed");
      const matching = records.filter((row) => !args.query.globalSearch || `Link ${row.id} ${row.url}`.includes(args.query.globalSearch));
      if (args.query.sortDir === "desc") matching.reverse();
      return { edges: matching.slice(args.query.offset, args.query.offset + args.query.limit).map((row) => ({
      id: row.id, sourceUrl: args.query.sourceUrl ?? row.url, targetUrl: args.query.targetUrl ?? row.url,
      anchorText: `Link ${row.id}`, linkType: "internal", rel: "", relNofollow: false,
      sourceStatusCode: 200, targetStatusCode: 200, sourceDepth: 1, discoveryOrder: row.id,
    })), total: matching.length };
    }
    if (cmd === "get_sitemap_validation") {
      window.testSitemapQuery = args.query;
      const rows = [404, 200].map((statusCode) => ({
        url: "https://example.test/repeated", finalUrl: "https://example.test/repeated", statusCode,
        statusText: statusCode === 200 ? "OK" : "Not Found", issueCount: statusCode === 200 ? 0 : 1,
        indexability: statusCode === 200 ? "Indexable" : "Non-Indexable",
        indexabilityStatus: statusCode === 200 ? "Indexable" : "Client Error", inlinkCount: 1,
        severity: statusCode === 200 ? "info" : "error", issues: statusCode === 200 ? [] : ["4xx response"],
      }));
      if (args.query.sortBy === "statusCode" && args.query.sortDir === "asc") rows.reverse();
      return { rows: rows.slice(args.query.offset, args.query.offset + args.query.limit), total: rows.length };
    }
    if (cmd === "list_crawl_sessions") {
      if (window.testHistoryFailure || new URL(location.href).searchParams.has("startup-error")) throw new Error("Crawl history could not be loaded");
      return window.testSessions;
    }
    if (cmd === "open_crawl_session") {
      if (window.testOpenSessionFailure) throw new Error("Saved crawl database is missing");
      window.testOpenedSession = args.sessionId;
      window.testSessions = window.testSessions.map((session) => ({ ...session, isCurrent: session.id === args.sessionId }));
      return { ...window.testSessions.find((session) => session.id === args.sessionId), config: window.testSessionConfig };
    }
    if (cmd === "delete_crawl_session") {
      window.testDeleteCalls = (window.testDeleteCalls ?? 0) + 1;
      if (window.testHoldDeletion) await new Promise((resolve) => { window.testFinishDeletion = resolve; });
      if (window.testDeleteFailure) throw new Error("Could not delete this crawl database");
      if (!window.testSessions.some((session) => session.id === args.sessionId)) throw new Error("Unknown saved crawl");
      window.testSessions = window.testSessions.filter((session) => session.id !== args.sessionId);
      if (window.testOpenedSession === args.sessionId) {
        window.testOpenedSession = undefined;
        window.testEmptyDataset = true;
      }
      return;
    }
    if (cmd === "open_crawl_comparison" || cmd === "query_crawl_comparison" || cmd === "export_crawl_comparison") {
      const opening = cmd === "open_crawl_comparison";
      const request = opening ? args.request : args;
      const query = opening ? { search: "", change: "all", includeResponseOnly: false, sortBy: "url", sortDir: "asc", offset: 0, limit: 100 } : args.query;
      window.testComparisonRequests.push({ command: cmd, ...request, ...query });
      if (opening) {
        if (request.baselineSessionId) window.testComparisonIds = [request.baselineSessionId, request.currentSessionId];
        if (window.testCompareFailure) throw new Error("Saved crawl could not be compared");
        const responseCount = window.testComparisonOnlyResponses ? 205 : 500;
        const responseRows = Array.from({ length: responseCount }, (_, index) => ({ key: 10000 + index, url: `https://example.test/response-only-${String(index).padStart(4, '0')}`, change: "responseOnly", changedFields: ["responseHash"], contentComparison: "unavailable", previousResponseHash: "response-old", currentResponseHash: "response-new" }));
        const extra = window.testComparisonManyRows ? 1250 : 0;
        const snapshot = window.testComparisonOnlyResponses
          ? { baselineRecords: 205, currentRecords: 205, added: 0, removed: 0, changed: 0, statusChanged: 0, titleChanged: 0, metaDescriptionChanged: 0,
            indexabilityChanged: 0, hashChanged: 205, contentChanged: 0, responseOnly: 205, contentUnavailable: 205, metricDeltas: [], rows: responseRows }
          : { baselineRecords: 1200 + extra, currentRecords: 1205 + extra, added: 8, removed: 3, changed: 12 + extra, statusChanged: 2,
            titleChanged: 8 + extra, metaDescriptionChanged: 3, indexabilityChanged: 1, hashChanged: 512, contentChanged: 4, responseOnly: 500, contentUnavailable: 33,
            metricDeltas: [{ label: "Structured data errors", previous: 2, current: 1, delta: -1 }],
            rows: [{ key: 1, url: "https://example.test/changed", change: "changed", previousStatusCode: 404, currentStatusCode: 200, previousTitle: "Old title", currentTitle: "New title",
              previousMetaDescription: "Old description", currentMetaDescription: "New description", changedFields: ["statusCode", "title", "metaDescription", "content"], contentComparison: "changed" },
            { key: 2, url: "https://example.test/legacy", change: "changed", changedFields: ["headings", "canonical", "robotsDirectives"], contentComparison: "unavailable" },
            ...Array.from({ length: 21 + extra }, (_, index) => ({ key: index + 3, url: `https://example.test/zz-page-${String(index).padStart(4, '0')}`, change: index < 8 ? "added" : index < 11 ? "removed" : "changed", previousTitle: `Before ${index}`, currentTitle: `After ${index}`, changedFields: index < 11 ? [] : ["title"], contentComparison: index < 11 ? "notApplicable" : "unchanged" })), ...responseRows] };
        if (window.testComparisonIdentities) {
          Object.assign(snapshot, { baselineRecords: 4, currentRecords: 4, added: 0, removed: 0, changed: 4, responseOnly: 0,
            statusChanged: 0, titleChanged: 4, metaDescriptionChanged: 0, indexabilityChanged: 0, hashChanged: 0, contentChanged: 0, contentUnavailable: 4, metricDeltas: [],
            rows: [
              { key: 41, url: 'https://example.test/repeated', occurrence: 1, previousListPosition: 2, currentListPosition: 7, previousUrl: 'HTTPS://EXAMPLE.TEST:443/repeated#old', currentUrl: 'https://example.test/repeated#new', previousFinalUrl: 'https://example.test/old-target', currentFinalUrl: 'https://example.test/new-target', previousTitle: 'First occurrence', currentTitle: 'First updated', changedFields: ['finalUrl', 'title'] },
              { key: 42, url: 'https://example.test/repeated', occurrence: 2, previousListPosition: 5, currentListPosition: 8, previousTitle: 'Second occurrence', currentTitle: 'Second updated', changedFields: ['title'] },
              { key: 43, url: 'https://example.test/alias-a', previousFinalUrl: 'https://example.test/shared', currentFinalUrl: 'https://example.test/shared', changedFields: ['title'] },
              { key: 44, url: 'https://example.test/alias-b', previousFinalUrl: 'https://example.test/shared', currentFinalUrl: 'https://example.test/shared', changedFields: ['title'] },
            ].map((row) => ({ change: 'changed', contentComparison: 'unavailable', ...row })) });
        }
        snapshot.rows = snapshot.rows.map((row) => ({ occurrence: 1, previousUrl: row.change === 'added' ? null : row.url, currentUrl: row.change === 'removed' ? null : row.url,
          previousFinalUrl: row.change === 'added' ? null : row.url, currentFinalUrl: row.change === 'removed' ? null : row.url, previousListPosition: null, currentListPosition: null,
          ...row, identityKey: `${row.occurrence ?? 1}:${row.url}` }));
        (window.testComparisonSnapshots ??= {})[request.comparisonId] = snapshot;
      }
      const snapshot = window.testComparisonSnapshots?.[request.comparisonId];
      if (!snapshot) throw new Error("Comparison is closed");
      const filtered = snapshot.rows.filter((row) => (query.includeResponseOnly || query.change === "responseOnly" || row.change !== "responseOnly")
        && (query.change === "all" || row.change === query.change) && (!query.changedField || row.changedFields.includes(query.changedField))
        && (!query.search || [row.url, row.previousTitle, row.currentTitle].join(' ').toLowerCase().includes(query.search.toLowerCase())));
      filtered.sort((a, b) => {
        const left = a[query.sortBy] ?? '', right = b[query.sortBy] ?? '';
        const comparison = typeof left === "number" && typeof right === "number" ? left - right : String(left).localeCompare(String(right));
        return (comparison || a.key - b.key) * (query.sortDir === "desc" ? -1 : 1);
      });
      if (cmd === "export_crawl_comparison") {
        window.testComparisonExport = { comparisonId: args.comparisonId, query, rows: filtered.length };
        if (window.testHoldComparisonExport) await new Promise((resolve) => { window.testFinishComparisonExport = resolve; });
        return { path: "/tmp/comparison.csv", rowCount: filtered.length };
      }
      const response = { summary: { ...snapshot, rows: [] }, total: filtered.length, offset: query.offset, limit: query.limit, rows: filtered.slice(query.offset, query.offset + query.limit) };
      if (window.testHoldComparison) await new Promise((resolve) => { window.testPendingComparisons.push(resolve); });
      // A delayed async native handler can publish after the frontend has already closed its ID.
      if (opening && window.testLateComparisonPublication) window.testComparisonSnapshots[request.comparisonId] = snapshot;
      return response;
    }
    if (cmd === "get_crawl_comparison_detail") {
      const row = window.testComparisonSnapshots?.[args.comparisonId]?.rows.find((item) => item.key === args.key);
      if (!row) throw new Error("Comparison row is missing");
      const result = { row, previous: row.change === "added" ? null : { ...records[0], url: row.previousUrl, finalUrl: row.previousFinalUrl, listPosition: row.previousListPosition, title: row.previousTitle, metaDescription: row.previousMetaDescription, statusCode: row.previousStatusCode,
        structuredDataIssues: [{ severity: "error", path: "$.offers", message: "Missing offer price" }], contentHash: "previous-content-hash", responseHash: "previous-full-response-hash" },
        current: row.change === "removed" ? null : { ...records[0], url: row.currentUrl, finalUrl: row.currentFinalUrl, listPosition: row.currentListPosition, title: row.currentTitle, metaDescription: row.currentMetaDescription, statusCode: row.currentStatusCode, structuredDataIssues: [], contentHash: "current-content-hash", responseHash: "current-full-response-hash" } };
      if (window.testHoldComparisonDetail) await new Promise((resolve) => { (window.testPendingComparisonDetails ??= []).push(resolve); });
      return result;
    }
    if (cmd === "close_crawl_comparison") {
      (window.testClosedComparisons ??= []).push(args.comparisonId);
      delete window.testComparisonSnapshots?.[args.comparisonId];
      return;
    }
    if (cmd === "list_audit_reports") return { rows: window.testAuditReports.slice(args.offset, args.offset + args.limit), total: window.testAuditReports.length };
    if (cmd === "prepare_audit_report") {
      if (window.testHoldAuditPrepare) await new Promise((resolve) => { window.testFinishAuditPrepare = resolve; });
      const request = { id: args.request.requestId, ...args.request, sourceStatus: "completed" };
      const report = { request, status: "ready", partial: false, sourceRecords: 1205, scopeRecords: 1205, eligibleHtmlRecords: 1205, blockedRecords: 0, failedRecords: 0, unavailableHtmlRecords: 0, findingCount: 1, coverage: [{ ruleId: "title.missing", state: "measured", eligibleRecords: 1205, reason: "Captured HTML titles." }] };
      window.testAuditReports.unshift(report); window.testAuditRequests.push({ command: cmd, ...args.request }); return report;
    }
    if (cmd === "get_audit_report") return window.testAuditReports.find((report) => report.request.id === args.reportId);
    if (cmd === "list_audit_report_comparisons") return { rows: window.testAuditComparisons.slice(args.offset, args.offset + args.limit), total: window.testAuditComparisons.length };
    if (cmd === "prepare_audit_report_comparison") {
      const baseline = window.testAuditReports.find((report) => report.request.id === args.baselineReportId); const current = window.testAuditReports.find((report) => report.request.id === args.currentReportId); if (!baseline || !current) throw new Error("Comparison reports are missing");
      const item = { id: args.requestId, summary: { schemaVersion: 1, identityVersion: "request-url-occurrence-v1", status: "ready", baselineReportId: baseline.request.id, currentReportId: current.request.id, baseline, current, findingCount: 2, evidenceRows: 2, compatibilityReasons: [] } }; window.testAuditComparisons.unshift(item); return item;
    }
    if (cmd === "get_audit_report_comparison") return window.testAuditComparisons.find((item) => item.id === args.comparisonId);
    if (cmd === "query_audit_report_comparison_findings") {
      const rows = [{ findingId: "title.missing", title: "Missing page title", severity: "error", category: "Titles", status: "resolved", added: 0, persisting: 0, resolved: 1, notObserved: 0, newlyObservedCurrent: 0, unverifiedCurrent: 0, countUnit: "original requests", compatibilityReasons: [] }, { findingId: "links.broken", title: "Broken links", severity: "warning", category: "Links", status: "notComparable", added: 0, persisting: 0, resolved: 0, notObserved: 1, newlyObservedCurrent: 0, unverifiedCurrent: 1, countUnit: "targets", compatibilityReasons: ["The rule was not measured with compatible coverage in both reports."] }]; const filtered = rows.filter((row) => (!args.query.status || row.status === args.query.status) && (!args.query.search || row.title.toLowerCase().includes(args.query.search.toLowerCase()))); return { rows: filtered.slice(args.query.offset, args.query.offset + args.query.limit), total: filtered.length };
    }
    if (cmd === "query_audit_report_comparison_evidence") {
      if (window.testHoldComparisonEvidence) await new Promise((resolve) => { window.testFinishComparisonEvidence = resolve; });
      const rows = args.query.findingId === "title.missing" ? [{ id: 1, findingId: "title.missing", identityKey: "row-1", requestUrl: "https://example.test/fixed", occurrence: 1, state: "resolved", reason: "The baseline observation was not present in the current report.", baseline: { originalUrl: "https://example.test/fixed", observed: { title: "" } }, current: { originalUrl: "https://example.test/fixed", observed: { title: "Fixed title" } }, previewTruncated: false }] : [{ id: 2, findingId: "links.broken", identityKey: "row-2", requestUrl: "https://example.test/not-observed", occurrence: 1, state: "notObserved", reason: "Current evidence was not observed; this does not verify a fix.", baseline: { originalUrl: "https://example.test/not-observed", observed: { statusCode: 404 } }, current: null, previewTruncated: false }]; const filtered = rows.filter((row) => (!args.query.state || row.state === args.query.state) && (!args.query.search || row.requestUrl.includes(args.query.search))); return { rows: filtered.slice(args.query.offset, args.query.offset + args.query.limit), total: filtered.length };
    }
    if (cmd === "export_audit_report_comparison") { window.testAuditRequests.push({ command: cmd, ...args }); return { path: "/tmp/ferrous-report-comparison/index.html", rowCount: 2 }; }
    if (cmd === "delete_audit_report_comparison") { window.testAuditComparisons = window.testAuditComparisons.filter((item) => item.id !== args.comparisonId); return; }
    if (cmd === "get_audit_report_comparison_ai") return null;
    if (cmd === "preview_audit_report_comparison_ai") { window.testAuditRequests.push({ command: cmd, ...args }); return { version: "fixture-comparison-ai-v1", previewDigest: "fixture-comparison-preview-digest", provider: "Fixture provider", model: "fixture-comparison-model", endpoint: "https://ai.fixture.test", findingCount: 2, completedFindings: 0, pendingFindings: 2, estimatedRequests: 1, estimatedInputChars: 1200, sampledEvidence: 2, overviewPending: true, overviewPlanned: true, samplingPolicy: "One stored comparison observation per finding.", dataCategories: ["Measured comparison states", "Before and after observations"], options: args.options }; }
    if (cmd === "run_audit_report_comparison_ai") { window.testAuditRequests.push({ command: cmd, ...args.request }); if (window.testComparisonAiError) throw new Error("Comparison AI provider unavailable"); return { version: args.request.expectedVersion, status: "completed", provider: "Fixture provider", model: "fixture-comparison-model", findingCount: 2, completedFindings: 2, requests: 1, inputChars: 1200, rows: [{ findingId: "title.missing", evidenceIds: ["1"], explanation: "The title issue was resolved.", recommendation: "Keep the title check.", verification: "Run another frozen comparison.", suggestedTeam: "Content", model: "fixture-comparison-model", generatedAtMs: 0, sampleCount: 1, evidenceTotal: 1 }] }; }
    if (cmd === "get_audit_report_ai") return null;
    if (cmd === "preview_audit_report_ai") return { version: "fixture-ai-v1", previewDigest: "fixture-preview-digest", provider: "Fixture provider", model: "fixture-model", endpoint: "https://ai.fixture.test", findingCount: 205, completedFindings: 0, pendingFindings: 205, estimatedRequests: 10, estimatedInputChars: 12_000, sampledEvidence: 30, overviewPending: true, overviewPlanned: true, samplingPolicy: "Up to 3 stored examples per finding.", dataCategories: ["Finding titles", "Measured counts", "Captured URL examples"], options: args.options };
    if (cmd === "run_audit_report_ai") { if (window.testAuditAiError) throw new Error("AI provider unavailable"); const preservedGeneration = { version: args.request.expectedVersion, status: "completed", provider: "Fixture provider", model: "fixture-model", findingCount: 205, completedFindings: 205, requests: 10, inputChars: 12_000, overview: { summary: "<strong>Fixture overview</strong>", prioritizedFindingIds: ["title.missing"], limitations: ["<em>Only validated annotations are included.</em>"], model: "fixture-model", generatedAtMs: 0, includedFindingCount: 2, totalFindingCount: 205, sampledEvidenceCount: 30, partialCoverage: true }, rows: [{ findingId: "title.missing", evidenceIds: ["evidence-1"], explanation: "Fixture explanation.", recommendation: "Add titles.", verification: "Recrawl.", suggestedTeam: "Content", model: "fixture-model", generatedAtMs: 0, sampleCount: 1, evidenceTotal: 1205 }] }; return { version: "fixture-ai-v2", status: "failed", provider: "Fixture provider", model: "failed-fixture-model", findingCount: 205, completedFindings: 0, requests: 1, inputChars: 1200, error: "Fixture provider timed out", rows: [], exportGenerationVersion: args.request.expectedVersion, preservedGeneration }; }
    if (cmd === "delete_audit_report") { if (window.testAuditDeleteError) throw new Error("Could not delete this report"); window.testAuditReports = window.testAuditReports.filter((report) => report.request.id !== args.reportId); return; }
    if (cmd === "query_audit_report_findings") {
      window.testAuditRequests.push({ command: cmd, ...args.query }); const rows = [{ id: "title.missing", title: "Missing page title", severity: "error", category: "Titles", counts: { uniqueUrls: 1205, sourceRecords: 1205, sourcePages: 1205, occurrences: 1205 }, coverage: "measured", explanation: "Captured pages have no title.", recommendation: "Add a descriptive title.", verification: "Recrawl and confirm the title is present.", suggestedTeam: "Content" }, ...Array.from({ length: 204 }, (_, index) => ({ id: `fixture-${index + 1}`, title: `Fixture finding ${index + 1}`, severity: "info", category: "Fixture", counts: { uniqueUrls: 1, sourcePages: 1, occurrences: 1 }, coverage: "measured", explanation: "Fixture finding.", recommendation: "Review fixture.", verification: "Verify fixture.", suggestedTeam: "Engineering" }))];
      const filtered = rows.filter((row) => (!args.query.search || row.title.toLowerCase().includes(args.query.search.toLowerCase())) && (!args.query.category || row.category.toLowerCase().includes(args.query.category.toLowerCase())) && (!args.query.team || row.suggestedTeam.toLowerCase().includes(args.query.team.toLowerCase())) && (!args.query.severity || row.severity === args.query.severity)); return { rows: filtered.slice(args.query.offset, args.query.offset + args.query.limit), total: filtered.length };
    }
    if (cmd === "query_audit_report_evidence") {
      window.testAuditRequests.push({ command: cmd, ...args.query }); const rows = Array.from({ length: 1205 }, (_, index) => ({ id: `evidence-${index + 1}`, originalUrl: `https://example.test/report-${String(index + 1).padStart(4, "0")}`, finalUrl: `https://example.test/report-${String(index + 1).padStart(4, "0")}`, kind: "page", attribution: "exactRecord", listPosition: index + 1, previewTruncated: index === 1204, observed: { title: index === 1204 ? "Last observed title" : "", statusCode: index === 1204 ? 404 : 200 } }));
      const filtered = rows.filter((row) => (!args.query.search || row.originalUrl.includes(args.query.search)) && (!args.query.statusCode || row.observed.statusCode === args.query.statusCode)); return { rows: filtered.slice(args.query.offset, args.query.offset + args.query.limit), total: filtered.length };
    }
    if (cmd === "export_audit_report") { if (window.testHoldAuditExport) await new Promise((resolve) => { window.testFinishAuditExport = resolve; }); return { path: "/tmp/ferrous-audit-report/index.html", rowCount: 1205 }; }
    if (cmd === "export_audit_report_evidence") { window.testAuditRequests.push({ command: cmd, ...args.query }); if (window.testHoldAuditEvidenceExport) await new Promise((resolve) => { window.testFinishAuditEvidenceExport = resolve; }); return { path: "/tmp/ferrous-audit-report/title-missing.csv", rowCount: 1 }; }
    if (cmd === "cancel_audit_report") { window.testAuditCancels.push(args.requestId); return true; }
    if (cmd === "list_config_profiles") return window.testProfiles ?? (window.testProfile ? [window.testProfile] : []);
    if (cmd === "load_config_profile") {
      const profile = (window.testProfiles ?? [window.testProfile]).find((item) => item.id === args.profileId);
      const failure = window.testProfileFailure;
      if (window.testHoldProfiles) await new Promise((resolve) => { window.testProfileLoads[args.profileId] = resolve; });
      if (failure) throw new Error("Could not load this profile");
      return profile;
    }
    if (cmd === "get_database_location") return { path: "/tmp/test-crawl.sqlite3" };
    if (cmd === "export_file") {
      window.testExportRequest = args.request;
      if (window.testHoldExport) await new Promise((resolve) => { window.testFinishExport = resolve; });
      if (window.testExportFailure) throw new Error(args.request.kind === "crawlArchive" ? "Could not write the crawl archive" : "Could not write the audit workbook");
      if (args.request.kind === "crawlArchive") return { path: "/tmp/ferrous-frog.ffcrawl.json", rowCount: records.length };
      if (args.request.kind === "selectedCsv") return { path: "/tmp/selected.csv", rowCount: args.request.recordIds.length };
      if (args.request.kind === "queuedUrlsCsv") return { path: "/tmp/queued.csv", rowCount: 7 };
      return { path: "/tmp/ferrous-frog-audit.xlsx", rowCount: records.length };
    }
    if (cmd === "export_selected_csv") {
      window.testCopiedIds = args.recordIds;
      if (window.testSelectedExportFailure) throw new Error("The selected rows could not be read");
      return 'url,title\n' + args.recordIds.map((id) => `https://example.test/page-${id},Page ${id}`).join('\n');
    }
    if (cmd === "start_crawl") {
      window.testStartCalls++;
      if (args.config.rendering.enabled && !window.testRenderingStatus.available) {
        throw new Error(`${window.testRenderingStatus.message} Turn off Render DOM in Settings to crawl HTML.`);
      }
      window.testStartedSeed = args.config.startUrl;
      window.testStartedList = args.config.listUrls;
      window.testStartedScope = [args.config.subdomainScope, args.config.folderScope];
      window.testStartedResume = args.resume;
      if (window.testStartFailure) throw new Error("Could not create the crawl database");
      const session = { ...window.testSessions[0], id: `new-${window.testStartCalls}`, name: args.config.startUrl || "URL list",
        startUrl: args.config.startUrl, mode: args.config.mode, status: "running", crawled: 0, isCurrent: true };
      window.testSessions = [session, ...window.testSessions.map((item) => ({ ...item, isCurrent: false }))];
      window.testEmit({ kind: "started" });
      return session;
    }
    if (cmd === "pause_crawl" || cmd === "resume_crawl" || cmd === "stop_crawl") return;
    if (cmd === "open_database_path") {
      window.testWorkspaceCalls = (window.testWorkspaceCalls ?? 0) + 1;
      if (window.testHoldWorkspace) await new Promise((resolve) => { window.testFinishWorkspace = resolve; });
      if (args.path === "/invalid/crawl.sqlite3") throw new Error("Cannot open this database");
      window.testEmptyDataset = true;
      return { path: args.path };
    }
    if (cmd === "get_search_console_credential_status") return { tokenSaved: false, keyringAvailable: true };
    const googleStatus = () => ({ clientConfigured: Boolean(window.testGoogleClient), clientId: window.testGoogleClient?.clientId ?? null, connected: Boolean(window.testGoogleConnected), refreshable: Boolean(window.testGoogleConnected), expiresAtMs: window.testGoogleConnected ? 1788944400000 : null, scopes: window.testGoogleConnected ? ["https://www.googleapis.com/auth/webmasters.readonly"] : [], keyringAvailable: true, message: null });
    if (cmd === "get_google_oauth_status") return googleStatus();
    if (cmd === "save_google_oauth_client") { window.testGoogleClient = args.request; return googleStatus(); }
    if (cmd === "connect_google_account") { if (!window.testGoogleClient) throw new Error("Save the OAuth client ID and secret before connecting a Google account"); window.testGoogleConnected = true; return googleStatus(); }
    if (cmd === "disconnect_google_account") { window.testGoogleConnected = false; if (args.clearClient) window.testGoogleClient = undefined; return googleStatus(); }
    if (cmd === "get_analytics_property") return window.testAnalyticsProperty ?? null;
    const backlinkStatus = () => ({ endpointTemplate: window.testBacklinks?.endpointTemplate ?? "", headerName: window.testBacklinks?.headerName ?? "", credentialSaved: Boolean(window.testBacklinkCredential), keyringAvailable: true });
    if (cmd === "get_backlink_settings") return backlinkStatus();
    if (cmd === "save_backlink_settings") {
      if (args.request.endpointTemplate && !args.request.endpointTemplate.includes("{url}")) throw new Error("The backlink endpoint must be an HTTP(S) URL template containing {url}");
      window.testBacklinks = args.request.endpointTemplate ? { endpointTemplate: args.request.endpointTemplate, headerName: args.request.headerName } : undefined;
      if (args.request.headerValue) window.testBacklinkCredential = args.request.headerValue;
      return backlinkStatus();
    }
    if (cmd === "merge_backlink_metrics") {
      window.testBacklinkRequest = args.request;
      for (const record of records) if (record.classification === "internal") { record.backlinkCount = 12; record.referringDomainCount = 4; record.backlinkAuthority = 25.5; }
      return { requestedUrls: 3, matchedRows: 3, failedUrls: 0, firstError: null };
    }
    const aiStatus = () => ({ ...(window.testAiSettings ?? { provider: "anthropic", model: "claude-opus-5", baseUrl: "", requestsPerMinute: 20, maxInputChars: 12000 }), keySaved: Boolean(window.testAiKey), keyringAvailable: true, message: null });
    if (cmd === "get_ai_status") return aiStatus();
    if (cmd === "save_ai_settings") { window.testAiSettings = args.settings; return aiStatus(); }
    if (cmd === "save_ai_api_key") { window.testAiKey = args.request.apiKey; return aiStatus(); }
    if (cmd === "clear_ai_api_key") { window.testAiKey = undefined; return aiStatus(); }
    if (cmd === "run_ai_task") {
      window.testAiRequest = args.request;
      if (!window.testAiKey) throw new Error("Save an API key for the AI provider in Settings > AI");
      const row = records.find((record) => record.id === args.request.recordId);
      row.aiInsights = { ...(row.aiInsights ?? {}), model: "claude-opus-5", updatedAtMs: 1788940800000,
        ...(args.request.task === "intent" ? { intent: { intent: "informational", confidence: 0.9, rationale: "Explains a process." } } : {}),
        ...(args.request.task === "metaDescription" ? { metaDescription: { draft: "A concise explanation of the process.", alternatives: ["Alternative draft"] } } : {}),
        ...(args.request.task === "spelling" ? { spelling: { language: "en", issues: [{ text: "teh", suggestion: "the", kind: "spelling" }] } } : {}) };
      return row.aiInsights;
    }
    if (cmd === "merge_analytics_metrics") {
      window.testAnalyticsProperty = args.request.propertyId;
      window.testAnalyticsRequest = args.request;
      return { propertyId: args.request.propertyId, fetchedRows: 42, matchedRows: 2, sessions: 1234 };
    }
    if (cmd === "get_page_speed_credential_status") return { keySaved: Boolean(window.testPageSpeedKey) && !window.testPageSpeedInvalidKey, keyringAvailable: !window.testPageSpeedKeyringFailure, message: window.testPageSpeedKeyringFailure ? "Could not access the OS credential store" : window.testPageSpeedInvalidKey ? "The saved PageSpeed API key is invalid; replace or clear it." : null };
    if (cmd === "get_http_auth_status") return { saved: Boolean(window.testHttpAuth), username: window.testHttpAuth?.username ?? null, keyringAvailable: true, message: null };
    if (cmd === "save_http_auth_credentials" || cmd === "clear_http_auth_credentials") {
      window.testHttpAuth = cmd === "save_http_auth_credentials" ? args.request : undefined;
      return { saved: Boolean(window.testHttpAuth), username: window.testHttpAuth?.username ?? null, keyringAvailable: true, message: null };
    }
    if (cmd === "get_form_login_status") return { saved: Boolean(window.testFormLogin), username: window.testFormLogin?.username ?? null, keyringAvailable: true, message: null };
    if (cmd === "save_form_login_credentials" || cmd === "clear_form_login_credentials") {
      window.testFormLogin = cmd === "save_form_login_credentials" ? args.request : undefined;
      return { saved: Boolean(window.testFormLogin), username: window.testFormLogin?.username ?? null, keyringAvailable: true, message: null };
    }
    if (cmd === "save_page_speed_api_key" || cmd === "clear_page_speed_api_key") {
      if (window.testHoldPageSpeedCredentials) await new Promise((resolve) => { window.testFinishPageSpeedCredentials = resolve; });
      if (window.testPageSpeedKeyringFailure) throw new Error("Could not access the OS credential store");
      window.testPageSpeedKey = cmd === "save_page_speed_api_key" ? args.request.apiKey : undefined;
      window.testPageSpeedInvalidKey = false;
      return { keySaved: Boolean(window.testPageSpeedKey), keyringAvailable: true, message: null };
    }
    if (cmd === "run_page_speed") {
      window.testPageSpeedRequest = args.request;
      await new Promise((resolve, reject) => {
        window.testFinishPageSpeed = resolve;
        window.testCancelPageSpeed = () => reject(new Error("PageSpeed measurement cancelled"));
        if (!window.testHoldPageSpeed) setTimeout(resolve, 15);
      });
      window.testCancelPageSpeed = undefined;
      if (window.testPageSpeedFailure) throw new Error("PageSpeed Insights returned HTTP 429");
      const row = records.find((record) => record.id === args.request.recordId);
      row.pageSpeed = { strategy: args.request.strategy, requestedUrl: row.finalUrl, completedAtMs: 1788940800000,
        finalUrl: "https://example.test/measured-final", fetchedAt: "2026-09-09T08:00:00.000Z", lighthouseVersion: "13.0.0",
        performanceScore: 0.92, accessibilityScore: null, bestPracticesScore: 0, seoScore: 1, lcpMs: 1234, cls: 0, tbtMs: 0 };
      return row.pageSpeed;
    }
    if (cmd === "run_page_speed_bulk") {
      window.testPageSpeedBulkRequest = args.request;
      let measured = 0, skipped = 0;
      for (const id of args.request.recordIds) {
        const row = records.find((record) => record.id === id);
        if (args.request.resume && row.pageSpeed?.strategy === args.request.strategy) { skipped++; continue; }
        row.pageSpeed = { strategy: args.request.strategy, requestedUrl: row.finalUrl, completedAtMs: 1788940800000, performanceScore: 0.8, accessibilityScore: 0.9, bestPracticesScore: 1, seoScore: 1, lcpMs: 1500, cls: 0.01, tbtMs: 10 };
        measured++;
      }
      return { measured, skipped, failed: [], cancelled: false };
    }
    if (cmd === "run_field_vitals") {
      window.testFieldVitalsRequest = args.request;
      if (window.testFieldVitalsFailure) throw new Error("Chrome UX Report returned HTTP 403");
      const row = records.find((record) => record.id === args.request.recordId);
      row.fieldVitals = { formFactor: args.request.formFactor, requestedUrl: row.finalUrl, completedAtMs: 1788940800000, hasData: !window.testFieldVitalsEmpty,
        lcpMsP75: 2100, clsP75: 0.05, inpMsP75: 180, fcpMsP75: 1400, ttfbMsP75: 600, collectionPeriodStart: "2026-08-15", collectionPeriodEnd: "2026-09-11" };
      return row.fieldVitals;
    }
    if (cmd === "cancel_page_speed") {
      if (window.testPageSpeedRequest?.requestId !== args.requestId || !window.testCancelPageSpeed) return false;
      window.testCancelledPageSpeed = args.requestId;
      window.testCancelPageSpeed();
      window.testCancelPageSpeed = undefined;
      return true;
    }
    if (cmd === "validate_crawl_configuration") {
      await new Promise((resolve) => setTimeout(resolve, window.testConfigurationValidationDelay ?? 15));
      if (window.testHoldConfigurationValidation) await new Promise((resolve) => { window.testFinishConfigurationValidation = resolve; });
      if (window.testConfigurationValidationFailure) throw new Error("invalid include URL regex pattern: [");
      if (window.testHeaderValidationFailure) throw new Error("Request header Authorization is reserved or may contain credentials");
      if (args.config.sitemap?.urls.some((url) => !/^https?:\/\//.test(url))) throw new Error("Sitemap URLs must use HTTP or HTTPS");
      return;
    }
    if (cmd === "measure_serp_snippet") {
      await new Promise((resolve) => setTimeout(resolve, window.testSnippetDelays?.[args.snippet.title] ?? 15));
      if (!/^https?:\/\//.test(args.snippet.url)) throw new Error("Enter an absolute HTTP or HTTPS URL.");
      const titleLength = [...args.snippet.title].length;
      const descriptionLength = [...args.snippet.description].length;
      return { titleLength, titlePixelWidth: titleLength * 10, descriptionLength, descriptionPixelWidth: descriptionLength * 5 };
    }
    if (cmd === "import_serp_snippets") {
      window.testSnippetImportText = args.text;
      if (window.testSnippetImportFailure) throw new Error("CSV requires url, title, and description columns.");
      return [{ url: "https://imported.test/one", title: 'Imported, "title"', description: "İstanbul 🦀" },
        { url: "https://imported.test/two", title: "Second", description: "Second description" }];
    }
    if (cmd === "export_serp_snippets") {
      window.testExportedSnippets = args.snippets;
      return { path: "/tmp/ferrous-frog-snippets.csv", rowCount: args.snippets.length };
    }
    if (cmd === "set_audit_thresholds") {
      window.testAuditThresholds = args.thresholds;
      return null;
    }
    throw new Error(`Unexpected IPC command in smoke test: ${cmd}`);
  }, { shouldMockEvents: true });
  // The installed mock expects `id`; the event API sends `eventId` when removing a listener.
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = (cmd, args, options) => invoke(cmd,
    cmd === "plugin:event|unlisten" ? { ...args, id: args.eventId } : args, options);
}

const profile = await mkdtemp(join(tmpdir(), "ferrous-frog-ui-"));
let browser;
let socket;
let productionServer;
const errors = [];
const server = await createServer({
  cacheDir: join(profile, "vite-cache"),
  server: { host: "127.0.0.1", port: 0, strictPort: false },
  plugins: [{ name: "smoke-fixture", configureServer(vite) {
    vite.middlewares.use("/__smoke__", async (_req, res) => {
      res.setHeader("Content-Type", "text/html");
      const html = await readFile("index.html", "utf8");
      res.end(await vite.transformIndexHtml("/__smoke__", html.replace('<script type="module" src="/src/main.tsx"></script>',
        `<script type="module">import { mockIPC } from '/node_modules/@tauri-apps/api/mocks.js';
        import { emit } from '/node_modules/@tauri-apps/api/event.js';
        (${setupFixture.toString()})(mockIPC, emit); await import('/src/main.tsx');</script>`)));
    });
  } }],
});

try {
  await server.listen();
  const chrome = process.env.CHROME_BIN ?? "google-chrome";
  browser = spawn(chrome, ["--headless", "--no-sandbox", "--disable-gpu",
    "--no-first-run", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let browserLog = "";
  let launchError;
  browser.stderr.on("data", (chunk) => { browserLog = (browserLog + chunk.toString()).slice(-8192); });
  browser.on("error", (error) => { launchError = error.message; });
  let target;
  let lastProbeError = "The debugging endpoint was not created.";
  const startupDeadline = Date.now() + 30000;
  while (Date.now() < startupDeadline) {
    if (launchError || browser.exitCode !== null || browser.signalCode !== null) {
      throw new Error(`Chrome could not start (${chrome}): ${launchError ?? `exit ${browser.exitCode}, signal ${browser.signalCode}`}\n${browserLog}`);
    }
    try {
      const port = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0];
      if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) throw new Error("Chrome has not written a valid debugging port yet.");
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1000) });
      if (!response.ok) throw new Error(`Chrome's debugging endpoint returned HTTP ${response.status}.`);
      const targets = await response.json();
      target = targets.find((candidate) => candidate.type === "page" && candidate.webSocketDebuggerUrl);
      if (target) break;
      lastProbeError = "Chrome has not created its first page yet.";
    } catch (error) { lastProbeError = error.message; }
    await delay(100);
  }
  assert.ok(target, `Chrome did not become ready within 30 seconds (${chrome}). ${lastProbeError}\n${browserLog}`);
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.method === "Runtime.exceptionThrown") errors.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
    if (message.method === "Runtime.consoleAPICalled" && message.params.args.some((arg) => String(arg.value).includes("Encountered two children with the same key"))) {
      errors.push(message.params.args.map((arg) => arg.value).join(" "));
    }
    const request = pending.get(message.id);
    if (request) {
      pending.delete(message.id);
      clearTimeout(request.timer);
      message.error ? request.reject(new Error(message.error.message)) : request.resolve(message.result);
    }
  };
  const cdp = (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => { pending.delete(requestId); reject(new Error(`Timed out: ${method}`)); }, 10000);
    pending.set(requestId, { resolve, reject, timer });
    socket.send(JSON.stringify({ id: requestId, method, params }));
  });
  const pressKey = async (key) => {
    const windowsVirtualKeyCode = { Enter: 13, Escape: 27, ArrowDown: 40, ArrowRight: 39 }[key];
    assert.ok(windowsVirtualKeyCode, `Unsupported smoke-test key: ${key}`);
    const event = { key, code: key, windowsVirtualKeyCode };
    await cdp("Input.dispatchKeyEvent", { ...event, type: "keyDown", ...(key === "Enter" ? { text: "\r" } : {}) });
    await cdp("Input.dispatchKeyEvent", { ...event, type: "keyUp" });
  };
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    assert.ok(!result.exceptionDetails, result.exceptionDetails?.exception?.description);
    return result.result.value;
  };
  const until = async (expression, message) => {
    for (let i = 0; i < 100; i++) {
      try {
        if (await evaluate(`Boolean(${expression})`)) return;
      } catch (error) {
        // Page.reload can replace the execution context between polling requests.
        if (error.message !== "Inspected target navigated or closed") throw error;
      }
      await delay(50);
    }
    assert.fail(message);
  };
  const captureScreenshot = async () => {
    await until("document.getAnimations().every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity)", "Finite UI transitions must settle before taking a screenshot");
    return cdp("Page.captureScreenshot");
  };
  const click = (selector) => evaluate(`document.querySelector(${JSON.stringify(selector)})?.click()`);
  const openTools = async () => {
    await evaluate("document.querySelector('[title=\"More tools\"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
    await until("document.querySelector('[role=\"menuitem\"]')", "Tools menu must open");
  };
  const menuItem = async (label) => {
    await openTools();
    await evaluate(`[...document.querySelectorAll('[role="menuitem"], [role="menuitemradio"]')].find((item) => item.textContent.includes(${JSON.stringify(label)})).click()`);
  };
  const openGraph = async () => {
    await menuItem("Crawl Graph");
    await until("document.querySelector('.crawl-graph-modal[data-state=\"open\"]')", "The graph tool must load when opened");
  };
  const useSvgGraph = async () => {
    await evaluate("[...document.querySelectorAll('.graph-actions button')].find((button) => button.textContent === 'Use SVG')?.click()");
    await until("document.querySelector('.graph-svg')", "The compatible SVG canvas must remain available");
  };
  const fill = (selector, value) => evaluate(`(() => { const input = document.querySelector(${JSON.stringify(selector)});
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value').set.call(input, ${JSON.stringify(value)});
    input.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  // Capture short exits in the browser before clicking; CDP round trips can outlast the animation.
  const observeDialogExit = (selector, capture = "") => evaluate(`(() => {
    window.testDialogExit = undefined;
    const dialog = document.querySelector(${JSON.stringify(selector)});
    dialog.addEventListener('animationstart', function observeExit(event) {
      if (event.target !== dialog || event.animationName !== 'dialog-exit') return;
      dialog.removeEventListener('animationstart', observeExit);
      window.testDialogExit = { state: dialog.dataset.state, inert: dialog.inert, text: dialog.textContent,
        animation: getComputedStyle(dialog).animationName, pointerEvents: getComputedStyle(dialog).pointerEvents };
      ${capture}
    });
  })()`);
  const settingsTab = async (label) => {
    await evaluate(`(() => {
      const button = [...document.querySelectorAll('.settings-tabs button')].find((button) => button.textContent.trim() === ${JSON.stringify(label)});
      const group = button.closest('details');
      if (!group.open) group.querySelector('summary').click();
    })()`);
    await evaluate(`[...document.querySelectorAll('.settings-tabs button')].find((button) => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
  };
  const selectProfile = (id) => evaluate(`(() => { const select = document.querySelector('.settings-section:not([hidden]) select'); select.value = ${JSON.stringify(id)}; select.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  const setting = (label) => `.settings-section:not([hidden]) label[data-test-setting=${JSON.stringify(label)}] input`;
  const markSetting = async (label) => {
    const section = await evaluate(`(() => {
      const item = [...document.querySelectorAll('.settings-section label')].find((item) => item.firstChild.textContent.trim() === ${JSON.stringify(label)});
      item.setAttribute('data-test-setting', ${JSON.stringify(label)});
      return item.closest('[data-settings-section]').dataset.settingsSection;
    })()`);
    await select("Settings section", section);
  };
  const toggleSetting = (label) => evaluate(`[...document.querySelectorAll('.settings-section:not([hidden]) .checkbox-field')].find((item) => item.textContent.trim() === ${JSON.stringify(label)}).querySelector('[role="checkbox"]').click()`);
  const savedSettings = () => evaluate("JSON.parse(localStorage.getItem('ferrous-frog-settings'))");
  const applySettings = async () => {
    await click('[data-action="apply-settings"]');
    await until("document.querySelector('.settings-footer [role=\"status\"]')?.textContent === 'No pending changes'", "Apply must validate and save the pending settings");
  };
  const select = (label, value) => evaluate(`(() => { const select = document.querySelector(${JSON.stringify(`[aria-label="${label}"]`)}); select.value = ${JSON.stringify(value)}; select.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  const openMode = async () => {
    await evaluate("document.querySelector('[aria-label=\"Crawl mode\"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
    await until("document.querySelector('[role=\"menuitemradio\"][data-mode=\"spider\"]')", "The Mode menu must expose crawl modes");
  };
  const chooseMode = async (mode) => { await openMode(); await click(`[role="menuitemradio"][data-mode="${mode}"]`); };
  const importSnippets = async (text) => {
    const path = join(profile, "snippets.csv");
    await writeFile(path, text);
    const { root } = await cdp("DOM.getDocument");
    const { nodeId } = await cdp("DOM.querySelector", { nodeId: root.nodeId, selector: '[aria-label="Import snippet CSV"]' });
    await cdp("DOM.setFileInputFiles", { nodeId, files: [path] });
  };
  const search = (value) => fill('[aria-label="Search results"]', value);
  const contrast = (foreground, background = foreground) => evaluate(`(() => {
    const context = document.createElement('canvas').getContext('2d');
    const luminance = (color) => {
      context.fillStyle = color; context.fillRect(0, 0, 1, 1);
      const rgb = [...context.getImageData(0, 0, 1, 1).data].slice(0, 3).map((n) => n / 255)
        .map((n) => n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4);
      return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    };
    const colors = [luminance(getComputedStyle(document.querySelector(${JSON.stringify(foreground)})).color),
      luminance(getComputedStyle(document.querySelector(${JSON.stringify(background)})).backgroundColor)].sort((a, b) => b - a);
    return (colors[0] + 0.05) / (colors[1] + 0.05);
  })()`);

  await cdp("Runtime.enable");
  await cdp("Page.enable");
  const systemTheme = (value) => cdp("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value }] });
  const showHistory = async (open) => {
    if (await evaluate(`document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded') !== ${JSON.stringify(String(open))}`)) await click('.crawl-history-toggle');
    await until(`document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded') === ${JSON.stringify(String(open))} && document.querySelector('.crawl-history-body').getAnimations().every((animation) => animation.playState !== 'running')`, "The history panel must finish opening or closing");
  };
  const openFixtureCrawl = async (restorePreferences = true) => {
    await until("document.querySelector('[data-session-id=\"fixture-current\"] [data-action=\"open-saved-crawl\"]')", "The saved crawl must appear in the library");
    await showHistory(true);
    if (restorePreferences) await evaluate("try { window.testSessionConfig = JSON.parse(localStorage.getItem('ferrous-frog-settings'))?.config; } catch { window.testSessionConfig = undefined; }");
    await click('[data-session-id="fixture-current"] [data-action="open-saved-crawl"]');
    await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "Opening a saved crawl must render its results");
  };
  const reloadApp = async (openCrawl = true) => {
    await evaluate("window.testBeforeReload = true");
    await cdp("Page.reload");
    await until("!window.testBeforeReload && window.testStartupTheme && document.querySelector('.crawl-home')", "The app should reload into its crawl library");
    if (openCrawl) await openFixtureCrawl();
  };
  await systemTheme("dark");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__` });
  await until("document.querySelectorAll('.crawl-card').length === 3", "Startup must load saved crawl metadata");
  await until("testStartupCalls === 1 && testStartupHadHome", "Splash completion must follow the crawl library and run only once");
  assert.equal(await evaluate("testQueries.length"), 0, "The home screen must load metadata without querying crawl records");
  assert.equal(await evaluate("document.querySelectorAll('.data-table').length"), 0, "Results must appear only after a crawl is opened or started");
  assert.ok(await evaluate("document.querySelector('[data-action=\"start-new-crawl\"]').disabled"), "A fresh launcher must wait for the user's URL");
  assert.equal(await evaluate("document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded')"), "false", "Saved crawls must be collapsed on a fresh launch");
  assert.ok(await evaluate("document.querySelector('.crawl-history-body').inert && document.querySelector('.crawl-history-body').getBoundingClientRect().height === 0"), "Collapsed history must be hidden and excluded from keyboard navigation");
  await until("document.querySelector('.crawl-home-content').getAnimations().every((animation) => animation.playState !== 'running')", "The launcher entry must settle before checking its placement");
  assert.ok(await evaluate(`(() => {
    const content = document.querySelector('.crawl-home-content').getBoundingClientRect();
    const launcher = document.querySelector('.crawl-launcher').getBoundingClientRect();
    return Math.abs((launcher.top + launcher.bottom - content.top - content.bottom) / 2) < 32;
  })()`), "The closed history panel must leave the launcher vertically centered");
  await evaluate("document.querySelector('.crawl-history-toggle').focus()");
  await pressKey("Enter");
  await until("document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded') === 'true' && !document.querySelector('.crawl-history-body').inert", "Enter must open saved crawls");
  await reloadApp(false);
  assert.equal(await evaluate("document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded')"), "true", "An explicitly opened history panel must remain open after restart");
  await evaluate("document.querySelector('[aria-label=\"Search saved crawls\"]').focus()");
  await pressKey("Escape");
  await until("document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded') === 'false' && document.activeElement.matches('.crawl-history-toggle')", "Escape must close history and return focus to its toggle");
  await reloadApp(false);
  assert.equal(await evaluate("document.querySelector('.crawl-history-toggle').getAttribute('aria-expanded')"), "false", "Closing history must also survive restart");
  await evaluate("testSettingsWriteFailure = true");
  await showHistory(true);
  assert.ok(await evaluate("document.querySelector('.crawl-history-preference-error')?.textContent.includes('Could not save')"), "A failed preference write must remain visible without preventing the panel opening");
  await evaluate("testSettingsWriteFailure = false");
  await showHistory(false);
  await cdp("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }, { name: "prefers-reduced-motion", value: "reduce" }] });
  await showHistory(true);
  assert.ok(await evaluate("getComputedStyle(document.querySelector('.crawl-history-body')).transitionDuration === '0s' && getComputedStyle(document.querySelector('.crawl-home-content')).animationName === 'none'"), "Reduced motion must remove drawer and screen animations");
  await until("document.querySelectorAll('.crawl-card').length === 3", "Reloaded history must finish loading before responsive checks");
  for (const theme of ["dark", "light"]) {
    await systemTheme(theme);
    for (const width of [1440, 390]) {
      await cdp("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false });
      await showHistory(false);
      assert.ok(await evaluate("document.querySelector('.crawl-home').scrollWidth <= document.querySelector('.crawl-home').clientWidth && document.documentElement.scrollWidth <= innerWidth"), "The library must fit narrow windows");
      assert.ok(await contrast('.crawl-launcher input', '.crawl-launcher-fields') >= 4.5, "The launcher must be readable in both themes");
      assert.ok(await evaluate("Math.abs(document.querySelector('.crawl-history').getBoundingClientRect().bottom - document.querySelector('.crawl-home').getBoundingClientRect().bottom) < 1"), "History must stay attached to the bottom of the home screen");
      if (process.env.UI_SCREENSHOT) {
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.home-collapsed-${theme}-${width}.png`, Buffer.from(shot.data, "base64"));
      }
      await showHistory(true);
      assert.ok(await evaluate("document.querySelector('.crawl-history-scroll').scrollWidth <= document.querySelector('.crawl-history-scroll').clientWidth && document.querySelector('.crawl-history-body').getBoundingClientRect().height <= innerHeight * 0.49"), "Expanded history must remain bounded and fit narrow windows");
      assert.ok(await evaluate("[...document.querySelectorAll('.crawl-card')].every((card) => { const button = card.querySelector('.crawl-card-delete').getBoundingClientRect(); const box = card.getBoundingClientRect(); return button.top - box.top <= 16 && box.right - button.right <= 16 && Boolean(card.querySelector('.crawl-card-heading .crawl-card-delete')); })"), "Delete belongs in each card's upper-right corner at every width");
      if (process.env.UI_SCREENSHOT) {
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.home-${theme}-${width}.png`, Buffer.from(shot.data, "base64"));
      }
    }
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await systemTheme("dark");
  await fill('[aria-label="Search saved crawls"]', "another.test");
  assert.equal(await evaluate("document.querySelectorAll('.crawl-card').length"), 1, "History search must filter saved sites");
  await fill('[aria-label="Search saved crawls"]', "no-match");
  assert.ok(await evaluate("document.querySelector('.crawl-history-empty')?.textContent.includes('No matching')"), "History search must explain empty results");
  await fill('[aria-label="Search saved crawls"]', "");
  await click('[data-session-id="fixture-current"] input[type="checkbox"]');
  await click('[data-session-id="fixture-baseline"] input[type="checkbox"]');
  assert.ok(await evaluate("document.querySelector('[data-session-id=\"fixture-other\"] input[type=\"checkbox\"]').disabled"), "Comparison selection must stop at two crawls");
  await evaluate("document.querySelector('[data-action=\"compare-saved-crawls\"]').focus()");
  await click('[data-action="compare-saved-crawls"]');
  await until("document.querySelector('.comparison-table')?.textContent.includes('New title')", "Selecting two saved crawls must open their comparison");
  assert.deepEqual(await evaluate("testComparisonIds"), ["fixture-baseline", "fixture-current"], "The older crawl must be the comparison baseline");
  assert.ok(await evaluate("document.querySelector('.comparison-sessions')?.textContent.includes('Example baseline')"), "Comparison must identify its saved crawls");
  assert.equal(await evaluate("window.testOpenedSession"), undefined, "Comparison must not replace the active crawl");
  assert.equal(await evaluate("testComparisonRequests.at(-1).includeResponseOnly"), false, "Saved comparisons must exclude response-only rows in the backend by default");
  assert.ok(await evaluate("!document.querySelector('.comparison-table').textContent.includes('response-only') && document.querySelector('.comparison-options [role=\"checkbox\"]').getAttribute('aria-checked') === 'false'"), "Raw response changes must not appear as ordinary content changes");
  assert.deepEqual(await evaluate("Object.fromEntries([...document.querySelectorAll('.comparison-summary .metric')].map((metric) => [metric.querySelector('span').textContent, metric.querySelector('strong').textContent]))"),
    { Baseline: '1,200', Current: '1,205', Added: '8', Removed: '3', Changed: '12', Status: '2', Titles: '8', Descriptions: '3', Indexability: '1', 'Content changes': '4', 'Response-only': '500', 'Content unavailable': '33' }, "Comparison counters must distinguish text changes, raw-only changes and unavailable legacy content");
  assert.ok(await evaluate("['HTTP status, Title, Meta description, Content', 'Headings, Canonical, Robots directives', 'Old description to New description', 'Unavailable', 'Response hash'].every((text) => document.querySelector('.comparison-table').textContent.includes(text))"), "Comparison rows must show readable reasons, descriptions and content availability");
  const comparisonTheme = await evaluate("document.documentElement.classList.contains('dark')");
  for (const [width, height] of [[1280, 840], [390, 640]]) {
    await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    assert.ok(await evaluate("document.querySelector('.comparison-body').scrollWidth <= document.querySelector('.comparison-body').clientWidth && document.querySelector('.comparison-controls button').getBoundingClientRect().right <= document.querySelector('.comparison-modal').getBoundingClientRect().right"), "Comparison filters and controls must fit small windows while the wide table scrolls");
    for (const theme of ["dark", "light"]) {
      await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
      assert.ok(await contrast('.comparison-options', '.comparison-modal') >= 4.5, `Comparison content guidance must stay readable in ${theme} mode`);
    }
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await evaluate(`document.documentElement.classList.toggle('dark', ${comparisonTheme})`);
  await click('.comparison-options [role="checkbox"]');
  await until("testComparisonRequests.at(-1).includeResponseOnly && document.querySelector('.comparison-table')?.textContent.includes('Response only')", "Response-only opt-in must query the saved-session backend again");
  await evaluate("testHoldComparison = true; testPendingComparisons = []");
  await click('.comparison-options [role="checkbox"]');
  await until("testPendingComparisons.length === 1", "The older comparison filter request must be in flight");
  await click('.comparison-options [role="checkbox"]');
  await until("testPendingComparisons.length === 2", "The comparison filter must remain usable while a query is pending");
  await evaluate("testPendingComparisons[1]()");
  await until("document.querySelector('.comparison-table')?.textContent.includes('Response only')", "The latest filter response must appear first");
  await evaluate("testPendingComparisons[0](); testHoldComparison = false; testPendingComparisons = []");
  await delay(150);
  assert.ok(await evaluate("document.querySelector('.comparison-table').textContent.includes('Response only') && document.querySelector('.comparison-options [role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), "An older filtered result must not overwrite the latest comparison");
  await click('.comparison-table [data-comparison-key="1"]');
  await until("document.querySelector('.comparison-detail-table')?.textContent.includes('previous-full-response-hash')", "Selected changes must load full record values without truncated hashes");
  await click('.comparison-detail-title button');
  assert.ok(await evaluate("document.querySelector('.comparison-detail').getBoundingClientRect().height > 700 && getComputedStyle(document.querySelector('.comparison-results')).display === 'none'"), "Expand details must dedicate the work area to inspecting a URL");
  await fill('[aria-label="Find comparison detail field"]', 'Structured data issues');
  assert.ok(await evaluate("document.querySelector('.comparison-detail-table').textContent.includes('Missing offer price') && document.querySelector('.comparison-detail-table').textContent.includes('[]')"), "Previous and current captured audit evidence must be inspectable side by side");
  await fill('[aria-label="Find comparison detail field"]', '');
  if (process.env.UI_SCREENSHOT) {
    for (const [width, height] of [[1440, 900], [390, 640]]) {
      await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      if (width < 700) {
        assert.ok(await evaluate("document.querySelector('.comparison-detail-table th:last-child').getBoundingClientRect().right <= document.querySelector('.comparison-modal').getBoundingClientRect().right && document.querySelector('.comparison-body').scrollWidth <= document.querySelector('.comparison-body').clientWidth"), "Narrow detail views must show both previous and current columns without horizontal overflow");
      }
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.comparison-${width}.png`, Buffer.from(shot.data, "base64"));
    }
    await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  }
  await click('.comparison-detail-title button');
  await until("getComputedStyle(document.querySelector('.comparison-results')).display !== 'none' && document.querySelector('.comparison-table tr[aria-selected=\"true\"]')?.dataset.comparisonKey === '1'", "Returning to changes must preserve the selected comparison row");
  await evaluate("testHoldComparisonDetail = true; testPendingComparisonDetails = []");
  await click('.comparison-table [data-comparison-key="1"]');
  await until("testPendingComparisonDetails.length === 1", "The first detail request must remain pending");
  await click('.comparison-table [data-comparison-key="2"]');
  await until("testPendingComparisonDetails.length === 2", "Selection must remain usable while details load");
  await evaluate("testPendingComparisonDetails[1]()");
  await until("document.querySelector('.comparison-detail-header h3')?.textContent.endsWith('/legacy')", "The latest selected URL must appear first");
  await evaluate("testPendingComparisonDetails[0](); testHoldComparisonDetail = false");
  await delay(150);
  assert.ok(await evaluate("document.querySelector('.comparison-detail-header h3').textContent.endsWith('/legacy')"), "A late detail must not replace the current selected URL");
  await evaluate("document.querySelector('.comparison-table [data-comparison-key=\"1\"]').focus()");
  await pressKey("ArrowDown");
  await until("document.activeElement.dataset.comparisonKey === '2'", "Arrow keys must move focus and selection through comparison rows");
  await evaluate("testComparisonManyRows = true");
  await click('.comparison-controls button');
  await until("document.querySelector('.comparison-limit')?.textContent.includes('of 1,273 matching changes')", "Prepared comparison must retain changes beyond the old 1,000-row limit");
  assert.ok(await evaluate("document.querySelectorAll('.comparison-table [data-comparison-key]').length < 100"), "Comparison DOM rows must remain virtualized within each server page");
  await fill('[aria-label="Comparison page"]', '13');
  await evaluate("document.querySelector('[aria-label=\"Comparison page\"]').focus()");
  await pressKey("Enter");
  await until("testComparisonRequests.at(-1).offset === 1200 && document.querySelector('.comparison-limit')?.textContent.includes('1,201–1,273')", "Page navigation must fetch rows beyond 1,000 from the backend");
  await fill('[aria-label="Search comparison"]', 'zz-page');
  await select("Comparison change", "changed");
  await select("Comparison changed field", "title");
  await select("Comparison sort", "currentTitle");
  await select("Comparison sort direction", "desc");
  await until("testComparisonRequests.at(-1).sortDir === 'desc' && document.querySelector('.comparison-limit')?.textContent.includes('of 1,260 matching changes')", "Search, reason, change and sort controls must query the complete comparison");
  assert.deepEqual(await evaluate("(({ search, change, changedField, sortBy, sortDir, offset, limit }) => ({ search, change, changedField, sortBy, sortDir, offset, limit }))(testComparisonRequests.at(-1))"),
    { search: 'zz-page', change: 'changed', changedField: 'title', sortBy: 'currentTitle', sortDir: 'desc', offset: 0, limit: 100 }, "Comparison filters must reset pagination and remain server-side");
  await click('.comparison-filters button');
  await until("document.querySelector('.comparison-feedback')?.textContent.includes('Exported 1,260 matching changes')", "Filtered CSV must include every match rather than the visible page");
  assert.equal(await evaluate("testComparisonExport.rows"), 1260);
  await evaluate("testHoldComparisonExport = true");
  await click('.comparison-filters button');
  await until("typeof testFinishComparisonExport === 'function'", "Export must be pending for the stale-result check");
  await fill('[aria-label="Search comparison"]', 'no-matches');
  await until("document.querySelector('.comparison-limit')?.textContent.includes('No matching changes')", "An empty server filter must have a clear state");
  await evaluate("testFinishComparisonExport(); testHoldComparisonExport = false");
  await delay(150);
  assert.ok(await evaluate("!document.querySelector('.comparison-feedback')?.textContent.includes('Exported')"), "A completed export from an older filter must not show a stale notice");
  await evaluate("testComparisonIdentities = true");
  await click('.comparison-controls button');
  await until("document.querySelector('.comparison-limit')?.textContent.includes('of 4 matching changes')", "Repeated List occurrences and redirect aliases must remain separate comparison rows");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.comparison-table [data-comparison-key]')].map((row) => row.dataset.comparisonIdentity).sort()"), ['1:https://example.test/alias-a', '1:https://example.test/alias-b', '1:https://example.test/repeated', '2:https://example.test/repeated']);
  assert.ok(await evaluate("document.querySelector('.comparison-table [data-comparison-key=\"41\"]').textContent.includes('2 to 7') && document.querySelector('.comparison-table').textContent.includes('https://example.test/new-target')"), "Rows must expose occurrence positions and changed final destinations");
  await click('.comparison-table [data-comparison-key="41"]');
  await until("document.querySelector('.comparison-detail-header')?.textContent.includes('Occurrence 1') && document.querySelector('.comparison-detail-table')?.textContent.includes('HTTPS://EXAMPLE.TEST:443/repeated#old')", "Identity details must retain captured literal URLs and occurrence evidence");
  await click('.comparison-table [data-comparison-key="42"]');
  await until("document.querySelector('.comparison-detail-header')?.textContent.includes('Occurrence 2') && document.querySelector('.comparison-detail-table')?.textContent.includes('Second updated')", "Selecting a repeated URL must inspect its own occurrence");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.comparison-identity.png`, Buffer.from(shot.data, "base64"));
  }
  await select('Comparison changed field', 'finalUrl');
  await until("testComparisonRequests.at(-1).changedField === 'finalUrl' && document.querySelector('.comparison-limit')?.textContent.includes('of 1 matching changes')", "Final-destination changes must be a backend filter");
  await click('.comparison-filters button');
  await until("testComparisonExport.query.changedField === 'finalUrl' && testComparisonExport.rows === 1", "Identity comparisons must preserve destination filters for CSV");
  await evaluate("testComparisonIdentities = false");
  await evaluate("testHoldComparison = true; testPendingComparisons = []; testLateComparisonPublication = true");
  await click('.comparison-controls button');
  await until("testPendingComparisons.length === 1", "Preparation must be pending before closing its workspace");
  const closingComparison = await evaluate("testComparisonRequests.at(-1).comparisonId");
  await click('[title="Close crawl comparison"]');
  await until("!document.querySelector('.comparison-modal')", "Closing must remove the full workspace");
  assert.ok(await evaluate(`testClosedComparisons.includes(${JSON.stringify(closingComparison)})`), "Closing must retire even a pending native comparison");
  const nativePreparationsBeforeReopen = await evaluate("testComparisonRequests.filter((request) => request.command === 'open_crawl_comparison').length");
  await click('[data-action="compare-saved-crawls"]');
  await until("document.querySelector('.comparison-modal')?.textContent.includes('Preparing comparison')", "A rapid reopen must wait while its older preparation finishes");
  await delay(100);
  assert.equal(await evaluate("testComparisonRequests.filter((request) => request.command === 'open_crawl_comparison').length"), nativePreparationsBeforeReopen, "A new native preparation must wait until the older one has closed");
  await click('[title="Close crawl comparison"]');
  await until("!document.querySelector('.comparison-modal')", "A queued preparation must remain closable");
  await click('[data-action="compare-saved-crawls"]');
  await until("document.querySelector('.comparison-modal')?.textContent.includes('Preparing comparison')", "A later reopen must replace the superseded queued request");
  await evaluate("testHoldComparison = false; testComparisonManyRows = false; testPendingComparisons[0]()");
  await until(`!testComparisonSnapshots[${JSON.stringify(closingComparison)}] && testClosedComparisons.filter((id) => id === ${JSON.stringify(closingComparison)}).length === 2`, "A late native preparation must be closed again after publication to remove its temporary workspace");
  await until("document.querySelector('.comparison-table')?.textContent.includes('New title')", "The newest reopened comparison must become ready after stale cleanup");
  assert.equal(await evaluate("testComparisonRequests.filter((request) => request.command === 'open_crawl_comparison').length"), nativePreparationsBeforeReopen + 1, "Superseded queued preparations must never invoke the backend");
  await evaluate("testLateComparisonPublication = false");
  await click('[title="Close crawl comparison"]');
  await until("!document.querySelector('.comparison-modal') && document.activeElement.dataset.action === 'compare-saved-crawls'", "Closing a reopened workspace must return focus to the library opener");
  assert.equal(await evaluate("Object.keys(testComparisonSnapshots).length"), 0, "Closed and superseded comparisons must release every prepared snapshot");
  assert.equal(await evaluate("window.testOpenedSession"), undefined, "Paging, detail and export must preserve the active crawl");
  await evaluate("window.testHistoryFixtures = testSessions; window.testSessions = []");
  await click('[aria-label="Refresh saved crawls"]');
  await until("document.querySelector('.crawl-history-empty')?.textContent.includes('No saved crawls')", "An empty library must offer a clear first-crawl state");
  await evaluate("window.testHistoryFailure = true");
  await click('[aria-label="Refresh saved crawls"]');
  await until("document.querySelector('.crawl-history-error')", "History errors must offer retry without blocking the launcher");
  await evaluate("window.testHistoryFailure = false; window.testSessions = testHistoryFixtures");
  await click('.crawl-history-error button');
  await until("document.querySelectorAll('.crawl-card').length === 3 && !document.querySelector('.crawl-history-error')", "Retry must recover saved crawl history");
  await evaluate("testHistoryFailure = true");
  await click('[aria-label="Refresh saved crawls"]');
  await until("document.querySelector('.crawl-history-error')", "Refresh failures must retain the existing history cards");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 390, height: 400, deviceScaleFactor: 1, mobile: false });
  await evaluate("document.querySelector('.crawl-history-scroll').scrollTop = document.querySelector('.crawl-history-scroll').scrollHeight");
  assert.ok(await evaluate(`(() => {
    const home = document.querySelector('.crawl-home').getBoundingClientRect();
    const history = document.querySelector('.crawl-history').getBoundingClientRect();
    const scroll = document.querySelector('.crawl-history-scroll').getBoundingClientRect();
    const last = document.querySelector('.crawl-history-pagination').getBoundingClientRect();
    return history.bottom <= home.bottom + 1 && scroll.bottom <= home.bottom + 1 && last.bottom <= scroll.bottom && document.querySelector('.crawl-home-content').clientHeight >= 140;
  })()`), "A history error in a short window must preserve the launcher and allow scrolling to the panel's last controls");
  await evaluate("testHistoryFailure = false");
  await click('.crawl-history-error button');
  await until("!document.querySelector('.crawl-history-error')", "History retry must remain usable in a short window");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await fill('[aria-label="Crawl URL"]', "ftp://example.test/");
  await click('[data-action="start-new-crawl"]');
  assert.equal(await evaluate("testStartCalls"), 0, "The launcher must reject non-HTTP URLs before requesting a crawl");
  await fill('[aria-label="Crawl URL"]', "https://launcher.test/");
  await evaluate("window.testStartFailure = true");
  await click('[data-action="start-new-crawl"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Could not create')", "A failed new crawl must leave the launcher available with its error");
  assert.equal(await evaluate("document.querySelectorAll('.crawl-card').length"), 3, "Failed starts must retain previous crawls");
  await evaluate("window.testStartFailure = false");
  await evaluate("document.querySelector('.crawl-launcher [aria-label=\"Crawl mode\"]').value = 'list'; document.querySelector('.crawl-launcher [aria-label=\"Crawl mode\"]').dispatchEvent(new Event('change', { bubbles: true }))");
  await fill('.crawl-launcher [aria-label="List URLs"]', " https://list-launcher.test/a \n\nhttps://list-launcher.test/a\n");
  await click('[data-action="start-new-crawl"]');
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)') && !document.querySelector('.crawl-home')", "Starting must open the workbench");
  assert.equal(await evaluate("testStartedResume"), false, "The launcher must always start a separate crawl");
  assert.deepEqual(await evaluate("testStartedList"), ["https://list-launcher.test/a", "https://list-launcher.test/a"], "New List sessions must normalize whitespace before saving targets without deduplicating them");
  assert.equal((await savedSettings()).storageMode, "database", "Desktop crawl preferences must use SQLite");
  await click('[aria-label="Crawl library"]');
  await until("document.querySelectorAll('.crawl-card').length === 4", "A new crawl must join history without removing older cards");
  assert.ok(await evaluate("document.querySelector('.crawl-launcher input').matches(':disabled') && document.querySelector('[data-action=\"open-saved-crawl\"]').disabled"), "An active crawl must lock competing library actions");
  await click('[data-action="return-to-workspace"]');
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "Returning to the running crawl must retain its results");
  await evaluate("testEmit({ kind: 'finished' })");
  await click('[aria-label="Crawl library"]');
  await evaluate("window.testOpenSessionFailure = true");
  await click('[data-session-id="fixture-current"] [data-action="open-saved-crawl"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('database is missing')", "Missing saved files must keep the library open with an error");
  assert.ok(await evaluate("Boolean(document.querySelector('.crawl-home'))"), "Failed opening must not enter an empty workbench");
  await evaluate("window.testOpenSessionFailure = false; window.testSessionConfig = undefined; window.testRecovery = { recoverable: true, queued: 2, seen: 20, crawled: 18 }");
  await evaluate("document.querySelector('.crawl-launcher [aria-label=\"Crawl mode\"]').value = 'list'; document.querySelector('.crawl-launcher [aria-label=\"Crawl mode\"]').dispatchEvent(new Event('change', { bubbles: true }))");
  await fill('.crawl-launcher [aria-label="List URLs"]', "https://unrelated.test/old-target");
  await click('[data-session-id="fixture-other"] [data-action="open-saved-crawl"]');
  await until("!document.querySelector('.crawl-home') && document.querySelector('[aria-label=\"Root URL\"]')?.value === 'https://another.test/docs/'", "A legacy List crawl must restore its mode and target");
  assert.deepEqual((await savedSettings()).config.listUrls, [], "Configless sessions must clear unrelated List targets");
  assert.ok((await savedSettings()).resumeCrawl, "An opened crawl with queued work must offer resume");
  await click('[aria-label="Crawl library"]');
  await click('[data-session-id="fixture-current"] [data-action="open-saved-crawl"]');
  await until("document.querySelector('[aria-label=\"Crawl mode\"]')?.dataset.mode === 'spider' && !document.querySelector('.crawl-home')", "A legacy Spider crawl must replace the previous List mode");
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "The selected saved crawl must finish loading rows");
  await evaluate(`window.testHeldQueries = {
    get_crawl_path: { found: true, steps: [], truncated: false },
    get_url_tree: { nodes: [], totalUrls: 9999, renderedUrls: 0, capped: true },
    get_crawl_graph: { nodes: [], edges: [], totalNodes: 9999, totalEdges: 0 },
    get_recovery_state: { recoverable: true, queued: 9999, seen: 9999, crawled: 9999 },
    get_database_location: { path: '/tmp/stale-crawl.sqlite3' },
    page_references: { references: [{ id: 999, sourceStorageKey: 'https://example.test/page-1', sourceUrl: 'https://stale.test/',
      targetUrl: 'https://stale.test/old-reference', kind: 'canonical', relNofollow: false }], total: 1 }
  }; window.testRecovery = undefined`);
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("window.testPendingQueries?.get_crawl_path", "The old crawl path must be in flight");
  await click('#detail-tab-references');
  await until("window.testPendingQueries?.page_references", "The old crawl references must be in flight");
  await click('[aria-label="Tree view"]');
  await until("window.testPendingQueries?.get_url_tree", "The old tree must be in flight");
  await openGraph();
  await until("window.testPendingQueries?.get_crawl_graph", "The old graph must be in flight");
  await click('[title="Close graph"]');
  await click('[aria-label="Crawl settings"]');
  await until("window.testPendingQueries?.get_recovery_state && window.testPendingQueries?.get_database_location", "Old database queries must be in flight");
  await click('[title="Close settings"]');
  await click('[aria-label="Crawl library"]');
  await click('[data-session-id="fixture-other"] [data-action="open-saved-crawl"]');
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)') && !document.querySelector('.crawl-home')", "Another crawl must open while older queries are pending");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.selected-references tbody')?.textContent.includes('fixture-other')", "The opened crawl must load its own stored references");
  await evaluate("window.testPendingQueries.page_references()");
  await delay(150);
  assert.ok(await evaluate("document.querySelector('.selected-references tbody')?.textContent.includes('fixture-other') && !document.querySelector('.selected-references').textContent.includes('stale.test')"), "Older reference responses must not replace the same URL in another session");
  await click('#detail-tab-page');
  await until("document.querySelector('.crawl-path-detail')?.textContent.includes('No internal path found')", "The opened crawl must load its own path");
  await click('[aria-label="Tree view"]');
  await until("document.querySelector('.url-tree-toolbar')?.textContent.includes('1,205')", "The opened crawl must load its own tree");
  await openGraph();
  await useSvgGraph();
  await until("document.querySelectorAll('.graph-svg circle').length === 4", "The opened crawl must load its own graph");
  await evaluate("for (const cmd of ['get_crawl_path', 'get_url_tree', 'get_crawl_graph']) window.testPendingQueries[cmd]()");
  await delay(150);
  assert.equal(await evaluate("document.querySelectorAll('.graph-svg circle').length"), 4, "An older graph response must not replace the opened crawl");
  assert.ok(await evaluate("document.querySelector('.url-tree-toolbar')?.textContent.includes('1,205')"), "An older tree response must not replace the opened crawl");
  assert.ok(await evaluate("document.querySelector('.crawl-path-detail')?.textContent.includes('No internal path found')"), "An older path response must not replace the selected URL");
  await click('[title="Close graph"]');
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Storage");
  await until("document.querySelector('.recovery-state')?.textContent.includes('No queued crawl state')", "The opened crawl must load its recovery state");
  await evaluate("window.testPendingQueries.get_recovery_state(); window.testPendingQueries.get_database_location()");
  await delay(150);
  assert.ok(await evaluate("document.querySelector('.recovery-state')?.textContent.includes('No queued crawl state')"), "Older recovery queries must not enable resume for the opened crawl");
  await markSetting("Current database path");
  assert.notEqual(await evaluate(`document.querySelector(${JSON.stringify(setting("Current database path"))}).value`), "/tmp/stale-crawl.sqlite3", "Older database queries must not replace the opened path");
  await click('[title="Close settings"]');
  await click('[aria-label="Crawl library"]');
  await until("document.querySelector('[data-session-id=\"fixture-baseline\"] [data-action=\"delete-saved-crawl\"]')", "Saved cards must offer deletion without opening the crawl");
  await evaluate("document.querySelector('[data-session-id=\"fixture-baseline\"] [data-action=\"delete-saved-crawl\"]').focus()");
  await click('[data-session-id="fixture-baseline"] [data-action="delete-saved-crawl"]');
  await until("document.querySelector('.delete-crawl-modal')?.textContent.includes('Example baseline')", "Delete confirmation must identify the chosen crawl");
  assert.equal(await evaluate("window.testDeleteCalls ?? 0"), 0, "Opening confirmation must not delete data");
  assert.ok(await evaluate("document.activeElement.matches('[data-action=\"cancel-delete-crawl\"]')"), "Delete confirmation must focus Cancel");
  await observeDialogExit('.delete-crawl-modal');
  await click('[data-action="cancel-delete-crawl"]');
  await until("window.testDialogExit?.state === 'closed'", "Deletion must keep its content mounted while closing");
  assert.ok(await evaluate("testDialogExit.text.includes('Example baseline') && testDialogExit.animation === 'dialog-exit' && testDialogExit.pointerEvents === 'none'"), "Closing deletion must retain the chosen name and disable pointer input through its exit animation");
  await until("!document.querySelector('.delete-crawl-modal')", "Cancel must close deletion without changing history");
  await until("document.activeElement.matches('[data-action=\"delete-saved-crawl\"]')", "Cancel must return focus to the chosen card");
  await evaluate("document.querySelectorAll('.crawl-card-select input:checked').forEach((input) => input.click())");
  await click('[data-session-id="fixture-baseline"] .crawl-card-select input');
  await click('[data-session-id="fixture-other"] .crawl-card-select input');
  await evaluate("window.testDeleteFailure = true; window.testHoldDeletion = true");
  await click('[data-session-id="fixture-baseline"] [data-action="delete-saved-crawl"]');
  await click('[data-action="confirm-delete-crawl"]');
  await until("window.testFinishDeletion && document.querySelector('[data-action=\"confirm-delete-crawl\"]').disabled", "Pending deletion must prevent duplicate actions");
  await pressKey("Escape");
  assert.ok(await evaluate("Boolean(document.querySelector('.delete-crawl-modal'))"), "Escape must not dismiss a pending deletion");
  await evaluate("window.testFinishDeletion()");
  await until("document.querySelector('.delete-crawl-modal [role=\"alert\"]')?.textContent.includes('Could not delete')", "A failed deletion must stay open for retry");
  assert.ok(await evaluate("Boolean(document.querySelector('[data-session-id=\"fixture-baseline\"]'))"), "Failed deletion must retain the saved card");
  await evaluate("window.testDeleteFailure = false; window.testHoldDeletion = false");
  await click('[data-action="confirm-delete-crawl"]');
  await until("!document.querySelector('.delete-crawl-modal') && !document.querySelector('[data-session-id=\"fixture-baseline\"]')", "Retry must delete only the chosen card");
  assert.ok(await evaluate("document.querySelector('.crawl-history-comparison')?.textContent.includes('1 of 2 selected')"), "Deleted cards must leave the comparison selection");
  await until("document.querySelector('[data-action=\"return-to-workspace\"]') && !document.querySelector('[data-action=\"return-to-workspace\"]').disabled", "An unrelated deletion must leave the current workspace available");
  await click('[data-action="return-to-workspace"]');
  await until("document.querySelector('.url-tree-toolbar')?.textContent.includes('1,205') && document.querySelector('[aria-label=\"Root URL\"]')?.value === 'https://another.test/docs/'", "Deleting another card must preserve the open crawl, its view and its results");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Storage");
  await evaluate("[...document.querySelectorAll('.settings-section:not([hidden]) button')].find((button) => button.textContent.trim() === 'Delete').click()");
  await until("document.querySelector('.delete-crawl-modal')?.textContent.includes('Another site')", "Settings deletion must use the same confirmation");
  await click('[data-action="cancel-delete-crawl"]');
  await click('[title="Close settings"]');
  await click('[aria-label="Crawl library"]');
  await click('[data-session-id="fixture-other"] [data-action="delete-saved-crawl"]');
  await click('[data-action="confirm-delete-crawl"]');
  await until("!document.querySelector('.delete-crawl-modal') && !document.querySelector('[data-session-id=\"fixture-other\"]')", "Deleting the open crawl must remove its card");
  assert.ok(await evaluate("document.querySelector('.crawl-home') && !document.querySelector('[data-action=\"return-to-workspace\"]') && !document.querySelector('.data-table')"), "Deleted current results must not remain accessible in the workbench");
  await evaluate("window.testOpenSessionFailure = false; localStorage.removeItem('ferrous-frog-settings'); localStorage.removeItem('ferrous-frog-last-url')");
  await reloadApp();
  assert.ok(await evaluate("Boolean(document.querySelector('.detail-empty'))"), "The URL inspector must reserve a stable panel before a URL is selected");
  assert.ok(await evaluate("document.querySelector('.data-table tbody tr:not(.virtual-spacer)').getBoundingClientRect().height <= 30 && document.querySelector('.grid').getBoundingClientRect().top < 155"), "The workbench should prioritize compact result rows and leave room for the URL inspector");
  assert.ok(await evaluate("document.querySelector('.overview-panel').getBoundingClientRect().width >= 300"), "A fresh install should use a readable overview width instead of treating a missing saved width as zero");
  await click('[aria-label="Toggle audit views"]');
  await until("document.querySelector('.issue-sidebar').getAnimations().every((animation) => animation.playState !== 'running')", "Audit navigation must finish sliding open");
  // Record the transition before clicking; a delayed CDP reply can outlast the entire slide.
  await evaluate(`for (const panel of document.querySelectorAll('.issue-sidebar, .overview-panel')) {
    panel.addEventListener('transitionrun', (event) => {
      if (event.target === panel && event.propertyName === 'translate' && panel.dataset.state === 'closed') {
        panel.dataset.testCloseSlide = 'started';
      }
    });
  }`);
  await evaluate("document.querySelector('[aria-label=\"Close audit views\"]').focus()");
  await click('[aria-label="Close audit views"]');
  assert.ok(await evaluate("document.querySelector('.issue-sidebar').inert && document.activeElement.matches('[aria-label=\"Toggle audit views\"]')"), "Closing audit navigation must remove its controls from keyboard focus and return to its toggle");
  await until("document.querySelector('.issue-sidebar').dataset.testCloseSlide === 'started'", "Audit navigation must animate its closing slide");
  await until("getComputedStyle(document.querySelector('.issue-sidebar')).visibility === 'hidden'", "Closed audit navigation must finish hidden");
  await evaluate("document.querySelector('[aria-label=\"Close overview\"]').focus()");
  await click('[aria-label="Close overview"]');
  assert.ok(await evaluate("document.querySelector('.overview-panel').inert && document.activeElement.matches('[aria-label=\"Toggle overview\"]')"), "Closing overview must return focus and disable its retained controls");
  await until("document.querySelector('.overview-panel').dataset.testCloseSlide === 'started'", "Overview must animate its closing slide");
  await until("getComputedStyle(document.querySelector('.overview-panel')).visibility === 'hidden'", "Closed overview must finish hidden");
  await click('[aria-label="Toggle overview"]');
  await until("document.querySelector('.overview-panel').getAnimations().every((animation) => animation.playState !== 'running')", "Overview must finish reopening before further interaction");
  assert.equal(await evaluate("testStartupTheme"), "dark", "System dark mode must apply before the app loads");
  await systemTheme("light");
  await until("!document.documentElement.classList.contains('dark')", "The default theme must follow operating-system changes");
  await menuItem("Dark Theme");
  await systemTheme("dark");
  await systemTheme("light");
  await delay(100);
  assert.ok(await evaluate("document.documentElement.classList.contains('dark')"), "An explicit dark choice must override the system");
  await reloadApp();
  assert.equal(await evaluate("testStartupTheme"), "dark", "An explicit preference must also apply before app startup after reload");
  await menuItem("System Theme");
  await until("!document.documentElement.classList.contains('dark')", "Returning to System must immediately use the current system theme");
  await reloadApp();
  assert.equal(await evaluate("testStartupTheme"), "light", "A saved System preference must use the current system theme before app startup");
  await systemTheme("dark");
  await until("document.documentElement.classList.contains('dark')", "System tracking must resume after clearing the manual choice");
  assert.equal(await evaluate("localStorage.getItem('ferrous-frog-theme')"), "system", "System mode must be saved as a preference, not frozen to dark or light");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(process.env.UI_SCREENSHOT, Buffer.from(shot.data, "base64"));
  }
  await openTools();
  assert.ok(await evaluate("[...document.querySelectorAll('[role=\"menuitemradio\"]')].find((item) => item.textContent.includes('System Theme')).getAttribute('aria-checked') === 'true'"), "The appearance menu must identify System as the active preference");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.appearance.png`, Buffer.from(shot.data, "base64"));
  }
  await pressKey("Escape");
  await evaluate("document.querySelector('.grid').scrollLeft = 640");
  await delay(100);
  assert.ok(await evaluate("(() => { const grid = document.querySelector('.grid').getBoundingClientRect(); const url = document.querySelector('.data-table tbody tr:not(.virtual-spacer) td:nth-child(2)').getBoundingClientRect(); return url.left >= grid.left && url.right <= grid.right; })()"), "The URL must stay visible when scrolling a wide results grid horizontally");
  await evaluate("document.querySelector('.grid').scrollLeft = 0");
  await click('[data-category="Page titles"]');
  await until("document.querySelector('[aria-label=\"Audit view\"]').value === 'all' && document.querySelector('.data-table thead').textContent.includes('Title length')", "A category should show all URLs with relevant columns before narrowing to an issue");
  await click('[data-category="Crawl overview"]');
  await click('[aria-label="Configure columns"]');
  await until("document.querySelector('.column-layout-modal')", "Column settings must open");
  const toggleColumn = async (label) => {
    await evaluate(`(() => { const row = [...document.querySelectorAll('.column-choice')].find((item) => item.querySelector('label').textContent.trim() === ${JSON.stringify(label)}); row.querySelector('input').click(); })()`);
  };
  await toggleColumn("Title");
  assert.ok(await evaluate("![...document.querySelectorAll('.data-table th')].some((cell) => cell.textContent.trim() === 'Title')"), "Hiding one column must preserve the rest of the grid");
  await toggleColumn("Final URL");
  await click('[aria-label="Move Final URL left"]');
  await fill('[aria-label="Layout name"]', 'Link review');
  await evaluate("document.querySelector('.column-layout-save').requestSubmit()");
  await until("document.querySelector('[aria-label=\"Visible columns\"]').value === 'saved:Link review'", "A named layout must become active");
  const savedColumns = await evaluate("[...document.querySelectorAll('.data-table th')].map((cell) => cell.textContent.trim())");
  assert.equal(savedColumns.at(-2), "Final URL", "Column ordering must affect the actual grid");
  assert.ok(await evaluate("[...document.querySelectorAll('.column-choice')].find((item) => item.querySelector('label').textContent.trim() === 'URL').querySelector('input').disabled"), "The URL identity column must always remain visible");
  for (const theme of ["dark", "light"]) {
    await systemTheme(theme);
    for (const width of [1280, 390]) {
      await cdp("Emulation.setDeviceMetricsOverride", { width, height: 840, deviceScaleFactor: 1, mobile: false });
      await delay(80);
      assert.ok(await evaluate("document.querySelector('.column-layout-modal').scrollWidth <= document.querySelector('.column-layout-modal').clientWidth && document.querySelector('.column-choices').clientHeight > 100"), "Column controls must fit both themes and narrow windows");
      if (process.env.UI_SCREENSHOT) {
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.columns-${theme}-${width}.png`, Buffer.from(shot.data, "base64"));
      }
    }
  }
  await systemTheme("dark");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await click('[aria-label="Close columns"]');
  await reloadApp();
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.data-table th')].map((cell) => cell.textContent.trim())"), savedColumns, "Column visibility and order must survive reload");
  await click('[data-category="Page titles"]');
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.data-table th')].map((cell) => cell.textContent.trim())"), savedColumns, "Named layouts must remain fixed across audit views");
  await click('[aria-label="Configure columns"]');
  await fill('[aria-label="Layout name"]', 'link review');
  await evaluate("document.querySelector('.column-layout-save').requestSubmit()");
  await until("document.querySelector('.column-layout-modal [role=\"alert\"]')?.textContent.includes('already exists')", "Duplicate layout names must not silently overwrite a preset");
  await evaluate("[...document.querySelectorAll('.column-layout-modal button')].find((button) => button.textContent === 'Delete layout').click()");
  assert.ok(await evaluate("!JSON.parse(localStorage.getItem('ferrous-frog-column-layouts')).presets.length"), "Saved layouts must be removable without deleting crawl data");
  await evaluate("[...document.querySelectorAll('.column-layout-modal button')].find((button) => button.textContent === 'Use relevant columns').click()");
  await click('[aria-label="Close columns"]');
  await until("document.querySelector('.data-table thead').textContent.includes('Title length')", "Relevant columns must restore the selected audit's default");
  await evaluate("localStorage.setItem('ferrous-frog-column-layouts', JSON.stringify({version:1, active:'saved:bad', custom:[null], presets:[]}))");
  await reloadApp();
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Visible columns\"]').value"), "relevant", "Malformed saved layouts must fall back safely");
  await click('[data-category="Crawl overview"]');
  await click('[aria-label="Toggle audit views"]');
  await until("document.querySelector('.issue-sidebar')", "The complete audit tree must remain available");
  assert.ok(await contrast('.issue-sidebar > p', '.issue-sidebar') >= 4.5, "Dark theme secondary text must remain readable on panels");
  assert.ok(await evaluate("Boolean(document.querySelector('[aria-label=\"Next page\"]'))"), "Results need a next-page control; the first 1,000 rows must not be a dead end");
  await evaluate("window.testMultipleMetadata = true");
  for (const [view, column, count] of [["titleMultiple", "Title tags", "3"], ["metaMultiple", "Meta tags", "2"]]) {
    await click(`[data-view="${view}"]`);
    await until(`testQueries.at(-1).view === '${view}' && document.querySelector('[data-view="${view}"] .issue-count')?.textContent === '1'`, "Multiple-tag views must query the engine and count affected pages");
    await until(`document.querySelector('.data-table thead').textContent.includes('${column}') && document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)').length === 1`, "Multiple-tag views must show count evidence with the retained first value");
    assert.equal(await evaluate(`(() => { const index = [...document.querySelectorAll('.data-table th')].findIndex((cell) => cell.textContent.includes('${column}')); return document.querySelector('.data-table tbody tr:not(.virtual-spacer)').children[index].textContent; })()`), count, "Known tag counts must be displayed");
    await evaluate(`[...document.querySelectorAll('.data-table th button')].find((button) => button.textContent.includes('${column}')).click()`);
    await until(`testQueries.at(-1).sortBy === '${view === "titleMultiple" ? "titleCount" : "metaDescriptionCount"}'`, "Tag-count sorting must run in storage");
  }
  await evaluate("window.testMultipleMetadata = false");
  await click('[data-view="canonicalLoop"]');
  await until("testQueries.at(-1).view === 'canonicalLoop'", "Canonical loops must have an engine-backed audit filter");
  await evaluate("window.testPaginationErrors = true");
  await click('[data-view="paginationNextToError"]');
  await until("testQueries.at(-1).view === 'paginationNextToError' && document.querySelector('[data-view=\"paginationNextToError\"] .issue-count')?.textContent === '2'", "Pagination target errors must query storage and use source-record counts");
  assert.ok(await evaluate("document.querySelector('.data-table thead').textContent.includes('Next URL') && document.querySelector('.data-table thead').textContent.includes('Previous URL')"), "Pagination audits must expose declared relation targets");
  assert.ok(await evaluate("document.querySelector('.data-table tbody').textContent.includes('https://example.test/missing-next')"), "Pagination rows must show the declared failing target");
  await evaluate("testEmitProgress(0)");
  assert.equal(await evaluate("document.querySelector('[data-view=\"paginationNextToError\"] .issue-count').textContent"), "2", "Progress must retain query-owned pagination diagnostics");
  await click('[data-view="paginationPrevToError"]');
  await until("testQueries.at(-1).view === 'paginationPrevToError' && document.querySelector('[data-view=\"paginationPrevToError\"] .issue-count')?.textContent === '1'", "Previous URL errors must have their own query and count");
  await evaluate("window.testPaginationErrors = false");
  await evaluate("window.testPaginationLoops = true");
  for (const [view, count] of [["paginationNextLoop", "2"], ["paginationPrevLoop", "1"]]) {
    await click(`[data-view="${view}"]`);
    await until(`testQueries.at(-1).view === '${view}' && document.querySelector('[data-view="${view}"] .issue-count')?.textContent === '${count}'`, 'Direction-specific pagination loops must query the engine and show source counts');
    assert.ok(await evaluate("document.querySelector('.data-table thead').textContent.includes('Next URL') && document.querySelector('.data-table thead').textContent.includes('Previous URL')"), 'Loop audits must preserve declared target evidence');
    await evaluate('testEmitProgress(0)');
    assert.equal(await evaluate(`document.querySelector('[data-view="${view}"] .issue-count').textContent`), count, 'Progress must preserve query-owned pagination loop counts');
  }
  await evaluate("window.testPaginationLoops = false");
  await evaluate("window.testPaginationReturns = true");
  for (const [view, count, target] of [["paginationNextNonReciprocal", "2", "next-without-return"], ["paginationPrevNonReciprocal", "1", "prev-without-return"]]) {
    await click(`[data-view="${view}"]`);
    await until(`testQueries.at(-1).view === '${view}' && document.querySelector('[data-view="${view}"] .issue-count')?.textContent === '${count}'`, 'Pagination return-link warnings must query storage and count source records');
    assert.ok(await evaluate(`document.querySelector('.data-table tbody').textContent.includes('https://example.test/${target}')`), 'Return-link warnings must expose the declared target');
    await evaluate('testEmitProgress(0)');
    assert.equal(await evaluate(`document.querySelector('[data-view="${view}"] .issue-count').textContent`), count, 'Progress must preserve query-owned pagination return-link counts');
  }
  await evaluate("window.testPaginationReturns = false");
  await evaluate("window.testPaginationMultiple = true");
  await click('[data-view="paginationMultipleTargets"]');
  await until("testQueries.at(-1).view === 'paginationMultipleTargets' && document.querySelector('[data-view=\"paginationMultipleTargets\"] .issue-count')?.textContent === '1'", "Multiple pagination declarations must query storage and count source pages");
  assert.ok(await evaluate("document.querySelector('.data-table thead').textContent.includes('Next Targets') && document.querySelector('.data-table tbody').textContent.includes('205')"), "Multiple target inventory must show the measured count");
  await evaluate("document.querySelector('.data-table tbody tr:not(.virtual-spacer)').click()");
  await click('#detail-tab-links');
  assert.ok(await evaluate("document.querySelector('#detail-panel-links')?.textContent.includes('Pagination audits check every captured target')"), "The inspector must explain that first-target columns do not limit pagination diagnostics");
  await until("document.querySelector('.pagination-targets.next')?.textContent.includes('https://example.test/other-next') && document.querySelectorAll('.pagination-targets.next li').length === 100", "The inspector must show a bounded first page of captured pagination targets");
  await click('.pagination-targets.next [aria-label="Last page"]');
  await until("document.querySelectorAll('.pagination-targets.next li').length === 5 && document.querySelector('.pagination-targets.next')?.textContent.includes('https://example.test/extra-next-202')", "The final pagination target must remain inspectable without rendering every target at once");
  await evaluate("testSetSummary({ paginationMultipleTargets: 1 }); testEmitProgress(0)");
  assert.equal(await evaluate("document.querySelector('[data-view=\"paginationMultipleTargets\"] .issue-count').textContent"), "1", "Progress must include measured pagination declaration counts");
  await evaluate("testSetSummary({ paginationMultipleTargets: 0 }); testEmitProgress(0)");
  await until("document.querySelector('[data-view=\"paginationMultipleTargets\"] .issue-count')?.textContent === '0'", "Live progress must replace stale pagination declaration counts");
  await click('#detail-tab-page');
  await evaluate("window.testPaginationMultiple = false");
  await evaluate("window.testAmpError = true");
  await click('[data-view="ampToError"]');
  await until("testQueries.at(-1).view === 'ampToError' && document.querySelector('[data-view=\"ampToError\"] .issue-count')?.textContent === '1'", "AMP target errors must query storage and count source rows");
  assert.ok(await evaluate("document.querySelector('.data-table thead').textContent.includes('AMP URL') && document.querySelector('.data-table tbody').textContent.includes('https://example.test/missing-amp')"), "AMP audit rows must show the declared target");
  await evaluate("testEmitProgress(0)");
  assert.equal(await evaluate("document.querySelector('[data-view=\"ampToError\"] .issue-count').textContent"), '1', "Progress must retain query-owned AMP diagnostics");
  await evaluate("window.testAmpError = false");
  await evaluate("window.testAmpReturns = true");
  await click('[data-view="ampNonReciprocal"]');
  await until("testQueries.at(-1).view === 'ampNonReciprocal' && document.querySelector('[data-view=\"ampNonReciprocal\"] .issue-count')?.textContent === '1'", "AMP canonical-return warnings must query storage and count measured sources");
  assert.ok(await evaluate("document.querySelector('.data-table thead').textContent.includes('AMP URL') && document.querySelector('.data-table tbody').textContent.includes('https://example.test/amp-without-return')"), "AMP return-link warnings must expose the declared target");
  await evaluate('testEmitProgress(0)');
  assert.equal(await evaluate("document.querySelector('[data-view=\"ampNonReciprocal\"] .issue-count').textContent"), '1', "Progress must retain query-owned AMP reciprocity counts");
  await evaluate("window.testAmpReturns = false");
  await evaluate("testExactDuplicates = 3");
  await click('[data-view="exactDuplicate"]');
  await until("testQueries.at(-1).view === 'exactDuplicate' && document.querySelector('.data-table thead').textContent.includes('Response hash')", "Exact duplicate audits must query storage and show full-response hash evidence");
  await until("document.querySelector('[data-view=\"exactDuplicate\"] .issue-count')?.textContent === '3'", "Exact duplicate counts must come from the storage query");
  await evaluate("testEmitProgress(0)");
  assert.equal(await evaluate("document.querySelector('[data-view=\"exactDuplicate\"] .issue-count').textContent"), '3', "Cheap progress events must retain expensive query-owned duplicate counts");
  await evaluate("testExactDuplicates = 0");
  await click('[data-view="all"]');
  await until("testQueries.at(-1).view === 'all' && document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "All URLs must restore the results");
  const exportMenu = async (label) => {
    await evaluate("document.querySelector('.export-trigger').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
    await until("document.querySelector('[role=\"menuitem\"]')", "Export menu must open");
    await evaluate(`[...document.querySelectorAll('[role="menuitem"]')].find((item) => item.textContent.trim() === ${JSON.stringify(label)}).click()`);
  };
  const selectGridRow = (index, modifiers = {}) => evaluate(`document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[${index}].dispatchEvent(new MouseEvent('click', { bubbles: true, ...${JSON.stringify(modifiers)} }))`);
  await click('[aria-label="Advanced filters"]');
  await until("document.querySelector('.advanced-filters-modal')", "Advanced filters must open a draft editor");
  await click('[aria-label="Add filter condition"]');
  await select('Filter field 1', 'url');
  await fill('[aria-label="Filter value 1"]', 'page-12');
  assert.ok(await evaluate("!testQueries.at(-1).filters"), "Editing a filter draft must not change results");
  await click('[aria-label="Add filter condition"]');
  await select('Filter field 2', 'responseTimeMs');
  await select('Filter operator 2', 'greaterThan');
  await fill('[aria-label="Filter value 2"]', '40');
  await evaluate("document.querySelector('.advanced-filters-modal form').requestSubmit()");
  await until("testQueries.at(-1).filters?.rules.length === 2 && !document.querySelector('.advanced-filters-modal')", "Validated all-condition filters must reach server-side paging");
  const combinedFilter = { match: 'all', rules: [{ field: 'url', operator: 'contains', value: 'page-12' }, { field: 'responseTimeMs', operator: 'greaterThan', value: '40' }] };
  assert.deepEqual(await evaluate("testQueries.at(-1).filters"), combinedFilter);
  assert.equal(await evaluate("testQueries.at(-1).offset"), 0, "Applying filters must reset the page");
  await exportMenu('CSV');
  await until("testExportRequest?.kind === 'csv'", "Filtered CSV must remain available");
  assert.deepEqual(await evaluate("testExportRequest.query.filters"), combinedFilter, "Grid exports must preserve the full advanced filter group");
  for (const [label, kind] of [['Response Headers CSV', 'responseHeadersCsv'], ['Raw HTML CSV', 'rawHtmlCsv'], ['Rendered HTML CSV', 'renderedHtmlCsv'], ['Visible Text CSV', 'visibleTextCsv']]) {
    await exportMenu(label);
    await until(`testExportRequest?.kind === ${JSON.stringify(kind)} && !document.querySelector('[title="Start crawl"]').disabled`, "Captured-data exports must finish through the native export command");
    assert.deepEqual(await evaluate("testExportRequest.query.filters"), combinedFilter, "Captured-data exports must preserve advanced filters");
  }

  await click('[aria-label="Tree view"]');
  await until("testTreeQuery?.filters?.rules.length === 2", "The directory tree must use the same filters as the grid");
  assert.deepEqual(await evaluate("testTreeQuery.filters"), combinedFilter);
  await click('[aria-label="Table view"]');
  await evaluate("testTreeDelay = 2000; testTreeTotal = 1234; testEmit({ kind: 'started' })");
  await click('[aria-label="Tree view"]');
  await until("document.querySelector('.url-tree-view')?.textContent.includes('1,234')", "Slow live tree queries must finish and display results without being repeatedly superseded");
  await evaluate("testTreeDelay = 0; testTreeTotal = 1205; testEmit({ kind: 'finished' })");
  await click('[aria-label="Table view"]');
  await click('[aria-label="Advanced filters"]');
  await select('Match conditions', 'any');
  await fill('[aria-label="Filter value 1"]', 'page-999');
  await evaluate("testFilterValidationFailure = true; document.querySelector('.advanced-filters-modal form').requestSubmit()");
  await until("document.querySelector('.advanced-filters-modal [role=\"alert\"]')?.textContent.includes('Invalid filter value')", "Native validation errors must preserve the previous filter");
  assert.deepEqual(await evaluate("testQueries.at(-1).filters"), combinedFilter);
  await evaluate("testFilterValidationFailure = false; document.querySelector('.advanced-filters-modal form').requestSubmit()");
  await until("testQueries.at(-1).filters?.match === 'any' && !document.querySelector('.advanced-filters-modal')", "Any-condition groups must apply after successful validation");
  await click('[aria-label="Advanced filters"]');
  await select('Match conditions', 'all');
  await evaluate("testHoldFilterValidation = true; document.querySelector('.advanced-filters-modal form').requestSubmit()");
  await until("Boolean(testFinishFilterValidation)", "Filter validation may be pending");
  await click('[aria-label="Cancel advanced filters"]');
  await evaluate("testFinishFilterValidation(); testHoldFilterValidation = false");
  await delay(100);
  assert.equal(await evaluate("testQueries.at(-1).filters.match"), 'any', "Cancel must discard a pending filter application");
  await click('[aria-label="Advanced filters"]');
  for (const [theme, width] of [['dark', 1280], ['light', 390]]) {
    await systemTheme(theme);
    await cdp('Emulation.setDeviceMetricsOverride', { width, height: 800, deviceScaleFactor: 1, mobile: false });
    assert.ok(await evaluate("document.querySelector('.advanced-filters-modal').scrollWidth <= document.querySelector('.advanced-filters-modal').clientWidth"), 'Filter controls must fit both themes and narrow screens');
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.filters-${theme}-${width}.png`, Buffer.from(shot.data, 'base64'));
    }
  }
  await systemTheme('dark');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await click('[aria-label="Remove filter condition 2"]');
  await click('[aria-label="Remove filter condition 1"]');
  await evaluate("document.querySelector('.advanced-filters-modal form').requestSubmit()");
  await until("!testQueries.at(-1).filters && !document.querySelector('.advanced-filters-modal')", "An empty group must remove advanced filters");
  await selectGridRow(0);
  await selectGridRow(2, { ctrlKey: true });
  await until("document.querySelector('.selection-count')?.textContent.includes('2 selected')", "Control-click must select multiple rows");
  await exportMenu("Selected Rows CSV");
  await until("testExportRequest?.kind === 'selectedCsv' && document.querySelector('.notice-bar')?.textContent.includes('2 rows')", "Selected CSV must export only chosen records");
  assert.deepEqual(await evaluate("testExportRequest.recordIds"), [1, 3], "Selected exports must preserve selection order without grid filters");
  await exportMenu("Copy Selected Rows");
  await until("testCopiedText?.includes('page-3')", "Clipboard export must use the native selected-record query");
  assert.deepEqual(await evaluate("testCopiedIds"), [1, 3]);
  assert.ok(!(await evaluate("testCopiedText")).includes('page-2'), "Clipboard CSV must exclude unselected rows");
  await selectGridRow(4, { shiftKey: true });
  await until("document.querySelector('.selection-count')?.textContent.includes('3 selected')", "Shift-click must select the visible range from its anchor");
  await evaluate("testClipboardFailure = true");
  await exportMenu("Copy Selected Rows");
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Clipboard denied')", "Clipboard failures must be visible and release the workspace");
  await evaluate("testClipboardFailure = false");
  await click('[aria-label="Dismiss error"]');
  await evaluate("(() => { const row = document.querySelector('.data-table tbody tr:not(.virtual-spacer)'); row.focus(); row.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true })); })()");
  await until("document.querySelector('.selection-count')?.textContent.includes('500 selected')", "Select all must be bounded to the current server page");
  await click('[aria-label="Last page"]');
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-1001')", "Last page must load its first URL");
  assert.ok(await evaluate("!document.querySelector('.selection-count')"), "Changing pages must clear stale selection IDs");
  await evaluate("document.querySelector('.grid').scrollTop = document.querySelector('.grid').scrollHeight");
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-1205')", "Last page must reach URLs beyond row 1,000");
  assert.ok(await evaluate("testQueries.at(-1).offset > 0 && testQueries.at(-1).limit <= 1000"));
  await search("page-12");
  await until("testQueries.at(-1).globalSearch === 'page-12' && testQueries.at(-1).offset === 0", "Search must reset paging");
  await evaluate("testSearchDelays['page-2'] = 600");
  await search("page-2");
  await until("testQueries.at(-1).globalSearch === 'page-2'", "Slow query should be in flight");
  await search("page-999");
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-999')", "Latest search should load");
  await delay(800);
  assert.ok(await evaluate("document.querySelector('.data-table tbody').textContent.includes('page-999')"), "Stale response must not replace latest search");
  await search("no-such-url");
  await until("document.querySelector('.grid-empty')", "A search with no matches must show a recovery action");
  assert.ok(await evaluate("!document.querySelector('.export-trigger').disabled"), "An empty filter must not block whole-crawl exports");
  await evaluate("document.querySelector('.export-trigger').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
  await until("document.querySelector('[role=\"menuitem\"]')", "Export menu must open for existing crawl data");
  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'CSV').getAttribute('aria-disabled')"), "true", "Filtered CSV should be disabled without matching URLs");
  assert.notEqual(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Crawl Archive').getAttribute('aria-disabled')"), "true", "Whole-crawl archives remain available");
  assert.ok(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].some((item) => item.textContent.trim() === 'Audit Workbook (XLSX)' && item.getAttribute('aria-disabled') !== 'true')"), "A whole-crawl audit workbook must be available in Export");
  await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Audit Workbook (XLSX)').click()");
  await until("testExportRequest?.kind === 'auditWorkbook'", "Workbook export must reach the desktop command");
  assert.deepEqual(await evaluate("testExportRequest"), { kind: "auditWorkbook", query: null, graphQuery: null }, "Workbook export must include the whole crawl even with an empty result filter");
  await until("document.querySelector('.notice-bar')?.textContent.includes('1,205')", "Workbook success must show the source URL count");
  for (const failure of [false, true]) {
    await evaluate(`testExportFailure = ${failure}; testHoldExport = true; testExportRequest = undefined; testFinishExport = undefined`);
    await exportMenu("Crawl Archive");
    await until("testFinishExport && testExportRequest?.kind === 'crawlArchive' && document.querySelector('[title=\"Start crawl\"]').disabled && document.querySelector('.notice-bar')?.textContent.includes('Creating')", "A pending archive must show progress and keep its source workspace stable");
    assert.deepEqual(await evaluate("testExportRequest"), { kind: "crawlArchive", query: null, graphQuery: null }, "Archives must preserve the entire crawl independently of grid filters and graph limits");
    assert.ok(await evaluate("document.querySelector('.export-trigger').disabled && document.querySelector('[aria-label=\"Crawl library\"]').disabled"), "An archive write must prevent conflicting exports and session replacement");
    await evaluate("testFinishExport()");
    if (failure) {
      await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Could not write the crawl archive') && !document.querySelector('[title=\"Start crawl\"]').disabled", "Archive failures must release the workspace and remain visible");
      assert.ok(await evaluate("!document.querySelector('.notice-bar')"), "A failed archive must clear its pending status");
      await click('[aria-label="Dismiss error"]');
    } else {
      await until("document.querySelector('.notice-bar')?.textContent.includes('1,205 rows to /tmp/ferrous-frog.ffcrawl.json') && !document.querySelector('[title=\"Start crawl\"]').disabled", "A completed archive must report its path and release the workspace");
    }
    await evaluate("testExportFailure = false; testHoldExport = false");
  }
  await exportMenu("Image Alt Text CSV");
  await until("testExportRequest?.kind === 'imageAltCsv'", "Image-alt exports must reach the desktop command even with an empty grid filter");
  assert.deepEqual(await evaluate("testExportRequest"), { kind: "imageAltCsv", query: null, graphQuery: null }, "Image-alt reports cover all image occurrences in the crawl");
  await evaluate("testExportFailure = true; testHoldExport = true; testExportRequest = undefined");
  await evaluate("document.querySelector('.export-trigger').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
  await until("document.querySelector('[role=\"menuitem\"]')", "Export menu must reopen");
  await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Audit Workbook (XLSX)').click()");
  await until("testExportRequest?.kind === 'auditWorkbook' && document.querySelector('[title=\"Start crawl\"]').disabled && document.querySelector('.notice-bar')?.textContent.includes('Creating')", "A pending workbook must show progress and prevent a new crawl from replacing its source");
  await evaluate("testFinishExport()");
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Could not write') && !document.querySelector('[title=\"Start crawl\"]').disabled", "A failed workbook must display the error and release the workspace");
  assert.ok(await evaluate("!document.querySelector('.notice-bar')"), "A failed export must clear its pending status");
  await click('[aria-label="Dismiss error"]');
  await evaluate("testExportFailure = false; testHoldExport = false");
  await evaluate("testEmit({ kind: 'started' })");
  await evaluate("testEmit({ kind: 'notice', message: 'Server requested Retry-After for https://example.test' })");
  await until("document.querySelector('.notice-bar')?.textContent.includes('Retry-After')", "Server-requested cooldowns must explain why the crawl is waiting");
  await evaluate("testEmitProgress(7)");
  await exportMenu("Queued URLs CSV");
  await until("testExportRequest?.kind === 'queuedUrlsCsv' && document.querySelector('.notice-bar')?.textContent.includes('7 rows')", "Queued URLs must export during a live crawl");
  await evaluate("document.querySelector('.export-trigger').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
  await until("document.querySelector('[role=\"menuitem\"]')", "Export must remain accessible during a crawl");
  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Audit Workbook (XLSX)').getAttribute('aria-disabled')"), "true", "Workbook reports require a stopped or completed crawl");
  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'XLSX').getAttribute('aria-disabled')"), "true", "Paged XLSX reports require a stable stopped crawl");
  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Image Alt Text CSV').getAttribute('aria-disabled')"), "true", "Paged image reports require a stable stopped crawl");
  assert.ok(await evaluate("['Response Headers CSV', 'Raw HTML CSV', 'Rendered HTML CSV', 'Visible Text CSV'].every((label) => [...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === label).getAttribute('aria-disabled') === 'true')"), "Captured evidence exports require a stopped crawl");

  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Crawl Archive').getAttribute('aria-disabled')"), "true", "Archives require a stopped or completed crawl");
  await pressKey("Escape");
  await click('[title="Pause crawl"]');
  await until("document.querySelector('[title=\"Resume crawl\"]')", "The crawl must pause before testing paused archive availability");
  await evaluate("document.querySelector('.export-trigger').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))");
  await until("document.querySelector('[role=\"menuitem\"]')", "The export menu must remain accessible while paused");
  assert.equal(await evaluate("[...document.querySelectorAll('[role=\"menuitem\"]')].find((item) => item.textContent.trim() === 'Crawl Archive').getAttribute('aria-disabled')"), "true", "Paused workers must not overlap an archive snapshot");
  await pressKey("Escape");
  await click('[title="Resume crawl"]');
  await until("document.querySelector('[title=\"Pause crawl\"]')", "The paused crawl must remain resumable");
  await evaluate("testRecovery = { recoverable: true, queued: 7, seen: 1212, crawled: 1205 }; testEmitProgress(0)");
  await evaluate("testEmit({ kind: 'finished' })");
  await until("document.querySelector('.grid-empty button')", "The final refresh must restore the empty-filter recovery control");
  await exportMenu("Queued URLs CSV");
  await until("testExportRequest?.kind === 'queuedUrlsCsv' && document.querySelector('.notice-bar')?.textContent.includes('7 rows')", "Stopped crawls must export saved in-flight URLs even when the last progress queue count was zero");
  await evaluate("testRecovery = { recoverable: false, queued: 0, seen: 0, crawled: 1205 }; testEmit({ kind: 'finished' })");
  await until("!document.querySelector('.notice-bar')", "A completed crawl must clear stale cooldown or progress notices");
  await until("document.querySelector('.grid-empty button') && document.querySelector('.grid').getAttribute('aria-busy') === 'false'", "The final crawl query must settle before resetting its empty filter");
  await click('.grid-empty button');
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-1') && document.querySelector('[aria-label=\"Search results\"]').value === ''", "Reset filters must recover the crawl results");
  await search("");
  await click('[data-view="titleDuplicate"]');
  await until("testQueries.at(-1).view === 'titleDuplicate'", "Grouped issue view must query the engine");
  await evaluate("testDuplicates = true; testEmit({ kind: 'started' })");
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-2')", "Duplicate views must refresh while the crawl runs");
  assert.ok(await contrast('.destructive') >= 4.5, "The active Stop button must stay readable in dark mode");
  await evaluate("testEmit({ kind: 'finished' })");
  await evaluate("testEmit({ kind: 'started' })");
  await evaluate("testEmit({ kind: 'failed', message: 'Invalid seed URL' })");
  await until("document.querySelector('button[title=\"Start crawl\"]') && document.querySelector('[role=\"alert\"]')?.textContent.includes('Invalid seed URL')", "Fatal crawl errors must restore the Start control");
  await click('[aria-label="Dismiss error"]');
  await until("!document.querySelector('[role=\"alert\"]')", "Errors must be dismissible after recovery");
  assert.ok(await evaluate("document.querySelectorAll('.data-table thead th').length < 15"), "Issue views should show relevant columns");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.detail-header')", "Selecting a row must populate the fixed inspector");
  await until("document.querySelector('.detail-panel [role=\"tab\"][aria-selected=\"true\"]')", "Details must have an active section");
  await evaluate("document.querySelector('.detail-panel [role=\"tab\"][aria-selected=\"true\"]').focus()");
  await pressKey("ArrowRight");
  assert.ok(await evaluate("document.activeElement.id === 'detail-tab-inlinks' && document.activeElement.getAttribute('aria-selected') === 'true'"), "Arrow keys must move focus and activate the inline links tab");
  await until("document.querySelector('.selected-links .link-report-table tbody tr')", "Inlinks must load in the main inspector without a dialog");
  assert.ok(await evaluate("!document.querySelector('[role=\"dialog\"]')"), "Inspecting links should keep the crawl workspace accessible");
  await click('.selected-links [aria-label="Last page"]');
  await until("document.querySelector('.selected-links')?.textContent.includes('Link 1205')", "Inline links must page through the complete engine result");
  await fill('[aria-label="Search inlinks"]', 'Link 1205');
  await until("document.querySelector('.selected-links tbody tr')?.textContent.includes('Link 1205') && document.querySelector('.selected-links .grid-pagination')?.textContent.includes('1–1 of 1')", "Inline link search must reset paging and show its filtered result");
  await fill('[aria-label="Search inlinks"]', 'no-such-link');
  await until("document.querySelector('.selected-links .link-report-empty')?.textContent.includes('No link edges')", "An empty inline link search must keep the search control available");
  await fill('[aria-label="Search inlinks"]', '');
  await until("document.querySelector('.selected-links tbody tr')", "Clearing inline search must restore links");
  await click('.selected-links th:nth-child(2) button');
  await until("document.querySelector('.selected-links tbody tr')?.textContent.includes('Link 1205') && testLinkQueries.at(-1).sortDir === 'desc'", "Inline sorting must request the sorted engine page");
  await evaluate("document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await until("testLinkQueries.at(-1).targetUrl === 'https://example.test/page-2' && testLinkQueries.at(-1).offset === 0 && document.querySelector('.selected-links tbody tr td:nth-child(3)')?.textContent === 'https://example.test/page-2'", "Changing the selected URL must reset inline links to page one");
  await evaluate("testLinkDelays['https://example.test/page-1'] = 600; document.querySelector('.data-table tbody tr:not(.virtual-spacer)').click()");
  await until("testLinkQueries.at(-1).targetUrl === 'https://example.test/page-1'", "A slow request for the old selection should be in flight");
  await evaluate("document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await until("document.querySelector('.selected-links tbody tr td:nth-child(3)')?.textContent === 'https://example.test/page-2'", "The newer selection must load independently");
  await delay(700);
  assert.equal(await evaluate("document.querySelector('.selected-links tbody tr td:nth-child(3)').textContent"), "https://example.test/page-2", "Stale inline links must not overwrite a new selection");
  await evaluate("testLinkDelays['https://example.test/page-2'] = 1250; window.testSlowLinkStart = testLinkQueries.length; testEmit({ kind: 'started' })");
  await until("testLinkQueries.length > testSlowLinkStart && document.querySelector('.selected-links').getAttribute('aria-busy') === 'true'", "The slow live link query should be in flight");
  await until("document.querySelector('.selected-links tbody tr td:nth-child(3)')?.textContent === 'https://example.test/page-2' && document.querySelector('.selected-links').getAttribute('aria-busy') === 'false'", "Live links must render even when a query takes longer than the polling interval");
  assert.equal(await evaluate("testLinkQueries.length - testSlowLinkStart"), 1, "Slow live requests must not overlap");
  await evaluate("testEmit({ kind: 'finished' }); testLinkDelays['https://example.test/page-2'] = 15; testLinkFailure = true");
  await click('#detail-tab-outlinks');
  await until("testLinkQueries.at(-1).sourceUrl === 'https://example.test/page-2' && testLinkQueries.at(-1).targetUrl === null", "Outlinks must query the selected source URL");
  await until("document.querySelector('.selected-links [role=\"alert\"]')?.textContent.includes('Link query failed')", "Inline query failures must appear in the affected panel");
  await evaluate("testLinkFailure = false; document.querySelector('.selected-links [role=\"alert\"] button').click()");
  await until("document.querySelector('.selected-links.outlinks tbody tr') && !document.querySelector('.selected-links [role=\"alert\"]')", "Retry must recover an inline link query");
  await click('#detail-tab-inlinks');
  await until("document.querySelector('.selected-links.inlinks tbody tr')", "The inlinks tab should remain usable after changing direction");
  const beforeReferenceScenario = await evaluate("({ url: document.querySelector('.detail-url').textContent, session: testOpenedSession, linkDelays: testLinkDelays, desktop: window.isTauri })");
  await click('#detail-tab-references');
  await until("document.querySelectorAll('.selected-references tbody tr').length === 100", "References must load a bounded engine page in the URL inspector");
  assert.deepEqual(await evaluate("testReferenceQueries.at(-1)"), { sourceStorageKey: "https://example.test/page-2", offset: 0, limit: 100 }, "Reference evidence must query the selected storage occurrence");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.selected-references tbody tr')].slice(0, 6).map((row) => row.cells[0].textContent)"),
    ["Canonical targets", "Hreflang targets", "Pagination (next / previous)", "AMP targets", "Meta refresh targets", "Iframe sources"], "All reference types must retain their labels");
  assert.equal(await evaluate("document.querySelectorAll('.selected-references tbody tr')[1].cells[3].textContent"), "Yes", "Reference nofollow evidence must remain visible");
  const referenceViewport = await evaluate("({ width: innerWidth, height: innerHeight })");
  const referenceTheme = await evaluate("document.documentElement.classList.contains('dark')");
  for (const [width, height] of [[1280, 840], [390, 640]]) {
    await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    assert.ok(await evaluate("document.querySelector('.selected-references').scrollWidth <= document.querySelector('.selected-references').clientWidth && document.querySelector('.selected-references .link-report-table-wrap').scrollHeight > document.querySelector('.selected-references .link-report-table-wrap').clientHeight"), "Reference evidence must scroll inside its bounded inspector at every window width");
    for (const theme of ["dark", "light"]) {
      await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
      assert.ok(await contrast('.selected-references .link-report-table', '.detail-panel') >= 4.5, `Reference evidence must remain readable in ${theme} mode`);
    }
  }
  await cdp("Emulation.setDeviceMetricsOverride", { ...referenceViewport, deviceScaleFactor: 1, mobile: false });
  await evaluate(`document.documentElement.classList.toggle('dark', ${referenceTheme})`);
  await click('.selected-references [aria-label="Last page"]');
  await until("testReferenceQueries.at(-1).offset === 200 && document.querySelectorAll('.selected-references tbody tr').length === 5 && document.querySelector('.selected-references .grid-pagination').textContent.includes('201–205 of 205')", "References must page beyond the first 100 entries");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("testReferenceQueries.at(-1).sourceStorageKey === 'https://example.test/page-1' && testReferenceQueries.at(-1).offset === 0 && document.querySelector('.selected-references tbody')?.textContent.includes('page-1')", "Changing URL must reset reference paging");
  await evaluate("testReferenceDelays['https://example.test/page-2'] = 600; document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await until("testReferenceQueries.at(-1).sourceStorageKey === 'https://example.test/page-2'", "An old reference request must be in flight");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.selected-references tbody')?.textContent.includes('page-1')", "The current reference selection must load independently");
  await delay(700);
  assert.ok(await evaluate("!document.querySelector('.selected-references tbody').textContent.includes('page-2')"), "Stale references must not replace a newer selected URL");
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], storageKey: 'list:1:https://example.test/page-1' } }))");
  await until("testReferenceQueries.at(-1).sourceStorageKey === 'list:1:https://example.test/page-1' && document.querySelector('.selected-references tbody')?.textContent.includes('list%3A1')", "List reference queries must keep the first occurrence's identity");
  await click('.selected-references [aria-label="Last page"]');
  await until("testReferenceQueries.at(-1).offset === 200 && document.querySelectorAll('.selected-references tbody tr').length === 5", "Repeated List rows must retain paged evidence");
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], storageKey: 'list:2:https://example.test/page-1' } }))");
  await until("testReferenceQueries.at(-1).sourceStorageKey === 'list:2:https://example.test/page-1' && testReferenceQueries.at(-1).offset === 0 && document.querySelector('.selected-references tbody')?.textContent.includes('list%3A2')", "A second occurrence of the same URL must load its own first reference page");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("testReferenceQueries.at(-1).sourceStorageKey === 'https://example.test/page-1' && document.querySelector('.selected-references tbody')?.textContent.includes('page-1')", "The original fixture row must be restored before live grid updates");
  await evaluate("testReferenceDelays['https://example.test/page-1'] = 1250; window.testSlowReferenceStart = testReferenceQueries.length; testEmit({ kind: 'started' })");
  await until("testReferenceQueries.length > testSlowReferenceStart && document.querySelector('.selected-references').getAttribute('aria-busy') === 'true'", "Live reference refresh must enter its loading state");
  await until("document.querySelector('.selected-references tbody')?.textContent.includes('page-1') && document.querySelector('.selected-references').getAttribute('aria-busy') === 'false'", "Slow live references must settle without overlapping requests");
  assert.equal(await evaluate("testReferenceQueries.length - testSlowReferenceStart"), 1, "Reference polling must wait until the prior request settles");
  await evaluate("testReferenceDelays = {}; testReferenceFailure = true; testEmit({ kind: 'finished' })");
  await until("document.querySelector('.selected-references [role=\"alert\"]')?.textContent.includes('Reference query failed')", "Reference failures must stay inside their inspector panel");
  await evaluate("testReferenceFailure = false; testReferenceTotal = 0; document.querySelector('.selected-references [role=\"alert\"] button').click()");
  await until("document.querySelector('.selected-references .link-report-empty')?.textContent.includes('No stored references')", "Retry must recover into a clear empty state for older crawls");
  const referencesBeforePreview = await evaluate("testReferenceQueries.length");
  await evaluate("window.isTauri = false; document.querySelector('#detail-tab-page').click()");
  await click('#detail-tab-references');
  await until("document.querySelector('.selected-references .link-report-empty')?.textContent.includes('desktop app')", "The browser preview must explain unavailable reference storage");
  assert.equal(await evaluate("testReferenceQueries.length"), referencesBeforePreview, "Browser mode must not request desktop reference data");
  await evaluate("window.isTauri = true; testReferenceDelays = {}; testReferenceFailure = false; testReferenceTotal = undefined; document.querySelector('#detail-tab-page').click()");
  await click('#detail-tab-captured');
  await until("document.querySelector('.captured-data')?.textContent.includes('Not retained for this URL occurrence')", "Older or uncaptured pages must have an explicit retention state");
  assert.deepEqual(await evaluate("testCaptureQueries.at(-1)"), { sourceStorageKey: 'https://example.test/page-1', kind: 'responseHeaders' }, "Captured data must query only the selected occurrence and representation");
  await evaluate("testCaptureRetained = true");
  await click('.captured-data-controls button');
  await until("document.querySelector('.captured-data-preview')?.textContent.includes('[redacted]')", "Captured headers must remain plain normalized evidence with redaction");
  await select('Captured data kind', 'rawHtml');
  await until("document.querySelector('.captured-data-preview')?.textContent.includes('<main>') && document.querySelector('.captured-data')?.textContent.includes('Capture truncated')", "Raw HTML must identify storage truncation");
  assert.ok(await evaluate("!document.querySelector('.captured-data-preview img') && !window.testCaptureExecuted"), "Captured HTML must never execute or create HTML nodes");
  const capturePanels = await evaluate("({ issues: document.querySelector('.issue-sidebar').dataset.state === 'open', overview: document.querySelector('.overview-panel').dataset.state === 'open' })");
  for (const [width, height] of [[1280, 840], [390, 740]]) {
    await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    if (width === 390) {
      if (capturePanels.issues) await click('[aria-label="Close audit views"]');
      if (capturePanels.overview) await click('[aria-label="Close overview"]');
    }
    await until("document.getAnimations().every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity)", "The responsive side panels must settle before measuring captured data");
    assert.ok(await evaluate("document.querySelector('.captured-data').scrollWidth <= document.querySelector('.captured-data').clientWidth && document.querySelector('[aria-label=\"Captured data kind\"]').getBoundingClientRect().right <= innerWidth"), "Captured data controls and text must stay inside narrow detail panels");
    assert.ok(await evaluate("(() => { const select = document.querySelector('[aria-label=\"Captured data kind\"]'), rect = select.getBoundingClientRect(); return rect.width > 100 && document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2) === select; })()"), "Captured data choices must remain visible and reachable at every window width");
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.captured-data-${width}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 840, deviceScaleFactor: 1, mobile: false });
  if (capturePanels.issues) await click('[aria-label="Toggle audit views"]');
  if (capturePanels.overview) await click('[aria-label="Toggle overview"]');
  await evaluate("testCaptureDelays['https://example.test/page-1|visibleText'] = 600");
  await select('Captured data kind', 'visibleText');
  await until("testCaptureQueries.at(-1).kind === 'visibleText'", "The first evidence request must be pending before switching kind");
  await select('Captured data kind', 'renderedHtml');
  await until("document.querySelector('.captured-data')?.textContent.includes('Not retained')", "Unretained rendered HTML must not show another representation");
  await delay(700);
  assert.ok(await evaluate("!document.querySelector('.captured-data-preview')"), "A late representation response must not replace the selected kind");
  await evaluate("testCaptureDelays = {}");
  await select('Captured data kind', 'visibleText');
  await until("document.querySelector('.captured-data')?.textContent.includes('Preview limited to 64 KiB')", "Preview truncation must be distinguished from storage truncation");
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], storageKey: 'list:2:https://example.test/page-1' } }))");
  await until("testCaptureQueries.at(-1).sourceStorageKey === 'list:2:https://example.test/page-1' && document.querySelector('.captured-data-preview')?.textContent.includes('[redacted]')", "Another List occurrence must receive its own evidence and reset to the initial representation");
  await evaluate("testCaptureDelays['https://example.test/page-2|responseHeaders'] = 600; document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await until("testCaptureQueries.at(-1).sourceStorageKey === 'https://example.test/page-2'", "Old selected-URL evidence must be pending");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.captured-data-source')?.textContent.includes('page-1')", "The new occurrence evidence must load independently");
  await delay(700);
  assert.ok(await evaluate("!document.querySelector('.captured-data-source').textContent.includes('page-2')"), "Late captured data must not replace a new selected URL");
  await evaluate("testCaptureFailure = true; testCaptureDelays = {}");
  await click('.captured-data-controls button');
  await until("document.querySelector('.captured-data [role=\"alert\"]')?.textContent.includes('could not be read')", "Captured data errors must stay in the selected URL panel");
  await evaluate("testCaptureFailure = false");
  await click('.captured-data-controls button');
  await until("!document.querySelector('.captured-data [role=\"alert\"]') && document.querySelector('.captured-data-preview')", "Refresh must recover a failed evidence query");
  await click('#detail-tab-page');

  await evaluate("document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await click('#detail-tab-inlinks');
  await until("document.querySelector('.selected-links.inlinks tbody tr td:nth-child(3)')?.textContent === 'https://example.test/page-2' && document.querySelector('[title=\"Start crawl\"]')", "Reference inspection must restore the original selected row, stopped state and inlinks tab");
  assert.deepEqual(await evaluate("({ url: document.querySelector('.detail-url').textContent, session: testOpenedSession, linkDelays: testLinkDelays, desktop: window.isTauri })"), beforeReferenceScenario, "Reference checks must leave the original selection, session, link delays and desktop mode unchanged");
  await click('#inspection-tab-issues');
  await until("document.querySelector('#inspection-panel-issues:not([hidden]) .issue-summary-table')", "The right inspector must show issue counts");
  await evaluate("[...document.querySelectorAll('.issue-summary-table button')].find((button) => button.textContent.includes('Duplicate Titles')).click()");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Audit view\"]').value"), "titleDuplicate", "An issue must drill into its real engine filter");
  if (process.env.UI_SCREENSHOT) {
    await click('[aria-label="Close audit views"]');
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.workbench.png`, Buffer.from(shot.data, "base64"));
    await click('[aria-label="Toggle audit views"]');
  }
  await click('.selected-links [title="Open full link report"]');
  await until("document.querySelector('.link-report-modal [aria-label=\"Last page\"]')?.disabled === false", "URL details must load paged inlinks before navigation");
  await click('.link-report-modal [aria-label="Last page"]');
  await until("document.querySelector('.link-report-modal')?.textContent.includes('Link 1205')", "Link reports must reach beyond the first 500 edges");
  await evaluate("[...document.querySelectorAll('.link-report-tabs button')].find((button) => button.textContent === 'Sitemap Validation').click()");
  await until("testSitemapQuery?.offset === 0 && document.querySelectorAll('.sitemap-validation-table tbody tr').length === 2", "Sitemap reports must reset paging and retain repeated List occurrences");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.sitemap-validation-table tbody tr')].map((row) => row.cells[2].textContent)"), ["404", "200"]);
  await evaluate("[...document.querySelectorAll('.sitemap-validation-table th button')].find((button) => button.textContent === 'Status').click()");
  await until("testSitemapQuery?.sortBy === 'statusCode' && testSitemapQuery.sortDir === 'asc' && document.querySelector('.sitemap-validation-table tbody tr')?.cells[2].textContent === '200'", "Sitemap sorting must query storage and reorder repeated URLs without losing occurrences");
  assert.equal(await evaluate("document.querySelectorAll('.sitemap-validation-table tbody tr').length"), 2);
  await click('[title="Close link reports"]');
  await click('#detail-tab-page');
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], finalUrl: 'https://example.test/final-target' } }))");
  await until("document.querySelector('#detail-panel-page')?.textContent.includes('https://example.test/final-target')", "Redirected URLs must retain their final destination in the inspector");
  await click('#detail-tab-outlinks');
  await until("testLinkQueries.at(-1).sourceUrl === 'https://example.test/final-target'", "Redirected-page outlinks must use the fetched final URL");
  await click('#detail-tab-inlinks');
  await until("testLinkQueries.at(-1).targetUrl === 'https://example.test/page-1'", "Redirected-page inlinks must keep the original requested URL");
  await click('#detail-tab-page');
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: rows[0] }))");
  assert.ok(await evaluate("Boolean(document.querySelector('#detail-tab-pagespeed'))"), "Selected URLs need a PageSpeed measurement panel");
  await click('#detail-tab-pagespeed');
  await until("document.querySelector('.page-speed-panel')?.textContent.includes('No measurement yet')", "Historical rows without metrics must have an actionable empty state");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"PageSpeed device\"]').value"), "mobile", "PageSpeed should start with a mobile lab run");
  await click('[data-action="configure-pagespeed"]');
  await until("document.querySelector('.settings-section[data-settings-section=\"integrations\"]:not([hidden]) .page-speed-settings')", "PageSpeed must link directly to its credential controls");
  await fill('[aria-label="Google OAuth client ID"]', "1234.apps.googleusercontent.com");
  await fill('[aria-label="Google OAuth client secret"]', "GOCSPX-fixture");
  await click('[data-action="save-google-client"]');
  await until("document.querySelector('.google-account-settings')?.textContent.includes('Client saved, not connected') && document.querySelector('[aria-label=\"Google OAuth client secret\"]').value === ''", "Saving the OAuth client must clear the secret draft and report the state");
  assert.ok(!JSON.stringify(await evaluate("localStorage")).includes("GOCSPX"), "OAuth secrets must never reach browser storage");
  await click('[data-action="connect-google"]');
  await until("document.querySelector('.google-account-settings')?.textContent.includes('Connected (auto-refresh)')", "Connecting must report the refreshable token");
  await fill('[aria-label="Google Analytics property ID"]', "properties/987654");
  await click('[data-action="merge-analytics"]');
  await until("document.querySelector('[data-analytics-result]')?.textContent.includes('42') && document.querySelector('.notice-bar')?.textContent.includes('Merged 2 Google Analytics rows')", "GA4 merges must report fetched and matched rows");
  assert.deepEqual(await evaluate("({ property: testAnalyticsRequest.propertyId, dates: [typeof testAnalyticsRequest.startDate, typeof testAnalyticsRequest.endDate] })"), { property: "properties/987654", dates: ["string", "string"] }, "GA4 requests must carry the property and date range");
  await click('[data-action="disconnect-google"]');
  await until("document.querySelector('.google-account-settings')?.textContent.includes('Client saved, not connected')", "Disconnecting must keep the client");
  await click('[data-action="clear-google-client"]');
  await until("document.querySelector('.google-account-settings')?.textContent.includes('No OAuth client saved')", "Removing the client must reset the status");
  await fill('[aria-label="Backlink endpoint template"]', "https://api.example.test/backlinks");
  await click('[data-action="save-backlink-settings"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('{url}')", "Templates without a URL placeholder must be rejected");
  await click('[aria-label="Dismiss error"]');
  await fill('[aria-label="Backlink endpoint template"]', "https://api.example.test/backlinks?target={url}");
  await fill('[aria-label="Backlink credential header name"]', "X-Api-Key");
  await fill('[aria-label="Backlink credential header value"]', "backlink-secret");
  await click('[data-action="save-backlink-settings"]');
  await until("document.querySelector('.backlink-settings')?.textContent.includes('Endpoint saved') && document.querySelector('.backlink-settings').textContent.includes('Credential saved') && document.querySelector('[aria-label=\"Backlink credential header value\"]').value === ''", "Saving the backlink endpoint must report the state and clear the credential draft");
  assert.ok(!JSON.stringify(await evaluate("localStorage")).includes("backlink-secret"), "Backlink credentials must never reach browser storage");
  await fill('[aria-label="Backlink URL limit"]', "250");
  await click('[data-action="merge-backlinks"]');
  await until("document.querySelector('[data-backlink-result]')?.textContent.includes('3') && document.querySelector('.notice-bar')?.textContent.includes('Merged backlink metrics for 3 of 3 URLs')", "Backlink merges must report requested and matched rows");
  assert.equal(await evaluate("testBacklinkRequest.maxUrls"), 250, "Backlink runs must carry the URL limit");
  await settingsTab("AI");
  await select("AI provider", "openAiCompatible");
  await fill('[aria-label="AI model"]', "local-model");
  await fill('[aria-label="AI base URL"]', "http://127.0.0.1:11434/v1");
  await click('[data-action="save-ai-settings"]');
  await until("testAiSettings?.provider === 'openAiCompatible' && testAiSettings.model === 'local-model' && testAiSettings.baseUrl === 'http://127.0.0.1:11434/v1'", "AI settings must save provider, model and base URL");
  await fill('[aria-label="AI API key"]', "sk-fixture-ai");
  await click('[data-action="save-ai-key"]');
  await until("document.querySelector('.ai-settings')?.textContent.includes('AI API key saved') && document.querySelector('[aria-label=\"AI API key\"]').value === ''", "Saving the AI key must clear the draft and report the state");
  assert.ok(!JSON.stringify(await evaluate("localStorage")).includes("sk-fixture-ai"), "AI keys must never reach browser storage");
  await click('[title="Close settings"]');
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await click('#detail-tab-ai');
  await until("document.querySelector('#detail-panel-ai')?.textContent.includes('No AI results yet')", "Rows without AI results must show an empty state");
  await click('[data-action="run-ai-intent"]');
  await until("document.querySelector('#detail-panel-ai')?.textContent.includes('informational (90%)')", "Intent classification must render with its confidence");
  await click('[data-action="run-ai-metaDescription"]');
  await until("document.querySelector('#detail-panel-ai')?.textContent.includes('A concise explanation of the process.') && document.querySelector('#detail-panel-ai').textContent.includes('informational (90%)')", "Meta description drafts must render beside earlier results");
  await click('[data-action="run-ai-spelling"]');
  await until("document.querySelector('#detail-panel-ai')?.textContent.includes('“teh” → “the”')", "Spelling issues must render with suggestions");
  assert.deepEqual(await evaluate("testAiRequest.task"), "spelling", "AI requests must carry the task");
  await click('[data-action="configure-ai"]');
  await until("document.querySelector('.settings-section[data-settings-section=\"ai\"]:not([hidden])')", "The AI panel must link to its settings");
  await click('[data-action="clear-ai-key"]');
  await until("document.querySelector('.ai-settings')?.textContent.includes('No saved AI API key')", "Clearing the AI key must update the status");
  await click('[title="Close settings"]');
  await click('#detail-tab-pagespeed');
  await click('[data-action="configure-pagespeed"]');
  await until("document.querySelector('.settings-section[data-settings-section=\"integrations\"]:not([hidden])')", "PageSpeed settings must reopen after the AI flow");
  await fill('[aria-label="PageSpeed API key"]', 'fixture-pagespeed-secret');
  await click('[data-action="save-pagespeed-key"]');
  await until("document.querySelector('.page-speed-settings')?.textContent.includes('API key saved') && document.querySelector('[aria-label=\"PageSpeed API key\"]').value === ''", "Saving a key must clear the password input and return status only");
  assert.ok(await evaluate("testPageSpeedKey === 'fixture-pagespeed-secret' && !JSON.stringify(localStorage).includes('fixture-pagespeed-secret')"), "API credentials must never be saved in browser preferences");
  await fill('[aria-label="PageSpeed API key"]', 'discard-this-draft-secret');
  await click('[data-action="cancel-settings"]');
  await click('[data-action="configure-pagespeed"]');
  await until("document.querySelector('[aria-label=\"PageSpeed API key\"]')?.value === ''", "Closing Settings must discard an unsaved API key");
  await click('[data-action="clear-pagespeed-key"]');
  await until("document.querySelector('.page-speed-settings')?.textContent.includes('No saved API key')", "Users must be able to remove the saved key");
  await evaluate("testPageSpeedInvalidKey = true; [...document.querySelectorAll('.page-speed-settings button')].find(button => button.textContent === 'Check status').click()");
  await until("document.querySelector('.page-speed-settings')?.textContent.includes('replace or clear')", "Invalid stored credentials must explain recovery");
  assert.ok(await evaluate("!document.querySelector('[data-action=\"clear-pagespeed-key\"]').disabled"), "Clear must remain available even for an invalid saved key");
  await click('[data-action="clear-pagespeed-key"]');
  await evaluate("testPageSpeedKeyringFailure = true");
  await fill('[aria-label="PageSpeed API key"]', 'retry-key-secret');
  await click('[data-action="save-pagespeed-key"]');
  await until("document.querySelector('.page-speed-settings [role=\"alert\"]')?.textContent.includes('credential store')", "A keyring failure must be visible without claiming the key was saved");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"PageSpeed API key\"]').value === 'retry-key-secret' && !JSON.stringify(localStorage).includes('retry-key-secret')"), "Failed saves must permit retry without persisting plaintext");
  await evaluate("testPageSpeedKeyringFailure = false; testHoldPageSpeedCredentials = true");
  await click('[data-action="save-pagespeed-key"]');
  await until("window.testFinishPageSpeedCredentials", "The credential write fixture must hold its operation");
  await click('[data-action="cancel-settings"]');
  await until("!document.querySelector('.settings-modal')", "Closing Settings should leave the credential worker running");
  await click('[data-action="configure-pagespeed"]');
  await until("document.querySelector('[aria-label=\"PageSpeed API key\"]')?.value === ''", "Reopened Settings must not recover the plaintext key");
  assert.ok(await evaluate("document.querySelector('[data-action=\"save-pagespeed-key\"]').disabled && document.querySelector('[data-action=\"clear-pagespeed-key\"]').disabled"), "Reopening Settings must retain the pending credential write guard");
  await evaluate("testHoldPageSpeedCredentials = false; testFinishPageSpeedCredentials()");
  await until("document.querySelector('.page-speed-settings')?.textContent.includes('API key saved') && !document.querySelector('[data-action=\"clear-pagespeed-key\"]').disabled", "A completed credential write must update the reopened Settings status");
  await click('[data-action="clear-pagespeed-key"]');
  await until("document.querySelector('.page-speed-settings')?.textContent.includes('No saved API key')", "The credential fixture must leave anonymous measurements available");
  await click('[data-action="cancel-settings"]');
  await click('[data-action="run-pagespeed"]');
  await until("document.querySelector('.page-speed-result')?.textContent.includes('Lighthouse 13.0.0') && !document.querySelector('.page-speed-running')", "Successful measurements must show the persisted result and release the workspace");
  assert.equal(await evaluate("testPageSpeedRequest.recordId"), 1, "PageSpeed must measure only the selected record");
  assert.equal(await evaluate("testPageSpeedRequest.strategy"), "mobile");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.page-speed-scores dd')].map((cell) => cell.textContent)"), ["92", "Not available", "0", "100"], "Category scores must distinguish unavailable from a measured zero");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.page-speed-metrics dd')].map((cell) => cell.textContent)"), ["1,234 ms", "0 ms", "0"], "Lab results must retain LCP, TBT and CLS units and zeros");
  assert.ok(await evaluate("!document.querySelector('.page-speed-result').textContent.includes('INP') && document.querySelector('.page-speed-result').textContent.includes('https://example.test/measured-final')"), "Lab measurements must show their final URL and must not mislabel TBT as INP");
  await select('PageSpeed device', 'desktop');
  await evaluate("testHoldPageSpeed = true");
  await click('[data-action="run-pagespeed"]');
  await until("document.querySelector('.page-speed-running')", "Slow PageSpeed requests must remain visible and cancellable");
  assert.ok(await evaluate("document.querySelector('button[title=\"Start crawl\"]').disabled && document.querySelector('[aria-label=\"Crawl library\"]').disabled"), "A measurement must retain its crawl while the network request runs");
  await evaluate("document.querySelectorAll('.data-table tbody tr:not(.virtual-spacer)')[1].click()");
  await until("document.querySelector('.detail-url')?.textContent.endsWith('page-2')", "Grid selection must remain responsive during a PageSpeed request");
  await search('page-2');
  await evaluate("testFinishPageSpeed()");
  await until("!document.querySelector('.page-speed-running') && document.querySelector('.page-speed-panel')?.textContent.includes('No measurement yet')", "A late result must not attach itself to the newer selected URL");
  await until("testQueries.at(-1).globalSearch === 'page-2'", "Finishing a measurement must query the current filter instead of its starting filter");
  await search('');
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)')?.textContent.includes('page-1')", "Clearing the filter must restore the measured row");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.page-speed-result')?.textContent.includes('Desktop')", "Returning to the measured URL must show its latest saved device result");
  await evaluate("testHoldPageSpeed = false; testPageSpeedFailure = true");
  await click('[data-action="run-pagespeed"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('HTTP 429') && !document.querySelector('.page-speed-running')", "PageSpeed failures must remain visible and release the workspace");
  assert.ok(await evaluate("document.querySelector('.page-speed-result')?.textContent.includes('Desktop')"), "A failed refresh must preserve the previous measurement");
  await click('[aria-label="Dismiss error"]');
  await evaluate("testPageSpeedFailure = false; testHoldPageSpeed = true");
  await click('[data-action="run-pagespeed"]');
  await until("document.querySelector('.page-speed-running')", "A fresh measurement must recover after failure");
  await click('#detail-tab-page');
  await click('[data-action="cancel-pagespeed"]');
  await until("!document.querySelector('.page-speed-running') && !document.querySelector('button[title=\"Start crawl\"]').disabled", "Cancellation must work after leaving the PageSpeed tab and restore crawl controls");
  assert.ok(await evaluate("testCancelledPageSpeed === testPageSpeedRequest.requestId && !document.querySelector('[role=\"alert\"]')"), "Cancellation must target its own run and remain neutral feedback");
  const beforeLongPageSpeed = await evaluate("({ width: innerWidth, height: innerHeight })");
  await cdp('Emulation.setDeviceMetricsOverride', { width: 390, height: 600, deviceScaleFactor: 1, mobile: false });
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], finalUrl: 'https://example.test/?q=' + 'a'.repeat(8000) } }))");
  await click('#detail-tab-pagespeed');
  await click('[data-action="run-pagespeed"]');
  await until("document.querySelector('.page-speed-running span')?.getAttribute('title').length > 8000", "The running banner must retain the full URL for inspection");
  assert.ok(await evaluate("document.querySelector('.page-speed-running').getBoundingClientRect().height < 80 && document.querySelector('[data-action=\"cancel-pagespeed\"]').getBoundingClientRect().bottom < innerHeight && document.querySelector('.workspace').getBoundingClientRect().height > 150"), "Long URLs must not expand the running banner or hide cancellation and the workspace");
  await click('[data-action="cancel-pagespeed"]');
  await until("!document.querySelector('.page-speed-running')", "Long-URL measurements must remain cancellable");
  await cdp('Emulation.setDeviceMetricsOverride', { ...beforeLongPageSpeed, deviceScaleFactor: 1, mobile: false });
  await evaluate("testHoldPageSpeed = false; testEmit({ kind: 'started' })");
  await click('#detail-tab-pagespeed');
  assert.ok(await evaluate("document.querySelector('[data-action=\"run-pagespeed\"]').disabled"), "PageSpeed must not start while crawling");
  assert.ok(await evaluate("document.querySelector('[data-action=\"run-field-vitals\"]').disabled"), "Field data must not start while crawling");
  await evaluate("testEmit({ kind: 'finished' })");
  await until("document.querySelector('.field-vitals-panel')?.textContent.includes('No field data fetched yet')", "Rows without field data must show an actionable empty state");
  await select("Field data form factor", "desktop");
  await click('[data-action="run-field-vitals"]');
  await until("document.querySelector('.field-vitals-panel')?.textContent.includes('Desktop · Field data') && document.querySelector('.field-vitals-panel').textContent.includes('2,100 ms')", "Field data must render p75 metrics for the chosen form factor");
  assert.deepEqual(await evaluate("testFieldVitalsRequest"), { recordId: await evaluate("testFieldVitalsRequest.recordId"), formFactor: "desktop" }, "Field data requests must carry the selected form factor");
  assert.ok(await evaluate("document.querySelector('.field-vitals-panel').textContent.includes('2026-08-15 to 2026-09-11')"), "Field data must show its collection period");
  await evaluate("testFieldVitalsEmpty = true");
  await click('[data-action="run-field-vitals"]');
  await until("document.querySelector('.field-vitals-panel')?.textContent.includes('no field data for this URL')", "Missing CrUX records must be explained instead of showing blanks");
  await evaluate("testFieldVitalsEmpty = false; testFieldVitalsFailure = true");
  await click('[data-action="run-field-vitals"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('HTTP 403')", "Field data failures must stay visible");
  await click('[aria-label="Dismiss error"]');
  await evaluate("testFieldVitalsFailure = false");
  assert.ok(await evaluate("document.querySelector('[data-action=\"run-pagespeed-selected\"]').textContent.includes('1 selected')"), "Bulk PageSpeed must count the selected rows");
  await evaluate("[...document.querySelectorAll('.page-speed-categories input')].find((input) => input.nextSibling.textContent === 'Accessibility').click()");
  await selectGridRow(1, { ctrlKey: true });
  await until("!document.querySelector('[data-action=\"run-pagespeed-selected\"]').disabled", "Selecting rows must enable the bulk measurement");
  await click('[data-action="run-pagespeed-selected"]');
  await until("document.querySelector('.notice-bar')?.textContent.includes('PageSpeed bulk run: 1 measured, 1 already measured, 0 failed.')", "Bulk PageSpeed must summarize measured, skipped and failed rows");
  assert.deepEqual(await evaluate("({ count: testPageSpeedBulkRequest.recordIds.length, strategy: testPageSpeedBulkRequest.strategy, categories: testPageSpeedBulkRequest.categories, resume: testPageSpeedBulkRequest.resume })"),
    { count: 2, strategy: "desktop", categories: ["performance", "bestPractices", "seo"], resume: true }, "Bulk requests must carry the selection, device, chosen categories and resume flag");
  await evaluate("[...document.querySelectorAll('.page-speed-categories input')].find((input) => input.nextSibling.textContent === 'Accessibility').click()");
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("!document.querySelector('[data-action=\"run-pagespeed\"]').disabled", "PageSpeed must become available again after crawl completion");
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], pageSpeed: { ...rows[0].pageSpeed, performanceScore: 100, lcpMs: -1, completedAtMs: 9e18, fetchedAt: 'invalid-date' } } }))");
  await until("document.querySelector('.page-speed-scores dd')?.textContent === 'Not available' && document.querySelector('.page-speed-metrics dd')?.textContent === 'Not available'", "Malformed imported snapshots must not display impossible scores or negative lab metrics");
  assert.ok(await evaluate("!document.querySelector('.page-speed-panel .page-speed-result time')"), "Invalid imported timestamps must not crash or invent a measurement date");
  await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: rows[0] }))");
  await until("document.querySelector('.page-speed-scores dd')?.textContent === '92'", "The regular saved result must render after malformed evidence is replaced");
  const pageSpeedViewport = await evaluate("({ width: innerWidth, height: innerHeight })");
  const pageSpeedTheme = await evaluate("localStorage.getItem('ferrous-frog-theme') ?? 'system'");
  const pageSpeedPanels = await evaluate("({ issues: document.querySelector('.issue-sidebar').dataset.state === 'open', overview: document.querySelector('.overview-panel').dataset.state === 'open' })");
  for (const theme of ['dark', 'light']) {
    await menuItem(`${theme === 'dark' ? 'Dark' : 'Light'} Theme`);
    for (const width of [1280, 390]) {
      await cdp('Emulation.setDeviceMetricsOverride', { width, height: 800, deviceScaleFactor: 1, mobile: false });
      if (width === 390) {
        if (await evaluate("document.querySelector('.issue-sidebar').dataset.state === 'open'")) await click('[aria-label="Close audit views"]');
        if (await evaluate("document.querySelector('.overview-panel').dataset.state === 'open'")) await click('[aria-label="Close overview"]');
      }
      await until("document.querySelector('.page-speed-result')", "The saved PageSpeed result must remain visible in both themes");
      await until("document.getAnimations().every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity)", "The responsive sidebar transition must settle before measuring PageSpeed layout");
      assert.ok(await evaluate("document.documentElement.scrollWidth <= innerWidth && document.querySelector('.page-speed-panel').scrollWidth <= document.querySelector('.page-speed-panel').clientWidth + 1"), "PageSpeed controls and metrics must fit narrow screens without horizontal overflow");
      assert.ok(await evaluate("(() => { const button = document.querySelector('[data-action=\"run-pagespeed\"]'), rect = button.getBoundingClientRect(); return document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2)?.closest('button') === button; })()"), "The PageSpeed action must remain visible and reachable in narrow layouts");
      if (process.env.UI_SCREENSHOT) {
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.pagespeed-${width}-${theme}.png`, Buffer.from(shot.data, 'base64'));
      }
    }
  }
  await cdp('Emulation.setDeviceMetricsOverride', { ...pageSpeedViewport, deviceScaleFactor: 1, mobile: false });
  await menuItem(`${pageSpeedTheme[0].toUpperCase()}${pageSpeedTheme.slice(1)} Theme`);
  if (pageSpeedPanels.issues) await click('[aria-label="Toggle audit views"]');
  if (pageSpeedPanels.overview) await click('[aria-label="Toggle overview"]');
  await click('#detail-tab-page');
  await click('#inspection-tab-overview');
  assert.ok(await evaluate("!performance.getEntriesByType('resource').some((entry) => /\\/src\\/(CrawlGraph|graph-renderer)\\.tsx?/.test(entry.name))"), "Startup must defer the optional graph tool and renderer");
  await evaluate("window.testGraphFailure = true");
  await openGraph();
  await until("document.querySelector('.cg-empty[role=\"alert\"]')?.textContent.includes('Graph query failed')", "A failed initial graph query must offer recovery in the graph");
  await evaluate("window.testGraphFailure = false; document.querySelector('.cg-empty button').click()");
  await until("!document.querySelector('.cg-empty')", "Retrying the graph must load its snapshot");
  await until("performance.getEntriesByType('resource').some((entry) => entry.name.includes('/src/graph-renderer.ts'))", "Opening the graph may load its WebGL renderer");
  await until("document.querySelector('.graph-canvas canvas') || document.querySelector('.cg-render-message')", "WebGL must initialize or explain its SVG fallback");
  await useSvgGraph();
  await click('[data-tab="filters"]');
  await until("document.querySelector('[title=\"Show broken source-to-target edges with both endpoint nodes\"]')?.textContent.includes('(1)')", "Only the known connection failure should count as a broken graph edge");
  await click('[title="Show broken source-to-target edges with both endpoint nodes"]');
  await until("document.querySelectorAll('.graph-svg circle').length === 2", "Broken-link filtering must retain both endpoints and exclude unknown targets");
  await click('.cg-filter-heading button');
  await until("document.querySelectorAll('.graph-svg circle').length === 4", "Reset must restore the full graph snapshot");
  await click('[data-tab="browse"]');
  await fill('[aria-label="Search graph URLs"]', "root");
  await until("document.querySelector('.cg-navigation-heading')?.textContent.includes('1 matching')", "Graph search must find matching URLs");
  await evaluate("document.querySelector('[aria-label=\"Search graph URLs\"]').focus()");
  await pressKey("ArrowDown");
  await until("document.activeElement.matches('.cg-navigation-row')", "ArrowDown must move from graph search to navigation");
  await pressKey("ArrowDown");
  await until("document.activeElement.matches('.cg-navigation-row.is-url')", "The next graph navigation row must be the matching URL");
  await pressKey("Enter");
  await until("document.querySelector('.cg-selected-url')?.textContent.includes('/root')", "Graph URLs must be selectable entirely with the keyboard");
  await click('.cg-related [title="https://example.test/offline"]');
  await until("document.querySelector('.cg-detail .cg-status')?.textContent.includes('No response')", "Neighbor navigation must show the selected URL's real failure status");
  await click('[aria-label="Clear selected URL"]');
  await click('[aria-label="Clear graph search"]');
  await until("document.querySelector('.graph-svg circle')", "The crawl graph should render nodes");
  const graphCoordinates = await evaluate("[...document.querySelectorAll('.graph-svg circle')].map((node) => [node.getAttribute('cx'),node.getAttribute('cy')])");
  const darkNodeColor = await evaluate("document.querySelector('.graph-svg circle').getAttribute('fill')");
  await systemTheme("light");
  await until(`document.querySelector('.graph-svg circle')?.getAttribute('fill') !== ${JSON.stringify(darkNodeColor)}`, "An open graph must update its cached node colors when the system theme changes");
  await systemTheme("dark");
  await until(`document.querySelector('.graph-svg circle')?.getAttribute('fill') === ${JSON.stringify(darkNodeColor)}`, "Graph colors must also follow a switch back to dark");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.graph-svg circle')].map((node) => [node.getAttribute('cx'),node.getAttribute('cy')])"), graphCoordinates, "Theme changes must preserve graph geometry");
  await click('[aria-label="Fit graph to view"]');
  await delay(250);
  const graphWidth = await evaluate("document.querySelector('.graph-svg').viewBox.baseVal.width");
  await click('[aria-label="Zoom in"]');
  await until(`document.querySelector('.graph-svg').viewBox.baseVal.width < ${graphWidth * 0.76}`, "SVG zoom controls must change the camera");
  await delay(250);
  const zoomedCamera = await evaluate("document.querySelector('.graph-svg').getAttribute('viewBox')");
  await click('[title="Reload graph"]');
  await delay(250);
  assert.equal(await evaluate("document.querySelector('.graph-svg').getAttribute('viewBox')"), zoomedCamera, "Snapshot refresh must preserve the camera");
  await evaluate("window.testGraphFailure = true");
  await click('[title="Reload graph"]');
  await until("document.querySelector('.cg-feedback [role=\"alert\"]')?.textContent.includes('Graph query failed')", "Failed refreshes must show a recoverable query error");
  assert.ok(await evaluate("!document.querySelector('.cg-empty') && document.querySelectorAll('.graph-svg circle').length === 4"), "Failed refreshes must preserve the usable previous graph snapshot");
  await evaluate("window.testGraphFailure = false; document.querySelector('.cg-feedback .error-bar button').click()");
  await until("!document.querySelector('.cg-feedback [role=\"alert\"]') && !document.querySelector('[title=\"Reload graph\"]').disabled", "Successful retry must clear the graph error and restore reload");
  assert.equal(await evaluate("document.querySelector('.graph-svg').getAttribute('viewBox')"), zoomedCamera, "Graph error recovery must retain camera position");
  await evaluate("testGraphDelay = 2000; testGraphTotalNodes = 1234; testEmit({ kind: 'started' })");
  await until("document.querySelector('.cg-cap')?.textContent.includes('1,234')", "Slow live graph queries must complete without being discarded by subsequent polls");
  assert.equal(await evaluate("document.querySelector('.graph-svg').getAttribute('viewBox')"), zoomedCamera, "Slow live graph updates must preserve the current camera");
  await evaluate("testGraphDelay = 0; testGraphTotalNodes = 4; testEmit({ kind: 'finished' })");
  await until("!document.querySelector('.cg-cap')", "Crawl completion must refresh the final graph snapshot");
  await evaluate("[...document.querySelectorAll('.graph-actions button')].find((button) => button.textContent === 'Use WebGL').click()");
  await until("document.querySelector('.graph-canvas canvas') || document.querySelector('.cg-render-message')", "Switching renderers must initialize or return to SVG");
  await click('[data-tab="filters"]');
  await evaluate("const button = document.querySelector('[title=\"Open source-to-target rows for broken links\"]'); button.focus(); button.click()");
  await until("document.querySelector('.link-report-modal[data-state=\"open\"]')?.contains(document.activeElement)", "A graph report must receive focus above its parent graph");
  await click('[title="Close link reports"]');
  await until("!document.querySelector('.link-report-modal') && document.activeElement.title === 'Open source-to-target rows for broken links'", "Closing a graph report must return focus to its button inside the remaining graph");
  await captureScreenshot();
  await click('[title="Close graph"]');
  await until("document.querySelector('.crawl-graph-modal[data-state=\"closed\"]')", "The graph must retain its canvas during its exit");
  assert.ok(await evaluate("document.querySelector('.graph-canvas canvas') || document.querySelector('.graph-svg circle')"), "Closing the graph must not tear down the renderer before the exit finishes");
  await until("!document.querySelector('.crawl-graph-modal')", "Graph renderers must unmount after closing");
  await openTools();
  await evaluate("[...document.querySelectorAll('[role=\"menuitemradio\"]')].find((item) => item.textContent.includes('Light Theme')).focus()");
  await pressKey("Escape");
  await until("document.querySelector('.dropdown-content[data-state=\"closed\"]')?.inert", "Closing menus must disable keyboard input during their exit");
  await pressKey("Enter");
  assert.ok(await evaluate("document.documentElement.classList.contains('dark')"), "Enter must not activate a menu item after Escape has closed the menu");
  await evaluate("void (window.testClosingMenuNode = document.querySelector('.dropdown-content[data-state=\"closed\"]'))");
  await evaluate("document.querySelector('[aria-label=\"More tools\"]').focus()");
  await pressKey("Enter");
  await until("document.querySelector('.dropdown-content[data-state=\"open\"]')?.contains(document.activeElement)", "Rapid menu reopening must move keyboard focus into the retained menu");
  assert.ok(await evaluate("document.querySelector('.dropdown-content[data-state=\"open\"]') === window.testClosingMenuNode"), "Rapid menu reopening must retain the existing menu content");
  await pressKey("Escape");
  await until("!document.querySelector('[role=\"menu\"]')", "The closed menu must unmount");
  await menuItem("About");
  await until("document.querySelector('.about-modal[data-state=\"open\"]')?.contains(document.activeElement)", "A menu-launched dialog must receive focus");
  await click('[title="Close about"]');
  await until("!document.querySelector('.about-modal') && document.activeElement.matches('[aria-label=\"More tools\"]')", "Closing a menu-launched dialog must restore focus to its toolbar opener");
  await menuItem("Light Theme");
  await until("!document.documentElement.classList.contains('dark')", "Theme control should switch to light");
  await systemTheme("light");
  await systemTheme("dark");
  await delay(100);
  assert.ok(await evaluate("!document.documentElement.classList.contains('dark')"), "An explicit light choice must override system changes");
  assert.ok(await contrast('.issue-sidebar > p', '.issue-sidebar') >= 4.5, "Light theme secondary text must remain readable on panels");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.light.png`, Buffer.from(shot.data, "base64"));
  }
  for (const [width, height] of [[1280, 840], [920, 640]]) {
    await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    if (width === 920) {
      await click('[aria-label="Close overview"]');
      await click('[aria-label="Close audit views"]');
    }
    await delay(100);
    assert.ok(await evaluate("document.querySelector('.grid').getBoundingClientRect().height > 100 && document.querySelector('.detail-content').clientHeight > 50"), `Results and detail content must remain usable at ${width}x${height}`);
    assert.ok(await evaluate("document.querySelector('.url-control input').clientWidth > 200 && document.querySelector('[aria-label=\"Crawl scope\"]').clientWidth > 130"), "The scope selector must leave a readable URL field");
    if (width === 920) {
      await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: { ...rows[0], error: 'A long connection error with diagnostic context. '.repeat(40) } }))");
      await until("document.querySelector('.detail-error')", "The selected URL must show the test error");
      assert.ok(await evaluate("document.querySelector('.detail-content').clientHeight > 50"), "A long error must not consume the entire detail reading area");
      await evaluate("document.querySelector('.detail-content').scrollTop = document.querySelector('.detail-content').scrollHeight");
      assert.ok(await evaluate("(() => { const panel = document.querySelector('.detail-content').getBoundingClientRect(); const field = document.querySelector('#detail-panel-page dd:last-child').getBoundingClientRect(); return field.top >= panel.top && field.bottom <= panel.bottom; })()"), "Page fields must remain reachable below long errors");
      await evaluate("window.__TAURI_INTERNALS__.invoke('get_rows', { query: { offset: 0, limit: 1 } }).then(({ rows }) => testEmit({ kind: 'record', record: rows[0] }))");
      await until("!document.querySelector('.detail-error')", "The screenshot should return to the normal record");
      await evaluate("document.querySelector('.detail-content').scrollTop = 0");
    }
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.${width}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await click('[aria-label="Toggle overview"]');
  await cdp("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
  await delay(150);
  assert.ok(await evaluate("Boolean(document.querySelector('[aria-label=\"Close overview\"]'))"), "Overview overlay needs its own reachable close control");
  await click('[aria-label="Close overview"]');
  await click('[aria-label="Toggle audit views"]');
  await until("document.querySelector('.issue-sidebar')", "The audit tree must open on a small screen");
  await click('[aria-label="Close audit views"]');
  assert.ok(await evaluate("(() => { const grid = document.querySelector('.grid'); const r = grid.getBoundingClientRect(); return grid.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); })()"), "Panels must not cover the grid after closing");
  assert.ok(await evaluate("document.querySelector('.grid').getBoundingClientRect().height > 100"), "Small screens must leave room for crawl results");
  assert.ok(await evaluate("document.documentElement.scrollWidth <= innerWidth"), "Controls must not overflow the screen");
  assert.ok(await evaluate("document.querySelector('.url-control input').clientWidth > 200 && document.querySelector('[aria-label=\"Crawl scope\"]').clientWidth > 130"), "The seed and scope must remain readable on a small screen");
  for (const selector of ['.export-trigger', '.overview-toggle', '.settings-trigger']) {
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-label')`), "Icon-only toolbar actions must retain accessible names");
  }
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.mobile.png`, Buffer.from(shot.data, "base64"));
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  const beforeSnippetSettings = await savedSettings();
  const beforeSnippetResults = await evaluate("document.querySelector('.data-table tbody').textContent");
  await openMode();
  await click('[data-action="serp-preview"]');
  await until("document.querySelector('.serp-modal')", "Mode must open the offline snippet editor");
  await fill('[aria-label="Snippet title"]', "Draft title 🦀");
  await fill('[aria-label="Snippet description"]', "<img src=x onerror=alert(1)> Plain text preview");
  await until("document.querySelector('.serp-metrics')?.textContent.includes('13 characters')", "Snippet metrics must display the latest native measurement");
  assert.equal(await evaluate("document.querySelectorAll('.serp-preview img').length"), 0, "Imported or edited metadata must render as text");
  await evaluate("window.testSnippetDelays = { 'Older title': 700 }");
  await fill('[aria-label="Snippet title"]', "Older title");
  await delay(200);
  await fill('[aria-label="Snippet title"]', "New");
  await until("document.querySelector('.serp-metrics')?.textContent.includes('3 characters')", "New edits must win over slower measurements");
  await delay(700);
  assert.ok(await evaluate("document.querySelector('.serp-metrics').textContent.includes('3 characters')"), "Stale measurements must not replace the latest text");
  await fill('[aria-label="Snippet URL"]', "javascript:alert(1)");
  await until("document.querySelector('.serp-modal [role=\"alert\"]')?.textContent.includes('HTTP')", "Invalid preview URLs must show an actionable error");
  const snippetCsv = 'url,title,description\nhttps://imported.test/one,"Imported, ""title""",İstanbul 🦀\nhttps://imported.test/two,Second,Second description';
  await importSnippets(snippetCsv);
  await until("document.querySelector('[aria-label=\"Snippet title\"]').value === 'Imported, \"title\"'", "CSV import must open the imported snippets");
  assert.equal(await evaluate("testSnippetImportText"), snippetCsv, "File import must pass exact CSV text to the native parser");
  await click('[aria-label="Next snippet"]');
  await fill('[aria-label="Snippet title"]', "Second edited");
  await until("document.querySelector('.serp-metrics').textContent.includes('13 characters')", "Each imported row must remain editable");
  await evaluate("[...document.querySelectorAll('.serp-actions button')].find((button) => button.textContent === 'Export CSV').click()");
  await until("testExportedSnippets?.[1].title === 'Second edited'", "Export must include changes to every draft row");
  assert.equal(await evaluate("testExportedSnippets[0].description"), "İstanbul 🦀", "Editing one row must preserve all other imported metadata");
  await evaluate("window.testSnippetImportFailure = true");
  await importSnippets("bad CSV");
  await until("document.querySelector('.serp-modal [role=\"alert\"]')?.textContent.includes('CSV requires')", "Invalid CSV must show a local import error");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Snippet title\"]').value"), "Second edited", "Failed imports must preserve existing drafts");
  await evaluate("window.testSnippetImportFailure = false");
  for (const theme of ["dark", "light"]) {
    await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
    assert.ok(await contrast('.serp-title', '.serp-preview') >= 4.5, "Snippet titles must be readable in both themes");
    assert.ok(await contrast('[aria-label="Snippet description"]') >= 4.5, "Snippet editor text must be readable in both themes");
    assert.equal(await evaluate("getComputedStyle(document.querySelector('[aria-label=\"Snippet description\"]')).backgroundColor"),
      await evaluate("getComputedStyle(document.querySelector('[aria-label=\"Snippet title\"]')).backgroundColor"), "All snippet fields must follow the selected theme");
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.serp-${theme}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 390, height: 640, deviceScaleFactor: 1, mobile: false });
  assert.ok(await evaluate("document.querySelector('.serp-content').scrollWidth <= document.querySelector('.serp-content').clientWidth"), "The snippet editor must fit narrow windows");
  await click('[title="Close SERP preview"]');
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await openMode();
  await click('[data-action="serp-preview"]');
  await until("document.querySelector('.serp-modal[data-state=\"open\"] [aria-label=\"Snippet title\"]')?.value === 'Second edited'", "Closing and reopening must retain draft edits for the current session");
  await until("document.querySelector('.serp-modal').contains(document.activeElement) && !document.querySelector('[role=\"menu\"]')", "Reopened dialogs must receive focus without a closing menu intercepting keyboard input");
  await pressKey("Escape");
  await until("!document.querySelector('.serp-modal')", "Escape must close the snippet editor");
  assert.deepEqual(await savedSettings(), beforeSnippetSettings, "Snippet drafts must not change crawl settings");
  assert.equal(await evaluate("document.querySelector('.data-table tbody').textContent"), beforeSnippetResults, "Snippet editing must not replace crawl results");
  await chooseMode("spider");
  const beforeListImport = await savedSettings();
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Scope");
  await fill('[aria-label="List URLs"]', 'https://example.test/a\n');
  const listImportPath = join(profile, 'url-list.txt');
  await writeFile(listImportPath, 'https://example.test/a\nhttps://example.test/b\nhttps://example.test/a\n');
  const listDocument = await cdp('DOM.getDocument');
  const listInput = await cdp('DOM.querySelector', { nodeId: listDocument.root.nodeId, selector: '[aria-label="Import URL file"]' });
  await cdp('DOM.setFileInputFiles', { nodeId: listInput.nodeId, files: [listImportPath] });
  const importedList = ['https://example.test/a', 'https://example.test/a', 'https://example.test/b', 'https://example.test/a'];
  await until(`document.querySelector('[aria-label="List URLs"]').value === ${JSON.stringify(importedList.join('\n'))}`, 'List file import must append every occurrence in order, including duplicates of existing entries');
  assert.deepEqual(await savedSettings(), beforeListImport, 'File import must stay in the Settings draft until Apply');
  await applySettings();
  assert.deepEqual((await savedSettings()).config.listUrls, importedList, 'Applied file imports must preserve duplicate occurrences in saved settings');
  await click('[data-action="cancel-settings"]');
  await until("!document.querySelector('.settings-modal')", 'Applied list import must close normally');
  await chooseMode("spider");
  const beforeCrawlControls = await savedSettings();
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Limits");
  await markSetting("Max response MiB");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max response MiB"))}).value`), "20", "HTTP responses must have a visible default download limit");
  await fill(setting("Max response MiB"), "0");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-modal [role=\"alert\"]')", "Zero response size must fail validation");
  assert.deepEqual(await savedSettings(), beforeCrawlControls, "Invalid response limits must preserve saved settings");
  await fill(setting("Max response MiB"), "32");
  await settingsTab("Sitemaps");
  await toggleSetting("Crawl XML sitemaps");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Spider sitemap URLs\"]').disabled && [...document.querySelectorAll('[data-settings-section=\"sitemaps\"] [role=\"checkbox\"]')].slice(1).every((item) => item.disabled)"), "The master sitemap option must disable dependent inputs");
  await toggleSetting("Crawl XML sitemaps");
  await toggleSetting("Use sitemaps from robots.txt");
  await toggleSetting("Probe /sitemap.xml");
  await toggleSetting("Follow linked sitemaps");
  await fill('[aria-label="Spider sitemap URLs"]', "https://example.test/map.xml");
  await evaluate("(() => { const input = document.querySelector('[aria-label=\"Spider sitemap URLs\"]'); input.focus(); input.setSelectionRange(input.value.length, input.value.length); })()");
  await cdp("Input.insertText", { text: "\n" });
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Spider sitemap URLs\"]').value.endsWith('\\n')"), "Typing Enter must retain the newline for a second sitemap source");
  await cdp("Input.insertText", { text: "https://example.test/catalog.xml" });
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Spider sitemap URLs\"]').value"), "https://example.test/map.xml\nhttps://example.test/catalog.xml", "Sitemap URLs must be editable line by line");
  await observeDialogExit('.settings-modal', `
      window.testClosingSettingsNode = dialog;
      window.testSettingsExit = { state: dialog.dataset.state, inert: dialog.inert,
        value: dialog.querySelector('[aria-label="Spider sitemap URLs"]').value };
      const trigger = document.querySelector('[aria-label="Crawl settings"]');
      trigger.focus(); trigger.click();
  `);
  await click('[data-action="cancel-settings"]');
  await until("window.testSettingsExit?.state === 'closed' && window.testSettingsExit.inert", "Settings must animate closing before unmounting and become inert");
  assert.equal(await evaluate("window.testSettingsExit.value"), "https://example.test/map.xml\nhttps://example.test/catalog.xml", "Settings must preserve its draft content through the exit animation");
  assert.deepEqual(await savedSettings(), beforeCrawlControls, "Cancel must discard response and nested sitemap edits");
  await until("document.querySelector('.settings-modal[data-state=\"open\"]')?.contains(document.activeElement)", "Reopening Settings during its exit must restore focus inside the retained dialog");
  assert.ok(await evaluate("document.querySelector('.settings-modal') === window.testClosingSettingsNode"), "Rapid reopening must retain the existing dialog content");
  await settingsTab("Limits");
  await markSetting("Max response MiB");
  await fill(setting("Max response MiB"), "32");
  await settingsTab("Sitemaps");
  await toggleSetting("Probe /sitemap.xml");
  await fill('[aria-label="Spider sitemap URLs"]', "file:///private-map.xml");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-modal [role=\"alert\"]')?.textContent.includes('HTTP or HTTPS')", "Invalid sitemap sources must show native validation errors");
  assert.deepEqual(await savedSettings(), beforeCrawlControls, "Invalid sitemap sources must not save any draft field");
  await fill('[aria-label="Spider sitemap URLs"]', "https://example.test/map.xml\n\nhttps://example.test/catalog.xml");
  await applySettings();
  const crawlControls = await savedSettings();
  assert.equal(crawlControls.config.maxResponseBytes, 32 * 1024 * 1024, "The MiB control must pass bytes to the engine");
  assert.deepEqual(crawlControls.config.sitemap, { enabled: true, discoverFromRobots: true, probeDefault: false, followLinked: true,
    urls: ["https://example.test/map.xml", "https://example.test/catalog.xml"] }, "Sitemap choices and trimmed explicit sources must save together");
  await click('[title="Close settings"]');
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await markSetting("Max response MiB");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max response MiB"))}).value`), "32", "The response limit must survive reopening");
  await settingsTab("Sitemaps");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Spider sitemap URLs\"]').value"), "https://example.test/map.xml\nhttps://example.test/catalog.xml", "Sitemap sources must survive reopening");
  await settingsTab('Resources');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(2) [role=\"checkbox\"]')].every((item) => item.getAttribute('aria-checked') === 'false')"), 'Reference discovery must preserve legacy off defaults');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(3) [role=\"checkbox\"]')].every((item) => item.getAttribute('aria-checked') === 'true' && !item.disabled)"), 'Reference Store choices must default to enabled without requesting targets');
  const referenceStoreBefore = await savedSettings();
  const referenceStoreLabels = ['Canonical targets', 'Hreflang targets', 'Pagination (next / previous)', 'AMP targets', 'Meta refresh targets', 'Iframe sources'];
  for (const label of referenceStoreLabels) await toggleSetting(`Store ${label}`);
  assert.deepEqual(await savedSettings(), referenceStoreBefore, 'Reference retention changes must remain pending until Apply');
  await toggleSetting('Crawl Canonical targets');
  await toggleSetting('Crawl Hreflang targets');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery tbody tr')].slice(0, 2).every((row) => { const store = row.querySelector('td:nth-child(3) [role=\"checkbox\"]'); return store.disabled && store.getAttribute('aria-checked') === 'true'; })"), 'Crawled reference types must visibly force Store on');
  await applySettings();
  assert.deepEqual((await savedSettings()).config.referenceLinks, { canonical: true, hreflang: true, pagination: false, amp: false, metaRefresh: false, iframe: false }, 'Reference types must save independently');
  for (const kind of ['canonical', 'hreflang', 'pagination', 'amp', 'metaRefresh', 'iframe']) {
    assert.equal((await savedSettings()).config.store[kind], false, `The explicit ${kind} Store preference must save independently of effective Crawl forcing`);
  }
  await toggleSetting('Crawl Canonical targets');
  assert.ok(await evaluate("document.querySelector('.reference-discovery tbody tr td:nth-child(3) [role=\"checkbox\"]').getAttribute('aria-checked') === 'false' && !document.querySelector('.reference-discovery tbody tr td:nth-child(3) [role=\"checkbox\"]').disabled"), 'Turning Crawl off must restore the explicit Store choice');
  await toggleSetting('Crawl Canonical targets');
  await settingsTab('Thresholds');
  await markSetting("Title maximum");
  await fill(setting("Title maximum"), "50");
  await applySettings();
  assert.equal((await savedSettings()).config.thresholds.titleMaxChars, 50, 'Thresholds must save with the configuration');
  await until("window.testAuditThresholds?.titleMaxChars === 50", 'Applied thresholds must reach the engine before rows reload');
  await click('.settings-section:not([hidden]) .settings-action-button');
  await until(`document.querySelector(${JSON.stringify(setting("Title maximum"))}).value === '60'`, 'Reset must restore default thresholds in the draft');
  await applySettings();
  await settingsTab('HTTP headers');
  await fill('[aria-label="HTTP auth username"]', "frog");
  await fill('[aria-label="HTTP auth password"]', "fixture-secret");
  await click('[data-action="save-http-auth"]');
  await until("document.querySelector('.http-auth-settings')?.textContent.includes('Credentials saved for frog') && document.querySelector('[aria-label=\"HTTP auth password\"]').value === ''", 'Saving credentials must report the username and clear the password draft');
  assert.deepEqual(await evaluate("window.testHttpAuth"), { username: "frog", password: "fixture-secret" }, 'Credentials must go to the OS store command, not the configuration');
  await toggleSetting("Send saved credentials to the starting origin");
  await applySettings();
  assert.deepEqual((await savedSettings()).config.httpAuth, { enabled: true }, 'Only the enabled flag may persist with the configuration');
  assert.ok(!JSON.stringify(await savedSettings()).includes('fixture-secret'), 'Saved settings must never contain the password');
  await click('[data-action="clear-http-auth"]');
  await until("document.querySelector('.http-auth-settings.http-auth-settings')?.textContent.includes('No saved credentials')", 'Clearing credentials must update the status');
  await fill('[aria-label="Form login username"]', "member");
  await fill('[aria-label="Form login password"]', "form-secret");
  await click('[data-action="save-form-login"]');
  await until("document.querySelector('.form-login-settings')?.textContent.includes('Credentials saved for member')", 'Form login credentials must report the username');
  assert.deepEqual(await evaluate("window.testFormLogin"), { username: "member", password: "form-secret" }, 'Form credentials must go to the OS store command');
  await toggleSetting("Log in with the saved form credentials before crawling");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-validation-error')?.textContent.includes('highlighted')", 'Enabling form login without a URL must be rejected natively');
  await fill('[aria-label="Form login URL"]', "https://example.test/login");
  await fill('[aria-label="Form login extra fields"]', "remember=1\n\nnext=/account");
  await applySettings();
  assert.deepEqual((await savedSettings()).config.formLogin, { enabled: true, url: "https://example.test/login", usernameField: "username", passwordField: "password", extraFields: [{ name: "remember", value: "1" }, { name: "next", value: "/account" }] }, 'Form login settings must persist without secrets');
  assert.ok(!JSON.stringify(await savedSettings()).includes('form-secret'), 'Saved settings must never contain the form password');
  await toggleSetting("Log in with the saved form credentials before crawling");
  await applySettings();
  await settingsTab('Automation');
  await select("Automatic export preset", "audit");
  await fill('[aria-label="Completion webhook URL"]', "https://hooks.example.test/crawl");
  await toggleSetting("Desktop notification on completion");
  await applySettings();
  assert.deepEqual((await savedSettings()).config.automation, { exportPreset: "audit", webhookUrl: "https://hooks.example.test/crawl", notifyOnCompletion: true }, 'Automation choices must save with the configuration');
  await fill('[aria-label="Completion webhook URL"]', "not a url");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-validation-error')?.textContent.includes('highlighted')", 'Invalid webhook URLs must be rejected natively');
  await fill('[aria-label="Completion webhook URL"]', "");
  await applySettings();
  await select("Crawl schedule", "once");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-validation-error')?.textContent.includes('highlighted')", 'A one-off schedule without a time must be rejected natively');
  await fill('[aria-label="Scheduled run time"]', "2099-01-01T09:30");
  await applySettings();
  assert.deepEqual((await savedSettings()).config.schedule, { mode: "once", runAt: "2099-01-01T09:30", intervalMinutes: 60 }, 'Schedules must save with the configuration');
  await until("document.querySelector('[data-schedule-status]')?.textContent.startsWith('Next scheduled crawl:')", 'Applied schedules must show the next run');
  const startCallsBeforeSchedule = await evaluate("testStartCalls");
  await select("Crawl schedule", "none");
  await applySettings();
  await until("document.querySelector('[data-schedule-status]')?.textContent === 'No scheduled crawl.'", 'Turning the schedule off must clear the next run');
  assert.equal(await evaluate("testStartCalls"), startCallsBeforeSchedule, 'A future schedule must not start a crawl');
  await click('[title="Close settings"]');
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await settingsTab('Resources');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery .checkbox-field')].find((field) => field.textContent.trim() === 'Crawl Canonical targets').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), 'Reference discovery must survive reopening');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery .checkbox-field')].find((field) => field.textContent.trim() === 'Store Meta refresh targets').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'false'"), 'Explicit reference Store choices must survive reopening');
  await settingsTab('Sitemaps');
  if (process.env.UI_SCREENSHOT) {
    for (const [width, height] of [[1280, 840], [390, 640]]) {
      await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      for (const theme of ["dark", "light"]) {
        await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.sitemaps-${theme}-${width}.png`, Buffer.from(shot.data, "base64"));
      }
    }
    await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
    await evaluate("document.documentElement.classList.add('dark')");
  }
  await settingsTab("Scope");
  await select("Folder scope", "exactUrl");
  await settingsTab("Sitemaps");
  assert.ok(await evaluate("[...document.querySelectorAll('[data-settings-section=\"sitemaps\"] [role=\"checkbox\"], [aria-label=\"Spider sitemap URLs\"]')].every((item) => item.disabled)"), "Exact URL scope must disable all sitemap discovery");
  await settingsTab('Resources');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(2) [role=\"checkbox\"]')].every((item) => item.disabled)"), 'Exact URL scope must disable reference discovery without clearing its preferences');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(3) [role=\"checkbox\"]')].every((item) => !item.disabled && item.getAttribute('aria-checked') === 'false')"), 'Exact URL must allow independent retention even with saved Spider Crawl choices');
  await toggleSetting('Store Canonical targets');
  await click('[data-action="cancel-settings"]');
  await chooseMode("list");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Sitemaps");
  assert.ok(await evaluate("[...document.querySelectorAll('[data-settings-section=\"sitemaps\"] [role=\"checkbox\"], [aria-label=\"Spider sitemap URLs\"]')].every((item) => item.disabled)"), "List mode must use its own sitemap sources");
  await settingsTab('Resources');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(2) [role=\"checkbox\"]')].every((item) => item.disabled)"), 'List mode must keep reference discovery disabled');
  assert.ok(await evaluate("[...document.querySelectorAll('.reference-discovery td:nth-child(3) [role=\"checkbox\"]')].every((item) => !item.disabled && item.getAttribute('aria-checked') === 'false')"), 'List mode must preserve all Store choices independently of disabled Crawl preferences');
  await toggleSetting('Store Iframe sources');
  await click('[data-action="cancel-settings"]');
  assert.equal((await savedSettings()).config.store.iframe, false, 'Cancel must discard pending reference Store choices');
  await chooseMode("spider");
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Settings must open");
  await settingsTab("Limits");
  const beforeDraft = await savedSettings();
  for (const [label, value] of [["Threads", "8"], ["RPS", "10"], ["Delay ms", "100"]]) {
    await markSetting(label);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting(label))}).value`), value, "Fresh settings must expose the faster default speed");
  }
  await markSetting("Threads");
  await fill(setting("Threads"), "17");
  assert.deepEqual(await savedSettings(), beforeDraft, "Editing a draft must not save or change active crawl options");
  await click('[data-action="cancel-settings"]');
  await click('[aria-label="Crawl settings"]');
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), String(beforeDraft?.config.concurrency ?? 8), "Cancel must restore the last applied settings");
  await fill(setting("Threads"), "0");
  await settingsTab("Query");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-tab-button[aria-current=\"page\"]')?.textContent.trim() === 'Speed' && document.activeElement.type === 'number'", "Invalid values in hidden sections must reveal and focus their control");
  assert.deepEqual(await savedSettings(), beforeDraft, "Invalid numeric settings must not be saved");
  await fill(setting("Threads"), "18");
  await evaluate("window.testConfigurationValidationFailure = true");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-validation-error')?.textContent.includes('invalid include')", "Native rule validation errors must leave the draft open");
  assert.deepEqual(await savedSettings(), beforeDraft, "Native validation failures must preserve applied settings");
  await evaluate("window.testConfigurationValidationFailure = false; window.testHoldConfigurationValidation = true");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('[data-action=\"apply-settings\"]')?.textContent.includes('Applying')", "Validation must indicate that Apply is pending");
  await pressKey("Escape");
  await until("!document.querySelector('.settings-modal')", "Pending Settings validation must remain cancellable");
  await evaluate("window.testHoldConfigurationValidation = false; testFinishConfigurationValidation()");
  await delay(50);
  assert.deepEqual(await savedSettings(), beforeDraft, "Dismissal must cancel pending validation before it can save");
  await click('[aria-label="Crawl settings"]');
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), String(beforeDraft?.config.concurrency ?? 8), "Escape must discard the cancelled draft");
  await fill(setting("Threads"), "19");
  await click('[data-action="ok-settings"]');
  await until("!document.querySelector('.settings-modal')", "OK must apply and close configuration");
  assert.equal((await savedSettings()).config.concurrency, 19, "OK must save the validated draft");
  await click('[aria-label="Crawl settings"]');
  await markSetting("Threads");
  await fill(setting("Threads"), String(beforeDraft?.config.concurrency ?? 8));
  await applySettings();
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Storage').click()");
  await fill('[placeholder="/path/to/ferrous-frog.sqlite3"]', "/invalid/crawl.sqlite3");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Open Database').click()");
  await until("document.querySelector('.settings-modal [role=\"alert\"]')?.textContent.includes('Cannot open this database')", "A failed dialog action must show its error inside the active dialog");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.settings.png`, Buffer.from(shot.data, "base64"));
  }
  await click('.settings-modal [aria-label="Dismiss error"]');
  await fill('[placeholder="/path/to/ferrous-frog.sqlite3"]', "/tmp/test-crawl.sqlite3");
  await evaluate("window.testHoldWorkspace = true");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Open Database').click()");
  await until("window.testFinishWorkspace", "The database operation must reach the native command");
  assert.ok(await evaluate("document.querySelector('.settings-content').disabled && document.querySelector('[data-action=\"ok-settings\"]').disabled"), "A pending workspace change must lock draft editing and Apply/OK");
  await settingsTab("Limits");
  await markSetting("Threads");
  await fill(setting("Threads"), "99");
  assert.ok(await evaluate("document.querySelector('[data-action=\"apply-settings\"]').disabled"), "Late input events must not dirty a locked draft");
  await click('[title="Close settings"]');
  assert.ok(await evaluate("document.querySelector('[title=\"Start crawl\"]').disabled"), "Starting a crawl must wait for the workspace operation");
  await click('[aria-label="Crawl settings"]');
  assert.ok(await evaluate("document.querySelector('.settings-content').disabled"), "Reopening Settings must retain the pending workspace lock");
  await settingsTab("Storage");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Open Database').click()");
  assert.equal(await evaluate("testWorkspaceCalls"), 2, "Competing database actions must not start while one is pending");
  await evaluate("window.testHoldWorkspace = false; window.testFinishWorkspace()");
  await until("testEmptyDataset && document.querySelector('.detail-empty') && !document.querySelector('.detail-header')", "Opening a different crawl must clear the previous URL selection while retaining the inspector layout");
  await until("!document.querySelector('.settings-content').disabled", "Finishing the workspace action must unlock Settings");
  assert.equal(await evaluate("document.querySelector('.settings-section:not([hidden]) input[readonly]').value"), "SQLite", "The clean draft must follow the newly opened database");
  await settingsTab("Limits");
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), String(beforeDraft?.config.concurrency ?? 8), "A pending database operation must preserve the last applied threads value");
  await fill(setting("Threads"), "5");
  await applySettings();
  assert.equal((await savedSettings()).storageMode, "database", "Applying an unrelated change must not switch the opened workspace back to Memory");
  await click('[title="Close settings"]');
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').value"), "includeSubdomains", "Existing host/descendant defaults must not silently become all-subdomain crawls");
  for (const [preset, host, folder] of [["exactHost", "exactHost", "anywhere"], ["startFolder", "exactHost", "startFolder"], ["allSubdomains", "allSubdomains", "anywhere"], ["exactUrl", "exactHost", "exactUrl"]]) {
    await select("Crawl scope", preset);
    const config = (await savedSettings()).config;
    assert.deepEqual([config.subdomainScope, config.folderScope], [host, folder], "A toolbar scope must persist both engine rules together");
  }
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Scope");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Folder scope\"]').value"), "exactUrl", "Settings must reflect the toolbar scope");
  assert.ok(await evaluate("[...document.querySelectorAll('.checkbox-field')].find((item) => item.textContent.trim() === 'Check links outside start folder').querySelector('[role=\"checkbox\"]').disabled"), "Exact URL scope must not offer outside-folder checks");
  await select("Folder scope", "startFolder");
  await toggleSetting("Check links outside start folder");
  await applySettings();
  assert.equal((await savedSettings()).config.checkLinksOutsideStartFolder, true, "Outside-folder checking must save separately from recursive scope");
  await select("Folder scope", "exactUrl");
  await applySettings();
  await toggleSetting("Follow internal nofollow links");
  await select("Folder scope", "exactFolder");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').value"), "exactUrl", "Unapplied scope edits must leave the active toolbar unchanged");
  await applySettings();
  const followChoices = (await savedSettings()).config;
  assert.equal(followChoices.followInternalNofollow, false, "Internal nofollow discovery must save independently");
  assert.equal(followChoices.followExternalNofollow, true, "Changing internal nofollow must preserve the external preference");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').value"), "custom", "Applied advanced combinations must not be mislabelled as a preset");
  await select("Subdomain scope", "allSubdomains");
  await select("Folder scope", "anywhere");
  await applySettings();
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').value"), "allSubdomains", "Scope changes in Settings must update the toolbar");
  await click('[title="Close settings"]');
  await reloadApp();
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').value"), "allSubdomains", "A new subdomain scope must survive restarting the app");
  await select("Crawl scope", "exactUrl");
  await fill('[aria-label="Seed URL"]', "https://spider-input.test/start");
  await chooseMode("list");
  await fill('[aria-label="Root URL"]', "https://list-input.test/root");
  await chooseMode("spider");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Seed URL\"]').value"), "https://spider-input.test/start", "Switching modes must retain the Spider seed");
  await reloadApp();
  await chooseMode("list");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Root URL\"]').value"), "https://list-input.test/root", "Each mode's input must survive reopening");
  await chooseMode("spider");
  await openMode();
  await click('[data-action="compare-crawls"]');
  await until("document.querySelector('.comparison-modal')", "Mode must directly open the existing comparison workflow");
  assert.equal(await evaluate("document.querySelector('.comparison-options [role=\"checkbox\"]').getAttribute('aria-checked')"), 'false', "Opening an archive comparison must reset the response-only option");
  await fill('[aria-label="Baseline crawl archive"]', '/tmp/baseline.ffcrawl.json');
  await click('.comparison-controls button');
  await until("testComparisonRequests.at(-1).command === 'open_crawl_comparison' && document.querySelector('.comparison-table')?.textContent.includes('New description')", "Archive comparisons must use the same prepared result view");
  assert.equal(await evaluate("testComparisonRequests.at(-1).archivePath"), '/tmp/baseline.ffcrawl.json', "Archive preparation must receive its path inside the native request");
  await evaluate("testComparisonOnlyResponses = true");
  await click('.comparison-controls button');
  await until("document.querySelector('.comparison-limit')?.textContent.includes('No matching changes')", "A fresh archive snapshot must hide raw-only changes by default");
  const archivePreparations = await evaluate("testComparisonRequests.filter((request) => request.command === 'open_crawl_comparison').length");
  await click('.comparison-options [role="checkbox"]');
  await until("testComparisonRequests.at(-1).includeResponseOnly && document.querySelector('.comparison-table')?.textContent.includes('Response only') && document.querySelector('.comparison-limit')?.textContent.includes('of 205 matching changes')", "Response-only archives must page through every matching row");
  assert.ok(await evaluate("document.querySelector('.comparison-options').textContent.includes('Older or incompatible captures') && [...document.querySelectorAll('.comparison-summary .metric')].find((metric) => metric.querySelector('span').textContent === 'Content unavailable').querySelector('strong').textContent === '205'"), "Legacy comparisons must disclose unavailable text and the need for matching new captures");
  await click('.comparison-options [role="checkbox"]');
  await until("testComparisonRequests.at(-1).includeResponseOnly === false && document.querySelectorAll('.comparison-table [data-comparison-key]').length === 0 && document.querySelector('.comparison-limit')?.textContent.includes('No matching changes')", "Disabling raw-only rows must request an empty filtered result");
  assert.equal(await evaluate("testComparisonRequests.filter((request) => request.command === 'open_crawl_comparison').length"), archivePreparations, "Archive filters must reuse the prepared snapshot");
  await evaluate("testComparisonOnlyResponses = false");
  await click('[title="Close crawl comparison"]');
  await menuItem("Audit reports");
  await until("document.querySelector('.audit-report-modal')", "Audit reports must open from Tools");
  await fill('[aria-label="Audit report title"]', 'Saved deterministic audit');
  await click('.audit-report-launcher .primary');
  await until("document.querySelector('.audit-report-summary')?.textContent.includes('1,205') && document.querySelector('[data-audit-finding-id=\"title.missing\"]')", "Preparing a saved crawl must publish frozen findings and coverage");
  await until("document.querySelector('.audit-findings + .audit-evidence-pagination')?.textContent.includes('1–100 of 205')", "Findings must use a bounded first page");
  await click('.audit-findings + .audit-evidence-pagination button:last-child');
  await until("document.querySelector('.audit-findings + .audit-evidence-pagination')?.textContent.includes('101–200 of 205')", "Findings paging must replace the previous page");
  await click('.audit-findings + .audit-evidence-pagination button:first-of-type');
  await until("document.querySelector('[data-audit-finding-id=\"title.missing\"]')", "Findings paging must return to the first page");
  const findingQueriesBeforeSearch = await evaluate("testAuditRequests.filter((request) => request.command === 'query_audit_report_findings').length");
  await fill('[aria-label="Search audit findings"]', 'Missing');
  await delay(100);
  assert.equal(await evaluate("testAuditRequests.filter((request) => request.command === 'query_audit_report_findings').length"), findingQueriesBeforeSearch, "Finding search must wait for its debounce window");
  await until("testAuditRequests.at(-1).search === 'Missing'", "Finding search must reach the server after its debounce window");
  await fill('[aria-label="Audit finding category"]', 'Titles');
  await until("testAuditRequests.at(-1).category === 'Titles'", "Finding category must reach the server after its debounce window");
  await click('[data-audit-finding-id="title.missing"]');
  await until("document.querySelector('.audit-evidence .audit-evidence-pagination')?.textContent.includes('1–100 of 1,205')", "Evidence must use a bounded first page");
  for (let page = 1; page <= 12; page++) {
    await click('.audit-evidence .audit-evidence-pagination button:last-child');
    await until(`document.querySelector('.audit-evidence .audit-evidence-pagination')?.textContent.includes(${JSON.stringify((page * 100 + 1).toLocaleString())})`, `Evidence page ${page + 1} must replace the previous page`);
  }
  await until("document.querySelector('.audit-evidence .audit-evidence-pagination')?.textContent.includes('1,201–1,205') && document.querySelector('[data-audit-evidence-id=\"evidence-1205\"]')", "Evidence paging must reach record 1,205");
  const evidenceQueriesBeforeSearch = await evaluate("testAuditRequests.filter((request) => request.command === 'query_audit_report_evidence').length");
  await fill('[aria-label="Search audit evidence"]', 'report-1205');
  await delay(100);
  assert.equal(await evaluate("testAuditRequests.filter((request) => request.command === 'query_audit_report_evidence').length"), evidenceQueriesBeforeSearch, "Evidence search must wait for its debounce window");
  await until("document.querySelector('.audit-evidence .audit-evidence-pagination')?.textContent.includes('1–1 of 1')", "Evidence search must find an off-page record");
  await fill('[aria-label="Evidence HTTP status"]', '404');
  await until("testAuditRequests.at(-1).statusCode === 404 && document.querySelector('.audit-evidence .audit-evidence-pagination')?.textContent.includes('1–1 of 1')", "Evidence HTTP status must reach the server with the matching evidence query");
  await evaluate("document.querySelector('[data-audit-evidence-id=\"evidence-1205\"]').focus()");
  await pressKey("Enter");
  await until("document.querySelector('.audit-evidence-detail')?.textContent.includes('report-1205')", "Keyboard evidence selection must open its captured detail");
  await click('.audit-evidence-detail button');
  assert.equal(await evaluate("testCopiedText"), "https://example.test/report-1205", "Evidence copy must use the full original URL");
  await click('[aria-label="Export all matching evidence (CSV)"]');
  await until("document.querySelector('.audit-evidence')?.textContent.includes('/tmp/ferrous-audit-report/title-missing.csv')", "Evidence export must disclose its CSV artifact path");
  assert.deepEqual(await evaluate("(({ command, findingId, search, statusCode, sortBy, sortDir, offset, limit, preview }) => ({ command, findingId, search, statusCode, sortBy, sortDir, offset, limit, preview }))(testAuditRequests.at(-1))"), { command: "export_audit_report_evidence", findingId: "title.missing", search: "report-1205", statusCode: 404, sortBy: "originalUrl", sortDir: "asc", offset: 0, limit: 100, preview: true }, "Evidence export must preserve the active matching status and search query for native full-result export");
  await evaluate("testHoldAuditEvidenceExport = true");
  await click('[aria-label="Export all matching evidence (CSV)"]');
  await until("[...document.querySelectorAll('.audit-report-launcher button')].some((button) => button.textContent.includes('Cancel preparation'))", "An in-flight evidence export must be cancellable");
  await evaluate("[...document.querySelectorAll('.audit-report-launcher button')].find((button) => button.textContent.includes('Cancel preparation')).click()");
  await until("testAuditCancels.length > 0", "Cancelling evidence export must call the shared native cancellation command");
  await evaluate("testFinishAuditEvidenceExport(); testHoldAuditEvidenceExport = false");
  await fill('[aria-label="Search audit findings"]', '');
  await fill('[aria-label="Audit finding category"]', '');
  await until("document.querySelector('[data-audit-finding-id=\"fixture-1\"]')", "Clearing a finding filter must restore another finding choice");
  await click('[data-audit-finding-id="fixture-1"]');
  await until("document.querySelector('[aria-label=\"Evidence HTTP status\"]')?.value === ''", "Choosing another finding must reset the evidence HTTP status filter");
  await click('.audit-report-summary button');
  await until("document.querySelector('.audit-report-summary')?.textContent.includes('/tmp/ferrous-audit-report/index.html')", "Report export must disclose its complete artifact path");
  assert.ok(await evaluate("document.querySelector('.audit-report-priorities')?.textContent.includes('Missing page title') && document.querySelector('.audit-report-priorities')?.textContent.includes('Add a descriptive title.')"), "The workspace must show measured action priorities before any AI generation");
  await click('.audit-report-ai fieldset button');
  await until("document.querySelector('.audit-ai-preview')?.textContent.includes('Sampling policy: Up to 3 stored examples per finding.') && document.querySelector('.audit-ai-preview')?.textContent.includes('Overview pending: Yes')", "AI preview must disclose overview planning and its actual sampling policy");
  await evaluate("[...document.querySelectorAll('.audit-ai-preview button')].find((button) => button.textContent.includes('overview')).click()");
  await until("document.querySelector('.audit-ai-overview')?.textContent.includes('<strong>Fixture overview</strong>') && document.querySelector('.audit-report-ai')?.textContent.includes('205 / 205 findings explained')", "AI overview must preserve literal provider text and retain complete finding counts");
  assert.equal(await evaluate("document.querySelector('.audit-ai-overview strong')"), null, "AI overview text must be escaped rather than interpreted as markup");
  assert.ok(await evaluate("document.querySelector('.audit-ai-preserved')?.textContent.includes('fixture-ai-v1') && document.querySelector('.audit-ai-preserved')?.textContent.includes('fixture-model') && document.querySelector('.audit-report-ai')?.textContent.includes('Latest generation failed: Fixture provider timed out')"), "A failed latest generation must keep its error while showing the preserved export generation provenance");
  assert.ok(await evaluate("document.querySelector('.audit-report-summary')?.textContent.includes('1,205')"), "AI output must not change frozen report counts");
  await fill('[aria-label="Audit report title"]', 'Follow-up deterministic audit');
  await click('.audit-report-launcher .primary');
  await until("document.querySelector('.audit-report-summary h3')?.textContent.includes('Follow-up deterministic audit')", "A second frozen report must become the comparison current report");
  await until("!document.querySelector('[aria-label=\"Comparison baseline report\"]').disabled && !document.querySelector('[aria-label=\"Comparison current report\"]').disabled", "Comparison controls must offer saved baseline and current reports");
  await evaluate("[...document.querySelectorAll('.audit-report-comparisons button')].find((button) => button.textContent.includes('Compare reports')).click()");
  await until("document.querySelector('.audit-comparison-summary')?.textContent.includes('2 findings') && document.querySelector('[data-audit-comparison-finding=\"title.missing\"]')", "Comparison preparation must publish persisted follow-up counts");
  await click('[data-audit-comparison-finding="title.missing"]');
  await until("document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('resolved') && document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('Fixed title')", "A resolved finding must show measured before and after values");
  await evaluate("[...document.querySelectorAll('.audit-report-comparisons .audit-report-ai fieldset button')].find((button) => button.textContent.includes('Review AI data')).click()");
  await until("document.querySelector('.audit-report-comparisons .audit-ai-preview')?.textContent.includes('One stored comparison observation per finding.')", "Comparison AI must be an explicit preview with comparison sampling details");
  await evaluate("[...document.querySelectorAll('.audit-report-comparisons .audit-ai-preview button')].find((button) => button.textContent.includes('overview')).click()");
  await until("testAuditRequests.at(-1).command === 'run_audit_report_comparison_ai' && testAuditRequests.at(-1).comparisonId === testAuditComparisons[0].id", "Comparison AI send must use the comparison command and frozen comparison ID");
  await evaluate("testComparisonAiError = true");
  await evaluate("[...document.querySelectorAll('.audit-report-comparisons .audit-report-ai fieldset button')].find((button) => button.textContent.includes('Review AI data')).click()");
  await until("document.querySelector('.audit-report-comparisons .audit-ai-preview')", "Comparison AI failure setup must still require a fresh preview");
  await evaluate("[...document.querySelectorAll('.audit-report-comparisons .audit-ai-preview button')].find((button) => button.textContent.includes('overview')).click()");
  await until("document.querySelector('.audit-report-comparisons .audit-report-ai [role=alert]')?.textContent.includes('Comparison AI provider unavailable')", "A comparison AI failure must remain visible without altering measured comparison data");
  assert.ok(await evaluate("document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('resolved') && document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('Fixed title')"), "Comparison evidence must remain reachable after AI failure");
  await evaluate("testComparisonAiError = false");
  await click('[data-audit-comparison-finding="links.broken"]');
  await until("document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('notObserved') && document.querySelector('.audit-comparison-evidence-table')?.textContent.includes('does not verify a fix')", "Missing current evidence must remain not observed rather than a verified fix");
  await evaluate("testAuditDeleteError = true");
  await click('[aria-label="Delete Saved deterministic audit"]');
  await click('.quit-modal .destructive');
  await until("document.querySelector('.quit-modal .error-bar')?.textContent.includes('Could not delete')", "A failed report deletion must remain retryable in its confirmation dialog");
  await evaluate("testAuditDeleteError = false");
  await click('.quit-modal .destructive');
  await until("!document.querySelector('[aria-label=\"Delete Saved deterministic audit\"]')", "Retrying deletion must remove only the requested report after native success");
  await click('[aria-label="Delete Follow-up deterministic audit"]');
  await click('.quit-modal .destructive');
  await until("!document.querySelector('[data-audit-report-id]') && document.querySelector('[data-audit-comparison-id]')", "Deleting source reports must retain the persisted comparison library");
  await click('[title="Close audit reports"]');
  await menuItem("Audit reports");
  await click('[data-audit-comparison-id]');
  await until("document.querySelector('.audit-comparison-summary')?.textContent.includes('Follow-up deterministic audit')", "A stored comparison must reopen after its source reports were deleted");
  await click('.audit-comparison-summary button');
  await until("document.querySelector('.audit-comparison-summary')?.textContent.includes('/tmp/ferrous-report-comparison/index.html')", "A reopened comparison must export independently of deleted source reports");
  assert.equal(await evaluate("testAuditRequests.at(-1).command"), "export_audit_report_comparison", "Comparison export must call its native command");
  await evaluate("testHoldComparisonEvidence = true");
  await click('[data-audit-comparison-finding="title.missing"]');
  await click('[title="Close audit reports"]');
  await evaluate("testFinishComparisonEvidence(); testHoldComparisonEvidence = false");
  await menuItem("Audit reports");
  await click('[data-audit-comparison-id]');
  await until("document.querySelector('.audit-comparison-summary')", "A late evidence query must not block reopening its saved comparison");
  await click('.audit-comparison-summary button');
  await until("document.querySelector('.audit-comparison-summary')?.textContent.includes('/tmp/ferrous-report-comparison/index.html')", "Reopened comparison export must remain usable after a cancelled late evidence query");
  await click('[title="Close audit reports"]');
  await evaluate("testHoldAuditPrepare = true");
  await menuItem("Audit reports");
  await fill('[aria-label="Audit report title"]', 'Cancelled late audit');
  await click('.audit-report-launcher .primary');
  await until("[...document.querySelectorAll('.audit-report-launcher button')].some((button) => button.textContent.includes('Cancel preparation'))", "An in-flight report preparation must be cancellable");
  await click('[title="Close audit reports"]');
  await evaluate("testFinishAuditPrepare(); testHoldAuditPrepare = false");
  await delay(100);
  await menuItem("Audit reports");
  await until("document.querySelector('.audit-report-modal') && !document.querySelector('.audit-report-summary') && testAuditCancels.length > 0", "A late prepared report must not reopen or publish after cancellation");
  await click('[data-audit-report-id]');
  await until("document.querySelector('.audit-report-summary')", "Saved report cards must reopen their frozen report");
  await evaluate("testHoldAuditExport = true");
  await click('.audit-report-summary button');
  await until("[...document.querySelectorAll('.audit-report-launcher button')].some((button) => button.textContent.includes('Cancel preparation'))", "An in-flight report export must be cancellable");
  await evaluate("[...document.querySelectorAll('.audit-report-launcher button')].find((button) => button.textContent.includes('Cancel preparation')).click()");
  await until("testAuditCancels.length > 1", "Cancelling a report export must call the shared native cancellation command");
  await evaluate("testFinishAuditExport(); testHoldAuditExport = false");
  await click('[title="Close audit reports"]');
  await fill('[aria-label="Seed URL"]', "");
  assert.ok(await evaluate("document.querySelector('[title=\"Start crawl\"]').disabled"), "An empty seed must not offer a runnable crawl");
  await fill('[aria-label="Seed URL"]', "https://keyboard.test/");
  await evaluate("document.querySelector('[aria-label=\"Seed URL\"]').focus()");
  await pressKey("Enter");
  await until("document.querySelector('[title=\"Pause crawl\"]') && testStartedSeed === 'https://keyboard.test/'", "Submitting the seed with Enter must start the crawl");
  assert.deepEqual(await evaluate("testStartedScope"), ["exactHost", "exactUrl"], "Starting must pass the selected scope to the engine");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').disabled"), "A running crawl must lock scope changes");
  await openMode();
  assert.ok(await evaluate("document.querySelector('[data-mode=\"list\"]').getAttribute('aria-disabled') === 'true' && document.querySelector('[data-action=\"compare-crawls\"]').getAttribute('aria-disabled') === 'true'"), "Active crawls must lock mode changes and comparison");
  await pressKey("Escape");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Scope");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Folder scope\"]').disabled && document.querySelector('[aria-label=\"Subdomain scope\"]').disabled"), "Settings must also lock the active scope");
  await click('[title="Close settings"]');
  assert.ok(await contrast('.destructive') >= 4.5, "The active Stop button must stay readable in light mode");
  await evaluate("testEmit({ kind: 'finished' })");
  await until("document.querySelector('[title=\"Start crawl\"]')", "The test crawl must finish before switching mode");
  await chooseMode("list");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Crawl scope\"]').disabled && document.querySelector('[aria-label=\"Crawl scope\"]').value === 'list'"), "List mode must explain its fixed scope while remembering Spider rules");
  await fill('[aria-label="Root URL"]', "");
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "List sources must remain configurable");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Scope').click()");
  await fill('[aria-label="List URLs"]', "https://list.test/first");
  await applySettings();
  await click('[title="Close settings"]');
  await click('[title="Start crawl"]');
  await until("document.querySelector('[title=\"Pause crawl\"]') && testStartedSeed === '' && testStartedList[0] === 'https://list.test/first'", "List mode must allow a blank root when URL sources are configured");
  await evaluate("testEmit({ kind: 'finished' })");
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Settings must remain accessible after a crawl");
  const beforeSettingsSearch = await savedSettings();
  await settingsTab("HTTP headers");
  const chromeHeaders = [
    { name: "Accept", value: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8" },
    { name: "Accept-Language", value: "en-US,en;q=0.9" },
    { name: "Upgrade-Insecure-Requests", value: "1" },
  ];
  const chromeAgent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Request User-Agent\"]')?.value"), chromeAgent, "Fresh settings must use the Chrome desktop agent");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.request-header')].map((row) => ({ name: row.querySelectorAll('input')[0].value, value: row.querySelectorAll('input')[1].value }))"), chromeHeaders, "Fresh settings must expose editable Chrome request headers");
  if (process.env.UI_SCREENSHOT) {
    const wasDark = await evaluate("document.documentElement.classList.contains('dark')");
    for (const [width, height] of [[1280, 840], [390, 640]]) {
      await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      assert.ok(await evaluate("document.querySelector('.settings-content').scrollWidth <= document.querySelector('.settings-content').clientWidth"), "Long browser header values must fit the Settings layout");
      for (const theme of ["dark", "light"]) {
        await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"}); document.documentElement.style.colorScheme = ${JSON.stringify(theme)}`);
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.headers-${width}-${theme}.png`, Buffer.from(shot.data, "base64"));
      }
    }
    await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 840, deviceScaleFactor: 1, mobile: false });
    await evaluate(`document.documentElement.classList.toggle('dark', ${wasDark}); document.documentElement.style.colorScheme = ${JSON.stringify(wasDark ? "dark" : "light")}`);
  }
  await click('[aria-label="Use Ferrous Frog request defaults"]');
  assert.equal(await evaluate("document.querySelectorAll('.request-header').length"), 0, "The crawler preset must clear browser header overrides");
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Request User-Agent\"]').value.startsWith('FerrousFrogSeoSpider/')"), "The crawler preset must restore crawler identification");
  assert.deepEqual(await savedSettings(), beforeSettingsSearch, "Request presets must remain in the Settings draft until Apply");
  await click('[aria-label="Use Chrome request defaults"]');
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Request User-Agent\"]').value"), chromeAgent, "Chrome defaults must restore the agent");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.request-header')].map((row) => ({ name: row.querySelectorAll('input')[0].value, value: row.querySelectorAll('input')[1].value }))"), chromeHeaders, "Chrome defaults must restore editable headers");
  await settingsTab("Storage");
  for (const [query, section, field] of [["max URLs", "limits", "Max URLs"], ["concurrency", "speed", "Threads"], ["respect robots", "robots", "Respect robots.txt"], ["near duplicate", "content", "Dup bits"], ["user-agent", "requests", "Request User-Agent"]]) {
    await fill('[aria-label="Search settings"]', query);
    await until("document.querySelector('[aria-label=\"Matching settings controls\"] option:not([value=\"\"])')", `Search must offer direct controls for ${query}`);
    await evaluate("document.querySelector('[aria-label=\"Search settings\"]').focus()");
    await pressKey("Enter");
    await until(`document.activeElement.closest('[data-settings-section]')?.dataset.settingsSection === ${JSON.stringify(section)}`, `Enter must reveal and focus the ${section} control`);
    assert.equal(await evaluate("document.activeElement.getAttribute('aria-label') || document.activeElement.closest('label').textContent.trim()"), field, "Search must focus the matching control without editing it");
    assert.equal(await evaluate("document.querySelectorAll('[aria-label=\"Matching settings controls\"] option:not([value=\"\"])').length"), 1, "Each control must appear once, without hidden checkbox inputs");
  }
  await fill('[aria-label="Search settings"]', "override");
  await until("[...document.querySelector('[aria-label=\"Matching settings controls\"]').options].some((option) => option.textContent.includes('robots.txt override') && option.disabled)", "Disabled override text must stay discoverable without being offered as editable");
  await fill('[aria-label="Search settings"]', "Max response");
  const responseControl = await evaluate("[...document.querySelector('[aria-label=\"Matching settings controls\"]').options].find((option) => option.textContent === 'Max response MiB').value");
  await select("Matching settings controls", responseControl);
  await until("document.activeElement.getAttribute('aria-describedby') === 'response-limit-help'", "Choosing a control must focus its input and expose its contextual help");
  assert.ok(await evaluate("document.getElementById(document.activeElement.getAttribute('aria-describedby')).textContent.includes('Browser rendering')"), "Download help must explain the actual HTTP limit");
  assert.deepEqual(await savedSettings(), beforeSettingsSearch, "Direct field search must preserve saved preferences");
  for (const [query, section] of [[" USER-AGENT ", "HTTP headers"], ["XPath", "Extraction"], ["CDP", "Rendering"]]) {
    await fill('[aria-label="Search settings"]', query);
    await until(`document.querySelector('.settings-tab-button[aria-current="page"]')?.textContent.trim() === ${JSON.stringify(section)}`,
      `Searching for ${query} must open the matching section`);
    assert.equal(await evaluate("document.querySelectorAll('.settings-tab-button').length"), 1, "Settings search must filter the navigation by control keywords");
  }
  await fill('[aria-label="Search settings"]', "missing-setting-name");
  await until("document.querySelector('.settings-search-empty')", "Unmatched settings must offer a clear search action");
  await click('.settings-search-empty button');
  await until("document.querySelectorAll('.settings-tab-button').length === 17", "Clearing the search must restore all working sections");
  await until("[...document.querySelectorAll('.settings-tabs details')].every((group) => !group.open)", "Clearing search must restore collapsed navigation");
  await evaluate("document.querySelector('.settings-tabs summary').focus()");
  assert.equal(await evaluate("document.activeElement.tagName"), "SUMMARY", "Settings group headers must accept keyboard focus");
  await pressKey("Enter");
  await until("document.querySelector('.settings-tabs details').open", "Settings groups must expand from the keyboard");
  await pressKey("Enter");
  await until("!document.querySelector('.settings-tabs details').open", "Settings groups must collapse from the keyboard");
  await fill('[aria-label="Search settings"]', "Chrome");
  await until("document.querySelector('.settings-tabs details').open", "A search match must reveal a collapsed group");
  await fill('[aria-label="Search settings"]', "");
  await until("document.querySelectorAll('.settings-tab-button').length === 17", "Clearing search must restore all sections");
  assert.ok(await evaluate("[...document.querySelectorAll('.settings-tabs details')].every((group) => !group.open)"), "Search must not leave every group expanded");
  await click('.settings-tabs summary');
  await until("document.querySelector('.settings-tabs details').open", "Settings groups must expand again");
  await settingsTab('Limits');
  await evaluate("document.querySelector('.settings-content').scrollTop = 300");
  await settingsTab('Scope');
  await until("document.querySelector('.settings-content').scrollTop === 0", "Changing settings sections must restore the beginning of the form");
  assert.deepEqual(await savedSettings(), beforeSettingsSearch, "Searching and navigating must not change crawl preferences");
  for (const [width, height] of [[1280, 840], [390, 640]]) {
    await cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    assert.ok(await evaluate("document.querySelector('.settings-sidebar').scrollWidth <= document.querySelector('.settings-sidebar').clientWidth && document.querySelector('.settings-content').clientHeight > 200"),
      "Settings navigation must fit and leave space for controls at narrow widths");
    if (width < 650) {
      assert.ok(await evaluate("getComputedStyle(document.querySelector('.settings-section-picker')).display !== 'none' && getComputedStyle(document.querySelector('.settings-tabs')).display === 'none'"), "Narrow settings need a usable section picker");
      await select('Settings section', 'rendering');
      await until("!document.querySelector('[data-settings-section=\"rendering\"]').hidden", "The narrow section picker must open the chosen controls");
    } else {
      assert.ok(await evaluate("document.querySelector('.settings-group-items .settings-tab-button span').getBoundingClientRect().left > document.querySelector('.settings-tabs summary span').getBoundingClientRect().left"), "Child settings need a clear tree indentation");
    }
    await fill('[aria-label="Search settings"]', "Backoff");
    await evaluate("document.querySelector('[aria-label=\"Search settings\"]').focus()");
    await pressKey("Enter");
    await until("document.activeElement.getAttribute('aria-describedby') === 'retry-help'", "Control search must focus a matching field at every window width");
    assert.ok(await evaluate(`(() => {
      const content = document.querySelector('.settings-content').getBoundingClientRect();
      const input = document.activeElement.getBoundingClientRect();
      const sidebar = document.querySelector('.settings-sidebar');
      return input.top >= content.top && input.bottom <= content.bottom && sidebar.scrollHeight <= sidebar.clientHeight;
    })()`), "Focused fields and the search picker must remain visible in small windows");
    for (const theme of ["dark", "light"]) {
      await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"}); document.documentElement.style.colorScheme = ${JSON.stringify(theme)}`);
      assert.ok(await contrast('.settings-control-search p', '.settings-sidebar') >= 4.5, `Settings search help must remain readable in ${theme} mode`);
    }
    if (process.env.UI_SCREENSHOT) {
      for (const theme of ["dark", "light"]) {
        await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"}); document.documentElement.style.colorScheme = ${JSON.stringify(theme)}`);
        const shot = await captureScreenshot();
        await writeFile(`${process.env.UI_SCREENSHOT}.settings-tree-${width}-${theme}.png`, Buffer.from(shot.data, "base64"));
      }
    }
    await fill('[aria-label="Search settings"]', "");
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 840, deviceScaleFactor: 1, mobile: false });
  await fill('[aria-label="Search settings"]', "Chrome");
  await click('[title="Close settings"]');
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('[aria-label=\"Search settings\"]')?.value === '' && document.querySelectorAll('.settings-tab-button').length === 17", "Reopening Settings must clear its search");
  assert.ok(await evaluate("[...document.querySelectorAll('.settings-tabs details')].every((group) => !group.open)"), "Reopening Settings must start with collapsed groups");
  await settingsTab("Limits");
  await markSetting("Threads");
  await fill(setting("Threads"), "7");
  await markSetting("Max URLs");
  await fill(setting("Max URLs"), "2345");
  await settingsTab("robots.txt");
  await toggleSetting("Respect robots.txt");
  await settingsTab("Resources");
  await toggleSetting("Crawl Images");
  await settingsTab("HTTP headers");
  await click('[aria-label="Use Ferrous Frog request defaults"]');
  await fill('[aria-label="Request User-Agent"]', 'HeaderFixture/1.0');
  await evaluate("[...document.querySelectorAll('.request-headers button')].find((button) => button.textContent.includes('Add header')).click()");
  await fill('[aria-label="Header 1 name"]', 'Authorization');
  await fill('[aria-label="Header 1 value"]', 'fixture-value');
  await evaluate("testHeaderValidationFailure = true");
  const beforeHeaderValidation = await savedSettings();
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-validation-error')?.textContent.includes('reserved')", "Native header validation must be shown before saving");
  assert.deepEqual(await savedSettings(), beforeHeaderValidation, "Rejected request headers must not reach persisted configuration");
  await evaluate("testHeaderValidationFailure = false");
  await fill('[aria-label="Header 1 name"]', ' Accept-Language ');
  await fill('[aria-label="Header 1 value"]', 'en-GB');
  await settingsTab("Content");
  await fill('[aria-label="Content include selectors"]', 'main\narticle');
  await fill('[aria-label="Content exclude selectors"]', 'nav\n\nfooter');
  const beforeContentPreview = await savedSettings();
  await fill('[aria-label="Content preview HTML"]', '<main>Hello world<footer>Footer</footer></main>');
  await evaluate("[...document.querySelectorAll('[data-settings-section=\"content\"] button')].find((button) => button.textContent === 'Preview text').click()");
  await until("document.querySelector('.content-preview-result')?.textContent.includes('2 words · 25.0%')", "Preview must show engine text and metrics using percentage units");
  assert.deepEqual(await evaluate("testContentPreviewRequest.content"), { includeSelectors: ['main', 'article'], excludeSelectors: ['nav', 'footer'] }, "Preview must normalize selectors exactly like the crawl configuration");
  assert.deepEqual(await savedSettings(), beforeContentPreview, "Pasted preview HTML and pending selectors must not be saved by previewing");
  await evaluate("testHoldContentPreview = true");
  await evaluate("[...document.querySelectorAll('[data-settings-section=\"content\"] button')].find((button) => button.textContent === 'Preview text').click()");
  await until("testFinishContentPreview", "Preview must reach the native worker");
  await fill('[aria-label="Content preview HTML"]', '<main>A changed sample</main>');
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Preview content text\"]').disabled"), "Editing a sample must not allow overlapping parser workers");
  await evaluate("testHoldContentPreview = false; testFinishContentPreview()");
  await delay(100);
  assert.ok(await evaluate("!document.querySelector('.content-preview-result')"), "A late preview must not overwrite changed sample text");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 390, height: 640, deviceScaleFactor: 1, mobile: false });
  await evaluate("testContentPreviewFailure = true");
  await click('[aria-label="Preview content text"]');
  await until("document.querySelector('[data-settings-section=\"content\"] [role=\"alert\"]')?.textContent.includes('Invalid content include selector')", "Preview errors must remain visible inside the content section");
  assert.ok(await evaluate(`(() => {
    const modal = document.querySelector('.settings-modal').getBoundingClientRect();
    const footer = document.querySelector('.settings-footer').getBoundingClientRect();
    return !document.querySelector('.settings-modal > .settings-validation-error') && footer.height >= 40 && footer.top >= modal.top && footer.bottom <= modal.bottom;
  })()`), "A nested preview error must keep Apply and Cancel inside a narrow Settings dialog");
  await evaluate("testContentPreviewFailure = false");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 840, deviceScaleFactor: 1, mobile: false });
  await settingsTab("Query");
  await evaluate("document.querySelector('.settings-section:not([hidden]) .checkbox-root').click()");
  await settingsTab("Rendering");
  await until("document.querySelector('.rendering-status')?.textContent.includes('Compatible browser found')", "Settings must report the detected rendering capability");
  await toggleSetting("Render DOM");
  await markSetting("Wait after load");
  await fill(setting("Wait after load"), "800");
  await settingsTab("Extraction");
  assert.ok(await evaluate("[...document.querySelectorAll('.capture-settings [role=\"checkbox\"]')].every((control) => control.getAttribute('aria-checked') === 'false')"), "Retaining page bodies and headers must be opt-in for existing settings");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Maximum capture bytes\"]').max"), '1048576', "The UI must enforce the 1 MiB capture ceiling");
  await toggleSetting('Client-observed response headers');
  await toggleSetting('Raw HTML');
  await toggleSetting('Visible text');
  await fill('[aria-label="Maximum capture bytes"]', '65536');
  await evaluate("document.querySelector('.settings-section:not([hidden]) .section-heading button').click()");
  await fill('[aria-label="Extractor pattern"]', 'main h1');
  const beforeExtractionPreview = await savedSettings();
  await evaluate("document.querySelector('[aria-label=\"Extractor name\"]').focus()");
  await cdp("Input.insertText", { text: "_test" });
  assert.ok(await evaluate("document.activeElement.matches('[aria-label=\"Extractor name\"]')"), "Renaming an extraction rule must retain typing focus");
  await fill('[aria-label="Extraction preview HTML"]', '<main><h1>Sample heading</h1></main>');
  await click('[aria-label="Test custom extraction"]');
  await until("document.querySelector('.extractor-preview [role=\"status\"]')?.textContent.includes('1 match')", "A rule can be tested before it is applied");
  assert.equal(await evaluate("testExtractionPreviewRequest.extractor.pattern"), "main h1", "The preview must use the selected draft rule");
  assert.ok(await evaluate("!document.querySelector('.extractor-preview img') && document.querySelector('.extractor-preview pre').textContent.includes('<img')"), "Extracted markup must be displayed as text");
  assert.deepEqual(await savedSettings(), beforeExtractionPreview, "Testing a rule must not save draft settings");
  await evaluate("testHoldExtractionPreview = true");
  await click('[aria-label="Test custom extraction"]');
  await until("window.testFinishExtractionPreview", "The preview fixture must hold its worker");
  await fill('[aria-label="Extractor pattern"]', 'main h2');
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Test custom extraction\"]').disabled"), "Changing a rule must not start an overlapping preview worker");
  await evaluate("testHoldExtractionPreview = false; testFinishExtractionPreview()");
  await until("!document.querySelector('[aria-label=\"Test custom extraction\"]').disabled", "Finishing an obsolete preview must permit testing the new rule");
  assert.ok(await evaluate("!document.querySelector('.extractor-preview [role=\"status\"]')"), "A late preview must not replace an edited rule's result");
  await fill('[aria-label="Extractor pattern"]', '[');
  await click('[aria-label="Test custom extraction"]');
  await until("document.querySelector('.extractor-preview [role=\"alert\"]')?.textContent.includes('Invalid CSS')", "Invalid extraction rules must show their preview error");
  await fill('[aria-label="Extractor pattern"]', 'main h1');
  await applySettings();
  await evaluate("testHoldExtractionPreview = true; testFinishExtractionPreview = undefined");
  await click('[aria-label="Test custom extraction"]');
  await until("window.testFinishExtractionPreview", "A preview must be pending before closing Settings");
  const heldExtractionCalls = await evaluate("testExtractionPreviewCalls");
  await click('[title="Close settings"]');
  await until("!document.querySelector('.settings-modal')", "Settings must fully unmount before checking worker persistence");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Extraction");
  await fill('[aria-label="Extraction preview HTML"]', '<main><h1>New sample</h1></main>');
  assert.ok(await evaluate("document.querySelector('[aria-label=\"Test custom extraction\"]').disabled"), "Closing and reopening Settings must retain the pending preview lock");
  await click('[aria-label="Test custom extraction"]');
  assert.equal(await evaluate("testExtractionPreviewCalls"), heldExtractionCalls, "Reopening must not start another worker while the previous one is pending");
  await evaluate("testHoldExtractionPreview = false; testFinishExtractionPreview()");
  await until("!document.querySelector('[aria-label=\"Test custom extraction\"]').disabled", "The reopened tester must unlock when the original worker finishes");
  assert.ok(await evaluate("!document.querySelector('.extractor-preview [role=\"status\"]')"), "An old dialog's result must not populate the new sample");
  await click('[title="Close settings"]');
  const settingsBeforeReload = await savedSettings();
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Saved settings should be readable after reopening the app");
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "7", "Concurrency must survive reopening without saving a named profile");
  await markSetting("Max URLs");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "2345", "Crawl limits must survive reopening");
  assert.ok(await evaluate("[...document.querySelectorAll('.checkbox-field')].find((item) => item.textContent.trim() === 'Respect robots.txt').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'false'"), "An explicit robots choice must not be replaced by the default during restore");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl mode\"]').dataset.mode"), "list", "Crawl mode must be restored");
  assert.equal((await savedSettings()).config.folderScope, "exactUrl", "List mode must retain the last Spider scope on reload");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Root URL\"]').value"), "", "An intentionally blank List root must override the older saved URL");
  await settingsTab("Resources");
  assert.ok(await evaluate("[...document.querySelectorAll('.settings-section:not([hidden]) .checkbox-field')].find((item) => item.textContent.trim() === 'Crawl Images').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), "Nested resource choices must be restored");
  await settingsTab("HTTP headers");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Request User-Agent\"]').value"), "HeaderFixture/1.0", "A saved custom agent must survive restart without being replaced by Chrome defaults");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Header 1 name\"]').value"), "Accept-Language", "Validated header names must be normalized and restored");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Header 1 value\"]').value"), "en-GB", "Custom request header values must survive restart");
  await click('[aria-label="Remove header 1"]');
  await click('[aria-label="Use Chrome request defaults"]');
  await click('[data-action="cancel-settings"]');
  assert.deepEqual(await savedSettings(), settingsBeforeReload, "Cancel must discard header removal and Chrome preset changes");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Content");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Content include selectors\"]').value"), "main\narticle", "Content regions must survive restart");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Content exclude selectors\"]').value"), "nav\nfooter", "Excluded regions must be normalized and restored");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Content preview HTML\"]').value"), "", "Preview HTML must not be persisted with crawl settings");
  await settingsTab("Rendering");
  await markSetting("Wait after load");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Wait after load"))}).value`), "800", "Rendering options must be restored");
  await settingsTab("Extraction");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Extractor pattern\"]').value"), "main h1", "Custom extractor definitions must be restored");
  assert.deepEqual((await savedSettings()).config.capture, { responseHeaders: true, rawHtml: true, renderedHtml: false, visibleText: true, maxBytes: 65536 }, "Capture preferences must persist with the applied crawl configuration");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Maximum capture bytes\"]').value"), '65536', "Capture limits must restore in Settings");

  assert.equal(await evaluate("document.querySelector('[aria-label=\"Extraction preview HTML\"]').value"), "", "Sample extraction HTML must not be persisted with crawl settings");
  await settingsTab("Scope");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"List URLs\"]').value"), "https://list.test/first", "List sources must be restored");
  await settingsTab("Storage");
  assert.equal(await evaluate("document.querySelector('.settings-section:not([hidden]) input[readonly]').value"), "SQLite", "Desktop storage must remain SQLite after reopening");
  assert.deepEqual(await savedSettings(), settingsBeforeReload, "Reopening should preserve the complete settings snapshot");
  const writes = await evaluate("testSettingsWrites");
  await evaluate("testEmit({ kind: 'started' }).then(() => testEmit({ kind: 'finished' }))");
  assert.equal(await evaluate("testSettingsWrites"), writes, "Crawl events must not write settings or serialize crawl results");
  await click('[title="Close settings"]');
  await evaluate("window.testProfile = { id: 'fixture-profile', name: 'Fixture preset', config: { ...JSON.parse(localStorage.getItem('ferrous-frog-settings')).config, concurrency: 3, maxUrls: 8765 } }");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Profiles");
  await until("document.querySelector('.settings-section:not([hidden]) option[value=\"fixture-profile\"]')", "The named profile should be available");
  await evaluate("(() => { const profile = document.querySelector('.settings-section:not([hidden]) select'); profile.value = 'fixture-profile'; profile.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await until("document.querySelector('.settings-footer [role=\"status\"]')?.textContent === 'Unapplied changes'", "Loading a profile must populate a draft");
  assert.notEqual((await savedSettings()).config.maxUrls, 8765, "A loaded profile must not become active before Apply");
  await applySettings();
  await until("JSON.parse(localStorage.getItem('ferrous-frog-settings')).config.maxUrls === 8765", "Loading a profile must also update the automatic snapshot");
  await click('[title="Close settings"]');
  await evaluate("window.testProfiles = ['a', 'b'].map((id, index) => ({ ...testProfile, id, name: 'Profile ' + id, config: { ...testProfile.config, maxUrls: (index + 1) * 1111 } })); window.testHoldProfiles = true; window.testProfileLoads = {}");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Profiles");
  await until("document.querySelector('.settings-section:not([hidden]) option[value=\"b\"]')", "Both test profiles must be available");
  await selectProfile("a");
  await until("testProfileLoads.a", "The first profile load must start");
  await selectProfile("b");
  await until("testProfileLoads.b", "The second profile load must start");
  await evaluate("testProfileLoads.a()");
  await evaluate("testProfileLoads.b()");
  await settingsTab("Limits");
  await markSetting("Max URLs");
  await until(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value === '2222'`, "The most recently requested profile must win even when the older response arrives first");
  assert.equal((await savedSettings()).config.maxUrls, 8765, "Asynchronous profile drafts must still require Apply");
  await settingsTab("Profiles");
  await selectProfile("a");
  await selectProfile("");
  await evaluate("testProfileLoads.a()");
  await settingsTab("Limits");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "2222", "Clearing profile selection must cancel its pending response");
  await settingsTab("Profiles");
  await evaluate("window.testProfileFailure = true");
  await selectProfile("a");
  await click('[data-action="cancel-settings"]');
  await evaluate("testProfileLoads.a()");
  assert.ok(await evaluate("!document.querySelector('[role=\"alert\"]')?.textContent.includes('Could not load this profile')"), "Cancelled profile failures must not publish an error after dismissal");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await markSetting("Max URLs");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "8765", "The last loaded profile must take effect after reopening without selecting it again");
  await evaluate("localStorage.setItem('ferrous-frog-settings', JSON.stringify({ version: 1, config: { maxUrls: 3210, resourceTypes: { images: true }, sitemap: { enabled: false, urls: ['https://legacy.test/sitemap.xml'] }, followNofollow: false } }))");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await markSetting("Max URLs");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "3210", "Older snapshots must retain their known values");
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "8", "New or missing fields must use their defaults");
  await markSetting("Max response MiB");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max response MiB"))}).value`), "20", "Legacy snapshots must receive the bounded HTTP default");
  const migratedSitemap = (await savedSettings()).config.sitemap;
  assert.deepEqual(migratedSitemap, { enabled: false, discoverFromRobots: true, probeDefault: true, followLinked: true,
    urls: ["https://legacy.test/sitemap.xml"] }, "Partial sitemap snapshots must preserve explicit choices and fill missing nested defaults");
  await settingsTab("Scope");
  for (const kind of ["internal", "external"]) {
    assert.ok(await evaluate(`[...document.querySelectorAll('.settings-section:not([hidden]) .checkbox-field')].find((item) => item.textContent.trim() === 'Follow ${kind} nofollow links').querySelector('[role="checkbox"]').getAttribute('aria-checked') === 'false'`), "Both nofollow choices must inherit a legacy profile's explicit false value");
  }
  await evaluate("localStorage.setItem('ferrous-frog-settings', JSON.stringify({ version: 1, config: { maxUrls: 3210, concurrency: 4, requestsPerSecond: 2, requestDelayMs: 250, respectRobots: false, useRobotsTxtOverride: true } }))");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  const beforePreset = await savedSettings();
  for (const [label, value] of [["Threads", "4"], ["RPS", "2"], ["Delay ms", "250"]]) {
    await markSetting(label);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting(label))}).value`), value, "Saved speed choices must survive a default change");
  }
  await evaluate("[...document.querySelectorAll('.settings-preset-row button')].find((button) => button.textContent.trim() === 'Default preset').click()");
  assert.deepEqual(await savedSettings(), beforePreset, "The default preset must remain a draft until Apply");
  await applySettings();
  const presetConfig = (await savedSettings()).config;
  assert.deepEqual([presetConfig.concurrency, presetConfig.requestsPerSecond, presetConfig.requestDelayMs], [8, 10, 100], "Apply must save the new default speed");
  assert.ok(presetConfig.respectRobots && !presetConfig.useRobotsTxtOverride, "The default preset must restore the site's robots policy");
  assert.equal(presetConfig.maxUrls, 3210, "A speed preset must preserve unrelated crawl limits");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  for (const [label, value] of [["Threads", "8"], ["RPS", "10"], ["Delay ms", "100"]]) {
    await markSetting(label);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting(label))}).value`), value, "The applied default speed must survive restarting");
  }
  for (const invalid of ['{invalid json', JSON.stringify({ version: 1, config: { listUrls: 'invalid array', respectRobots: false } }),
    JSON.stringify({ version: 1, config: { sitemap: { urls: [42] }, respectRobots: false } })]) {
    await evaluate(`localStorage.setItem('ferrous-frog-settings', ${JSON.stringify(invalid)})`);
    await reloadApp(false);
    await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Saved settings could not be read')", "Malformed saved settings must fall back with visible feedback");
    await openFixtureCrawl(false);
    await click('[aria-label="Crawl settings"]');
    assert.ok(await evaluate("[...document.querySelectorAll('.checkbox-field')].find((item) => item.textContent.trim() === 'Respect robots.txt').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), "Invalid settings must not disable the polite defaults");
  }
  await markSetting("Threads");
  await fill(setting("Threads"), "7");
  await applySettings();
  await until("!document.querySelector('[aria-label=\"Dismiss settings error\"]')", "A successful edit must replace invalid settings and clear its error");
  await evaluate("testSettingsWriteFailure = true");
  await fill(setting("Threads"), "8");
  await click('[data-action="apply-settings"]');
  await until("document.querySelector('.settings-modal [role=\"alert\"]')?.textContent.includes('Settings could not be saved')", "Write failures must be visible inside Settings");
  assert.equal((await savedSettings()).config.concurrency, 7, "A failed write must preserve the last saved snapshot");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "8", "A write failure must leave the current session editable");
  await click('[title="Close settings"]');
  await fill('[aria-label="Seed URL"]', "https://unsaved.test/");
  await until("document.querySelector('.data-table') && document.querySelector('[role=\"alert\"]')?.textContent.includes('Settings could not be saved')", "A failed legacy URL write must keep the app and its save warning visible");
  assert.notEqual((await savedSettings()).config.startUrl, "https://unsaved.test/", "A failed URL write must preserve the saved seed");
  await evaluate("testSettingsWriteFailure = false");
  await click('[aria-label="Crawl settings"]');
  await markSetting("Threads");
  await fill(setting("Threads"), "9");
  await applySettings();
  await until("!document.querySelector('[aria-label=\"Dismiss settings error\"]')", "Successful persistence must recover after a storage failure");
  assert.equal((await savedSettings()).config.concurrency, 9);
  assert.deepEqual(Object.keys(await savedSettings()).sort(), ['config', 'modeStartUrls', 'resumeCrawl', 'storageMode', 'version'], "Only preferences belong in the saved snapshot");
  for (const [tab, checkbox, field] of [["Storage", "Resume database", "Storage engine"], ["Rendering", "Render DOM", "Backend"]]) {
    await settingsTab(tab);
    assert.ok(await evaluate(`(() => {
      const section = document.querySelector('.settings-section:not([hidden])');
      const box = [...section.querySelectorAll('.checkbox-field')].find((label) => label.textContent.trim() === ${JSON.stringify(checkbox)}).querySelector('[role="checkbox"]').getBoundingClientRect();
      const input = [...section.querySelectorAll('label')].find((label) => label.firstChild.textContent.trim() === ${JSON.stringify(field)}).querySelector('input, select').getBoundingClientRect();
      return Math.abs(box.top + box.height / 2 - input.top - input.height / 2) < 1;
    })()`), `${tab} checkboxes must line up with the neighboring input, below its label`);
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.settings-${tab.toLowerCase()}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await settingsTab("Limits");
  await markSetting("Threads");
  await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).focus(); testRequestQuit()`);
  await until("document.activeElement.textContent === 'No'", "Native quit must take keyboard focus above Settings");
  await click('.quit-actions button:first-child');
  await until(`document.activeElement === document.querySelector(${JSON.stringify(setting("Threads"))})`, "Canceling native quit must restore focus to the settings field");
  await click('[title="Close settings"]');
  await search("no-matching-url");
  await until("document.querySelector('.grid-empty')", "Use an empty filter while retaining the saved crawl");
  await menuItem("Quit");
  await until("document.querySelector('[role=\"alertdialog\"]')", "Quit must ask for confirmation");
  await until("document.activeElement.textContent === 'No'", "Quit confirmation must focus the safe choice");
  assert.ok(await evaluate("document.querySelector('[role=\"alertdialog\"]').textContent.includes('saved automatically')"), "Quitting must explain that SQLite crawl results are retained");
  if (process.env.UI_SCREENSHOT) {
    const shot = await captureScreenshot();
    await writeFile(`${process.env.UI_SCREENSHOT}.quit.png`, Buffer.from(shot.data, "base64"));
  }
  await observeDialogExit('.quit-modal', `const backdrop = getComputedStyle(document.querySelector('.quit-backdrop'));
    window.testQuitBackdropExit = { animation: backdrop.animationName, pointerEvents: backdrop.pointerEvents };`);
  await evaluate("[...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'No').click()");
  await until("window.testDialogExit?.state === 'closed'", "Quit must remain mounted for its exit animation");
  assert.ok(await evaluate("testDialogExit.animation === 'dialog-exit' && testQuitBackdropExit.animation === 'fade-exit' && testQuitBackdropExit.pointerEvents === 'none'"), "Quit and backdrop must fade out together without intercepting clicks");
  await until("!document.querySelector('.quit-modal')", "Quit must unmount after its closing animation");
  await until("document.activeElement.matches('[aria-label=\"More tools\"]')", "Finished quit closing must return keyboard focus to its origin");
  assert.equal(await evaluate("testQuitCalls"), 0, "No must keep the app open without stopping its crawl");
  await evaluate("testRequestQuit()");
  await until("document.querySelector('[role=\"alertdialog\"]')", "Native close requests must use the same confirmation");
  await pressKey("Escape");
  await until("!document.querySelector('[role=\"alertdialog\"]')", "Escape must cancel quitting");
  await evaluate("testEmit({ kind: 'started' })");
  await evaluate("testRequestQuit(); testRequestQuit()");
  await until("document.querySelector('[role=\"alertdialog\"]')?.textContent.includes('A crawl is running')", "Active crawls must be explained before quitting");
  assert.equal(await evaluate("document.querySelectorAll('[role=\"alertdialog\"]').length"), 1, "Repeated close requests must not stack confirmation dialogs");
  await evaluate("window.testQuitFailure = true; [...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'Yes').click()");
  await until("document.querySelector('[role=\"alertdialog\"] [role=\"alert\"]')?.textContent.includes('Could not stop')", "A failed quit must remain visible and allow retrying");
  await evaluate("window.testQuitFailure = false; [...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'Yes').click()");
  await until("testQuitCalls === 2", "Only Yes may request quitting, including retries");

  await reloadApp();
  await click('.data-table tbody tr:not(.virtual-spacer)');
  await until("document.querySelector('.detail-header')", "Select an existing result before checking rendering");
  const selectedBeforeRendering = await evaluate("document.querySelector('.detail-header').textContent");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Rendering");
  await until("document.querySelector('.rendering-status')?.textContent.includes('/opt/chrome/chrome')", "Rendering must identify the browser it will use");
  await toggleSetting("Render DOM");
  await markSetting("Wait after load");
  await fill(setting("Wait after load"), "900");
  await applySettings();
  await click('[title="Close settings"]');
  await evaluate("testRenderingStatus = { available: false, browserPath: null, message: 'JavaScript rendering is not included in this build.' }");
  await click('[title="Start crawl"]');
  await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Turn off Render DOM')", "A saved rendering choice must fail clearly if the backend becomes unavailable");
  assert.equal(await evaluate("document.querySelector('.detail-header')?.textContent"), selectedBeforeRendering, "Rejected rendering must preserve the selected result");
  assert.equal(await evaluate("testStartCalls"), 0, "Unavailable rendering must be detected before sending a crawl start request");
  assert.ok(await evaluate("Boolean(document.querySelector('[title=\"Start crawl\"]') && document.querySelector('.data-table tbody tr:not(.virtual-spacer)'))"), "Rejected rendering must preserve results and recover Start");
  assert.ok((await savedSettings()).config.rendering.enabled, "An unavailable backend must not silently rewrite rendering preferences");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Rendering");
  await until("document.querySelector('.rendering-status')?.textContent.includes('not included')", "Standard builds must explain why rendering is unavailable");
  await markSetting("Wait after load");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Wait after load"))}).value`), "900");
  assert.ok(await evaluate(`document.querySelector(${JSON.stringify(setting("Wait after load"))}).disabled`), "Unavailable rendering options must not pretend to work");
  await toggleSetting("Render DOM");
  await applySettings();
  assert.equal((await savedSettings()).config.rendering.enabled, false, "A saved rendering choice can still be turned off when unavailable");
  assert.ok(await evaluate("document.querySelector('.settings-section:not([hidden]) [role=\"checkbox\"]').disabled"), "Unavailable rendering cannot be enabled");
  for (const theme of ["dark", "light"]) {
    await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
    await systemTheme(theme);
    assert.ok(await contrast('.rendering-status') >= 4.5, `Rendering status must be readable in ${theme} mode`);
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.rendering-${theme}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await evaluate("testRenderingStatus = { available: false, browserPath: null, message: 'No Chrome or Chromium executable found. Install a browser and check again.' }");
  await click('[aria-label="Check rendering availability"]');
  await until("document.querySelector('.rendering-status')?.textContent.includes('Install a browser')", "Missing browsers must offer a recovery step");
  await evaluate("testRenderingFailure = true");
  await click('[aria-label="Check rendering availability"]');
  await until("document.querySelector('.rendering-status [role=\"alert\"]')?.textContent.includes('Could not check')", "Detection failures must be visible and retryable");
  await evaluate("testRenderingFailure = false; testRenderingStatus = { available: true, browserPath: '/new/chrome', message: 'Compatible browser found.' }");
  await click('[aria-label="Check rendering availability"]');
  await until("document.querySelector('.rendering-status')?.textContent.includes('/new/chrome')", "Rechecking must detect a newly installed browser");
  assert.ok(await evaluate("!document.querySelector('.settings-section:not([hidden]) [role=\"checkbox\"]').disabled"), "A detected browser must enable the rendering choice");
  await evaluate("testRenderingDelay = 300");
  await click('[aria-label="Check rendering availability"]');
  await click('[title="Close settings"]');
  await evaluate("testRenderingDelay = 0; testRenderingStatus = { available: false, browserPath: null, message: 'Browser was removed.' }");
  await click('[aria-label="Crawl settings"]');
  await settingsTab("Rendering");
  await until("document.querySelector('.rendering-status')?.textContent.includes('Browser was removed')", "Reopening Settings must refresh browser availability");
  await delay(350);
  assert.ok(await evaluate("document.querySelector('.rendering-status').textContent.includes('Browser was removed')"), "An older browser probe must not overwrite the current result");
  await click('[title="Close settings"]');
  await click('[title="Start crawl"]');
  await until("document.querySelector('[title=\"Pause crawl\"]')", "HTML crawling must still work without rendering support");
  await evaluate("testEmit({ kind: 'finished' })");

  await systemTheme("dark");
  const deniedStorage = await cdp("Page.addScriptToEvaluateOnNewDocument", { source: `
    Storage.prototype.getItem = function () { throw new DOMException("Storage denied", "SecurityError"); };
    Storage.prototype.setItem = function () { throw new DOMException("Storage denied", "SecurityError"); };
  ` });
  await reloadApp();
  assert.equal(await evaluate("testStartupTheme"), "dark", "Unavailable storage must still allow the system theme before app startup");
  await until("document.querySelector('[aria-label=\"Dismiss settings error\"]')", "Unavailable storage must start the app with a visible settings warning");
  await click('[aria-label="Crawl settings"]');
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "8", "Unavailable storage must use the default configuration");
  await fill(setting("Threads"), "6");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "6", "Settings must remain editable when storage is unavailable");
  await cdp("Page.removeScriptToEvaluateOnNewDocument", { identifier: deniedStorage.identifier });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__?startup-error` });
  await until("window.testStartupCalls === 1 && document.querySelector('[role=\"alert\"]')?.textContent.includes('Crawl history')", "Failed history loading must still release the splash and display the error");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 920, height: 640, deviceScaleFactor: 1, mobile: false });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__?updates=available` });
  await until("window.testStartupCalls === 1 && window.testStartupHadHome", "Update checks must not hold the splash open");
  await until("document.querySelector('.update-modal')?.textContent.includes('0.2.0')", "A newer release must be announced after the workspace is ready");
  assert.ok(await evaluate("document.querySelector('.update-modal').textContent.includes('0.1.0')"), "The update notice must identify the installed version");
  assert.equal(await evaluate("testUpdateChecks"), 1, "Startup must make only one update request");
  assert.ok(await evaluate("document.activeElement?.textContent.includes('Remind me later')"), "The update notice must focus its non-disruptive action");
  if (process.env.UI_SCREENSHOT) {
    for (const theme of ["dark", "light"]) {
      await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.update-${theme}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await click('.update-modal [data-action="remind"]');
  await until("!document.querySelector('.update-modal')", "Reminding later must close the notice");
  assert.ok(await evaluate("Number(localStorage.getItem('ferrous-frog-update-reminder-until')) > Date.now() + 23 * 60 * 60 * 1000"), "The reminder must persist for a day");
  await reloadApp();
  await delay(2300);
  assert.equal(await evaluate("testUpdateChecks"), 0, "A saved reminder must suppress startup checks after a restart");
  await menuItem("Check for updates");
  await until("document.querySelector('.update-modal')?.textContent.includes('0.2.0') && !document.querySelector('.update-modal progress')", "A manual check must bypass the reminder");
  await evaluate("window.testOpenUrlFailure = true");
  await click('.update-modal [data-action="download"]');
  await until("document.querySelector('.update-modal [role=\"alert\"]')?.textContent.includes('browser')", "Browser launch failures must stay inside the update dialog");
  await evaluate("window.testOpenUrlFailure = false");
  await click('.update-modal [data-action="download"]');
  await until("testOpenedUrls.length === 1", "Download must open the release in the browser");
  assert.equal(await evaluate("testOpenedUrls[0]"), "https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/v0.2.0", "Downloads must use the checked release URL");
  await until("!document.querySelector('.update-modal')", "Opening the download page must return to the workspace");
  await evaluate("window.testUpdateResult.update = null");
  await menuItem("Check for updates");
  await until("document.querySelector('.update-modal [data-state=\"current\"]')", "Manual checks must confirm when no newer release exists");
  await pressKey("Escape");
  await until("!document.querySelector('.update-modal')", "Escape must close update information");
  await evaluate("localStorage.removeItem('ferrous-frog-update-reminder-until')");
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__?updates=error` });
  await until("window.testStartupCalls === 1 && window.testUpdateChecks === 1", "An offline update check must still allow startup");
  await delay(100);
  assert.ok(await evaluate("!document.querySelector('.update-modal') && !document.querySelector('[role=\"alert\"]')"), "Automatic connection errors must stay quiet");
  await menuItem("Check for updates");
  await until("document.querySelector('.update-modal [role=\"alert\"]')?.textContent.includes('GitHub')", "Manual connection failures must give visible feedback");
  await evaluate("window.testUpdateFailure = false");
  await click('.update-modal [data-action="retry"]');
  await until("document.querySelector('.update-modal [data-state=\"current\"]')", "The failed update check must be retryable");
  await pressKey("Escape");
  await until("!document.querySelector('.update-modal')", "The update dialog must dismiss after retry");

  await fill('.crawl-launcher [aria-label="Crawl URL"]', "https://recovery.test/");
  await click('[data-action="start-new-crawl"]');
  await until("document.querySelector('[title=\"Pause crawl\"]') && document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "Start a crawl for the delayed Stop recovery check");
  await delay(100);
  await evaluate("window.testPendingQueries = {}; window.testHeldQueries = { get_recovery_state: { recoverable: false, queued: 0, seen: 20, crawled: 20 } }");
  await click('[title="Stop crawl"]');
  await until("window.testPendingQueries.get_recovery_state", "The stopped crawl's recovery query must be pending");
  await evaluate("window.testRecovery = { recoverable: true, queued: 2, seen: 20, crawled: 18 }");
  await click('[aria-label="Crawl library"]');
  await click('[data-session-id="fixture-other"] [data-action="open-saved-crawl"]');
  await until("!document.querySelector('.crawl-home') && document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "A different recoverable crawl must open before the old Stop query returns");
  assert.ok((await savedSettings()).resumeCrawl, "The new crawl must enable its queued work");
  await evaluate("window.testPendingQueries.get_recovery_state()");
  await delay(150);
  assert.ok((await savedSettings()).resumeCrawl, "A stale Stop response must not clear another crawl's Resume selection");

  const splashWindow = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).app.windows.find((window) => window.label === 'splashscreen');
  await cdp("Emulation.setDeviceMetricsOverride", { width: splashWindow.width, height: splashWindow.height, deviceScaleFactor: 1, mobile: false });
  for (const preference of ["light", "dark", "system"]) {
    await evaluate(`localStorage.setItem('ferrous-frog-theme', ${JSON.stringify(preference)}); window.testBeforeReload = true`);
    await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/splash.html` });
    await until("!window.testBeforeReload && document.readyState === 'complete' && document.querySelector('.splash-screen progress')", "The real splash page and stylesheet must load independently of React");
    await evaluate("document.fonts.ready.then(() => true)");
    assert.equal(await evaluate("document.documentElement.classList.contains('dark')"), preference !== "light", "The splash must use the saved appearance before displaying");
    assert.ok(await evaluate("document.documentElement.scrollWidth <= innerWidth && document.querySelector('.splash-screen [role=\"status\"]').getBoundingClientRect().bottom <= innerHeight"), "The splash must fit its native window size");
    assert.ok(await contrast('.splash-screen p', '.splash-screen') >= 4.5, "Splash status text must remain readable");
    if (process.env.UI_SCREENSHOT) {
      const shot = await captureScreenshot();
      await writeFile(`${process.env.UI_SCREENSHOT}.splash-${preference}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await systemTheme("light");
  await until("!document.documentElement.classList.contains('dark')", "System appearance must also update on the splash screen");
  assert.deepEqual(errors, [], "No browser runtime errors");
  assert.equal(await evaluate("document.body.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }))"), true,
    "Development builds must retain the browser context menu");
  productionServer = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: false } });
  for (const path of ["/", "/splash.html"]) {
    const url = `http://127.0.0.1:${productionServer.httpServer.address().port}${path}`;
    await cdp("Page.navigate", { url });
    await until(`location.href === ${JSON.stringify(url)} && document.readyState === 'complete'`, "Production page must load");
    await until("document.body && !document.body.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }))",
      `Production context menu must be disabled on ${path}`);
  }
  assert.deepEqual(errors, [], "No browser runtime errors in production pages");
  console.log("UI smoke passed: crawl library, saved sessions, deletion and comparison, workspace query isolation, workbench and paging, links and retries, filters and exports, live updates, keyboard navigation, themes and contrast, responsive layouts, settings search/groups/persistence/recovery, rendering availability/retry/result preservation, aligned checkboxes, splash startup/failure/themes, production context-menu suppression, quit confirmation/cancellation/focus/retry, and release checks/reminders/downloads/offline recovery.");
} finally {
  socket?.close();
  if (browser?.exitCode === null && browser.signalCode === null) {
    browser.kill();
    await new Promise((resolve) => browser.once("exit", resolve));
  }
  await server.close();
  if (productionServer) await new Promise((resolve) => productionServer.httpServer.close(resolve));
  await rm(profile, { recursive: true, force: true, maxRetries: 3 });
}
