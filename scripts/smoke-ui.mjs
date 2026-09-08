// Run with: node scripts/smoke-ui.mjs (Chrome/Chromium required; CHROME_BIN overrides its path).
// This tests the real React screen against Tauri's IPC mock. Rust integration tests cover the engine.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createServer } from "vite";

function setupFixture(mockIPC, emit) {
  window.isTauri = true;
  window.testStartupTheme = document.documentElement.classList.contains("dark") ? "dark" : "light";
  const summary = Object.fromEntries(`total internal external success redirects clientErrors serverErrors
    noResponse broken nearDuplicates indexable nonIndexable titleMissing titleDuplicate metaMissing
    metaDuplicate h1Missing h1Duplicate h2Missing h2Duplicate canonicalMissing canonicalMultiple noindex
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
  window.testQueries = [];
  window.testLinkQueries = [];
  window.testLinkDelays = {};
  window.testDuplicates = false;
  window.testEmptyDataset = false;
  window.testSearchDelays = {};
  window.testSettingsWrites = 0;
  window.testStartupCalls = 0;
  window.testQuitCalls = 0;
  window.testUpdateChecks = 0;
  window.testUpdateFailure = new URL(location.href).searchParams.get("updates") === "error";
  window.testUpdateResult = {
    currentVersion: "0.1.0",
    update: new URL(location.href).searchParams.get("updates") === "available"
      ? { version: "0.2.0", releaseUrl: "https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/v0.2.0" }
      : null,
  };
  window.testOpenedUrls = [];
  const saveItem = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {
    if (key === "ferrous-frog-settings") window.testSettingsWrites++;
    if (window.testSettingsWriteFailure) throw new DOMException("Storage full", "QuotaExceededError");
    return saveItem.call(this, key, value);
  };
  window.testEmit = (payload) => emit("crawl-event", payload);
  window.testRequestQuit = () => emit("quit-requested");
  mockIPC(async (cmd, args) => {
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
      window.testStartupHadGrid = Boolean(document.querySelector('.data-table tbody tr:not(.virtual-spacer)'));
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
      if (window.testEmptyDataset) matching = [];
      if (query.globalSearch) matching = matching.filter((row) => row.finalUrl.includes(query.globalSearch));
      if (query.sortBy) matching = [...matching].reverse();
      const response = { rows: matching.slice(query.offset, query.offset + query.limit), total: matching.length,
        summary: { ...summary, titleDuplicate: window.testDuplicates ? 2 : 0 } };
      await new Promise((resolve) => setTimeout(resolve, window.testSearchDelays[query.globalSearch] ?? 15));
      return response;
    }
    if (cmd === "get_recovery_state") return { recoverable: false, queued: 0, seen: 0, crawled: 0 };
    if (cmd === "get_image_assets") return { images: [], total: 0 };
    if (cmd === "get_crawl_path") return { found: false, hops: [], capped: false };
    if (cmd === "get_crawl_graph") return {
      nodes: ["root", "offline", "blocked", "pending"].map((label) => ({
        url: `https://example.test/${label}`, label, statusCode: label === "root" ? 200 : null,
        crawled: label === "root" || label === "offline", classification: "internal", depth: 1,
        inlinkCount: 1, outlinkCount: 0,
      })),
      edges: ["offline", "blocked", "pending"].map((label, id) => ({
        id, sourceUrl: "https://example.test/root", targetUrl: `https://example.test/${label}`,
        anchorText: label, linkType: "internal", sourceStatusCode: 200, targetStatusCode: null,
        sourceDepth: 1, targetDepth: 1, rel: "", relNofollow: false,
      })), totalNodes: 4, totalEdges: 3,
    };
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
    if (cmd === "list_crawl_sessions") return [];
    if (cmd === "list_config_profiles") return window.testProfile ? [window.testProfile] : [];
    if (cmd === "load_config_profile") return window.testProfile;
    if (cmd === "get_database_location") return { path: "/tmp/test-crawl.sqlite3" };
    if (cmd === "start_crawl") {
      window.testStartedSeed = args.config.startUrl;
      window.testStartedList = args.config.listUrls;
      window.testEmit({ kind: "started" });
      return;
    }
    if (cmd === "open_database_path") {
      if (args.path === "/invalid/crawl.sqlite3") throw new Error("Cannot open this database");
      window.testEmptyDataset = true;
      return { path: args.path };
    }
    if (cmd === "get_search_console_credential_status") return { tokenSaved: false, keyringAvailable: true };
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
const errors = [];
const server = await createServer({
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
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    assert.ok(!result.exceptionDetails, result.exceptionDetails?.exception?.description);
    return result.result.value;
  };
  const until = async (expression, message) => {
    for (let i = 0; i < 100; i++) {
      if (await evaluate(`Boolean(${expression})`)) return;
      await delay(50);
    }
    assert.fail(message);
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
  const fill = (selector, value) => evaluate(`(() => { const input = document.querySelector(${JSON.stringify(selector)});
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value').set.call(input, ${JSON.stringify(value)});
    input.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  const settingsTab = async (label) => {
    await evaluate(`[...document.querySelectorAll('.settings-tabs button')].find((button) => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
  };
  const setting = (label) => `.settings-section:not([hidden]) label[data-test-setting=${JSON.stringify(label)}] input`;
  const markSetting = (label) => evaluate(`[...document.querySelectorAll('.settings-section:not([hidden]) label')].find((item) => item.firstChild.textContent.trim() === ${JSON.stringify(label)}).setAttribute('data-test-setting', ${JSON.stringify(label)})`);
  const toggleSetting = (label) => evaluate(`[...document.querySelectorAll('.settings-section:not([hidden]) .checkbox-field')].find((item) => item.textContent.trim() === ${JSON.stringify(label)}).querySelector('[role="checkbox"]').click()`);
  const savedSettings = () => evaluate("JSON.parse(localStorage.getItem('ferrous-frog-settings'))");
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
  const reloadApp = async () => {
    await evaluate("window.testBeforeReload = true");
    await cdp("Page.reload");
    await until("!window.testBeforeReload && window.testStartupTheme && document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "The app should reload with its saved preference");
  };
  await systemTheme("dark");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__` });
  await until("document.querySelector('.data-table tbody tr:not(.virtual-spacer)')", "The real grid should render fixture rows");
  await until("testStartupCalls === 1 && testStartupHadGrid", "Splash completion must follow the first rendered results and run only once");
  assert.ok(await evaluate("Boolean(document.querySelector('.detail-empty'))"), "The URL inspector must reserve a stable panel before a URL is selected");
  assert.ok(await evaluate("document.querySelector('.data-table tbody tr:not(.virtual-spacer)').getBoundingClientRect().height <= 30 && document.querySelector('.grid').getBoundingClientRect().top < 155"), "The workbench should prioritize compact result rows and leave room for the URL inspector");
  assert.ok(await evaluate("document.querySelector('.overview-panel').getBoundingClientRect().width >= 300"), "A fresh install should use a readable overview width instead of treating a missing saved width as zero");
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
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(process.env.UI_SCREENSHOT, Buffer.from(shot.data, "base64"));
  }
  await openTools();
  assert.ok(await evaluate("[...document.querySelectorAll('[role=\"menuitemradio\"]')].find((item) => item.textContent.includes('System Theme')).getAttribute('aria-checked') === 'true'"), "The appearance menu must identify System as the active preference");
  if (process.env.UI_SCREENSHOT) {
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(`${process.env.UI_SCREENSHOT}.appearance.png`, Buffer.from(shot.data, "base64"));
  }
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await evaluate("document.querySelector('.grid').scrollLeft = 640");
  await delay(100);
  assert.ok(await evaluate("(() => { const grid = document.querySelector('.grid').getBoundingClientRect(); const url = document.querySelector('.data-table tbody tr:not(.virtual-spacer) td:nth-child(2)').getBoundingClientRect(); return url.left >= grid.left && url.right <= grid.right; })()"), "The URL must stay visible when scrolling a wide results grid horizontally");
  await evaluate("document.querySelector('.grid').scrollLeft = 0");
  await click('[data-category="Page titles"]');
  await until("document.querySelector('[aria-label=\"Audit view\"]').value === 'all' && document.querySelector('.data-table thead').textContent.includes('Title length')", "A category should show all URLs with relevant columns before narrowing to an issue");
  await click('[data-category="Crawl overview"]');
  await click('[aria-label="Toggle audit views"]');
  await until("document.querySelector('.issue-sidebar')", "The complete audit tree must remain available");
  assert.ok(await contrast('.issue-sidebar > p', '.issue-sidebar') >= 4.5, "Dark theme secondary text must remain readable on panels");
  assert.ok(await evaluate("Boolean(document.querySelector('[aria-label=\"Next page\"]'))"), "Results need a next-page control; the first 1,000 rows must not be a dead end");
  await click('[aria-label="Last page"]');
  await until("document.querySelector('.data-table tbody')?.textContent.includes('page-1001')", "Last page must load its first URL");
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
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
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
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
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
  await click('#inspection-tab-issues');
  await until("document.querySelector('#inspection-panel-issues:not([hidden]) .issue-summary-table')", "The right inspector must show issue counts");
  await evaluate("[...document.querySelectorAll('.issue-summary-table button')].find((button) => button.textContent.includes('Duplicate Titles')).click()");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Audit view\"]').value"), "titleDuplicate", "An issue must drill into its real engine filter");
  if (process.env.UI_SCREENSHOT) {
    await click('[aria-label="Close audit views"]');
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(`${process.env.UI_SCREENSHOT}.workbench.png`, Buffer.from(shot.data, "base64"));
    await click('[aria-label="Toggle audit views"]');
  }
  await click('.selected-links [title="Open full link report"]');
  await until("document.querySelector('.link-report-modal [aria-label=\"Last page\"]')", "URL details must give direct access to paged inlinks");
  await click('.link-report-modal [aria-label="Last page"]');
  await until("document.querySelector('.link-report-modal')?.textContent.includes('Link 1205')", "Link reports must reach beyond the first 500 edges");
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
  await click('#inspection-tab-overview');
  await menuItem("Crawl Graph");
  await until("document.querySelector('[title=\"Show broken source-to-target edges with both endpoint nodes\"]')?.textContent.includes('(1)')", "Only the known connection failure should count as a broken graph edge");
  await until("document.querySelector('.graph-svg circle')", "The crawl graph should render nodes");
  const darkNodeColor = await evaluate("document.querySelector('.graph-svg circle').getAttribute('fill')");
  await systemTheme("light");
  await until(`document.querySelector('.graph-svg circle')?.getAttribute('fill') !== ${JSON.stringify(darkNodeColor)}`, "An open graph must update its cached node colors when the system theme changes");
  await systemTheme("dark");
  await until(`document.querySelector('.graph-svg circle')?.getAttribute('fill') === ${JSON.stringify(darkNodeColor)}`, "Graph colors must also follow a switch back to dark");
  await click('[title="Close graph"]');
  await menuItem("Light Theme");
  await until("!document.documentElement.classList.contains('dark')", "Theme control should switch to light");
  await systemTheme("light");
  await systemTheme("dark");
  await delay(100);
  assert.ok(await evaluate("!document.documentElement.classList.contains('dark')"), "An explicit light choice must override system changes");
  assert.ok(await contrast('.issue-sidebar > p', '.issue-sidebar') >= 4.5, "Light theme secondary text must remain readable on panels");
  if (process.env.UI_SCREENSHOT) {
    const shot = await cdp("Page.captureScreenshot");
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
      const shot = await cdp("Page.captureScreenshot");
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
  for (const selector of ['.export-trigger', '.overview-toggle', '.settings-trigger']) {
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-label')`), "Icon-only toolbar actions must retain accessible names");
  }
  if (process.env.UI_SCREENSHOT) {
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(`${process.env.UI_SCREENSHOT}.mobile.png`, Buffer.from(shot.data, "base64"));
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Settings must open");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Storage').click()");
  await fill('[placeholder="/path/to/ferrous-frog.sqlite3"]', "/invalid/crawl.sqlite3");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Open Database').click()");
  await until("document.querySelector('.settings-modal [role=\"alert\"]')?.textContent.includes('Cannot open this database')", "A failed dialog action must show its error inside the active dialog");
  if (process.env.UI_SCREENSHOT) {
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(`${process.env.UI_SCREENSHOT}.settings.png`, Buffer.from(shot.data, "base64"));
  }
  await click('.settings-modal [aria-label="Dismiss error"]');
  await fill('[placeholder="/path/to/ferrous-frog.sqlite3"]', "/tmp/test-crawl.sqlite3");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Open Database').click()");
  await until("testEmptyDataset && document.querySelector('.detail-empty') && !document.querySelector('.detail-header')", "Opening a different crawl must clear the previous URL selection while retaining the inspector layout");
  await click('[title="Close settings"]');
  await fill('[aria-label="Seed URL"]', "");
  assert.ok(await evaluate("document.querySelector('[title=\"Start crawl\"]').disabled"), "An empty seed must not offer a runnable crawl");
  await fill('[aria-label="Seed URL"]', "https://keyboard.test/");
  await evaluate("document.querySelector('[aria-label=\"Seed URL\"]').focus()");
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" });
  await until("document.querySelector('[title=\"Pause crawl\"]') && testStartedSeed === 'https://keyboard.test/'", "Submitting the seed with Enter must start the crawl");
  assert.ok(await contrast('.destructive') >= 4.5, "The active Stop button must stay readable in light mode");
  await evaluate("testEmit({ kind: 'finished' })");
  await until("document.querySelector('[title=\"Start crawl\"]')", "The test crawl must finish before switching mode");
  await evaluate("(() => { const mode = document.querySelector('[aria-label=\"Crawl mode\"]'); mode.value = 'list'; mode.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await fill('[aria-label="Root URL"]', "");
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "List sources must remain configurable");
  await evaluate("[...document.querySelectorAll('.settings-modal button')].find((button) => button.textContent.trim() === 'Scope').click()");
  await fill('[aria-label="List URLs"]', "https://list.test/first");
  await click('[title="Close settings"]');
  await click('[title="Start crawl"]');
  await until("document.querySelector('[title=\"Pause crawl\"]') && testStartedSeed === '' && testStartedList[0] === 'https://list.test/first'", "List mode must allow a blank root when URL sources are configured");
  await evaluate("testEmit({ kind: 'finished' })");
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Settings must remain accessible after a crawl");
  await settingsTab("Crawl");
  await markSetting("Threads");
  await fill(setting("Threads"), "7");
  await markSetting("Max URLs");
  await fill(setting("Max URLs"), "2345");
  await toggleSetting("Respect robots.txt");
  await settingsTab("Resources");
  await toggleSetting("Images");
  await settingsTab("Query");
  await evaluate("document.querySelector('.settings-section:not([hidden]) .checkbox-root').click()");
  await settingsTab("Rendering");
  await toggleSetting("Render DOM");
  await markSetting("Wait after load");
  await fill(setting("Wait after load"), "800");
  await settingsTab("Extraction");
  await evaluate("document.querySelector('.settings-section:not([hidden]) .section-heading button').click()");
  await fill('[aria-label="Extractor pattern"]', 'main h1');
  await click('[title="Close settings"]');
  const settingsBeforeReload = await savedSettings();
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await until("document.querySelector('.settings-modal')", "Saved settings should be readable after reopening the app");
  await markSetting("Threads");
  await markSetting("Max URLs");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "7", "Concurrency must survive reopening without saving a named profile");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "2345", "Crawl limits must survive reopening");
  assert.ok(await evaluate("[...document.querySelectorAll('.checkbox-field')].find((item) => item.textContent.trim() === 'Respect robots.txt').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'false'"), "An explicit robots choice must not be replaced by the default during restore");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Crawl mode\"]').value"), "list", "Crawl mode must be restored");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Root URL\"]').value"), "", "An intentionally blank List root must override the older saved URL");
  await settingsTab("Resources");
  assert.ok(await evaluate("[...document.querySelectorAll('.settings-section:not([hidden]) .checkbox-field')].find((item) => item.textContent.trim() === 'Images').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), "Nested resource choices must be restored");
  await settingsTab("Rendering");
  await markSetting("Wait after load");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Wait after load"))}).value`), "800", "Rendering options must be restored");
  await settingsTab("Extraction");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Extractor pattern\"]').value"), "main h1", "Custom extractor definitions must be restored");
  await settingsTab("Scope");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"List URLs\"]').value"), "https://list.test/first", "List sources must be restored");
  await settingsTab("Storage");
  assert.equal(await evaluate("document.querySelector('.settings-section:not([hidden]) select').value"), "database", "Storage mode must be restored");
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
  await until("JSON.parse(localStorage.getItem('ferrous-frog-settings')).config.maxUrls === 8765", "Loading a profile must also update the automatic snapshot");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await markSetting("Max URLs");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "8765", "The last loaded profile must take effect after reopening without selecting it again");
  await evaluate("localStorage.setItem('ferrous-frog-settings', JSON.stringify({ version: 1, config: { maxUrls: 3210, resourceTypes: { images: true } } }))");
  await reloadApp();
  await click('[aria-label="Crawl settings"]');
  await markSetting("Max URLs");
  await markSetting("Threads");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Max URLs"))}).value`), "3210", "Older snapshots must retain their known values");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "4", "New or missing fields must use their defaults");
  for (const invalid of ['{invalid json', JSON.stringify({ version: 1, config: { listUrls: 'invalid array', respectRobots: false } })]) {
    await evaluate(`localStorage.setItem('ferrous-frog-settings', ${JSON.stringify(invalid)})`);
    await reloadApp();
    await until("document.querySelector('[role=\"alert\"]')?.textContent.includes('Saved settings could not be read')", "Malformed saved settings must fall back with visible feedback");
    await click('[aria-label="Crawl settings"]');
    assert.ok(await evaluate("[...document.querySelectorAll('.checkbox-field')].find((item) => item.textContent.trim() === 'Respect robots.txt').querySelector('[role=\"checkbox\"]').getAttribute('aria-checked') === 'true'"), "Invalid settings must not disable the polite defaults");
  }
  await markSetting("Threads");
  await fill(setting("Threads"), "7");
  await until("!document.querySelector('[aria-label=\"Dismiss settings error\"]')", "A successful edit must replace invalid settings and clear its error");
  await evaluate("testSettingsWriteFailure = true");
  await fill(setting("Threads"), "8");
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
  await until("!document.querySelector('[aria-label=\"Dismiss settings error\"]')", "Successful persistence must recover after a storage failure");
  assert.equal((await savedSettings()).config.concurrency, 9);
  assert.deepEqual(Object.keys(await savedSettings()).sort(), ['config', 'resumeCrawl', 'storageMode', 'version'], "Only preferences belong in the saved snapshot");
  for (const [tab, checkbox, field] of [["Crawl", "Respect robots.txt", "Dup bits"], ["Storage", "Resume database", "Mode"], ["Rendering", "Render DOM", "Backend"]]) {
    await settingsTab(tab);
    assert.ok(await evaluate(`(() => {
      const section = document.querySelector('.settings-section:not([hidden])');
      const box = [...section.querySelectorAll('.checkbox-field')].find((label) => label.textContent.trim() === ${JSON.stringify(checkbox)}).querySelector('[role="checkbox"]').getBoundingClientRect();
      const input = [...section.querySelectorAll('label')].find((label) => label.firstChild.textContent.trim() === ${JSON.stringify(field)}).querySelector('input, select').getBoundingClientRect();
      return Math.abs(box.top + box.height / 2 - input.top - input.height / 2) < 1;
    })()`), `${tab} checkboxes must line up with the neighboring input, below its label`);
    if (process.env.UI_SCREENSHOT) {
      const shot = await cdp("Page.captureScreenshot");
      await writeFile(`${process.env.UI_SCREENSHOT}.settings-${tab.toLowerCase()}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await settingsTab("Crawl");
  await markSetting("Threads");
  await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).focus(); testRequestQuit()`);
  await until("document.activeElement.textContent === 'No'", "Native quit must take keyboard focus above Settings");
  await click('.quit-actions button:first-child');
  await until(`document.activeElement === document.querySelector(${JSON.stringify(setting("Threads"))})`, "Canceling native quit must restore focus to the settings field");
  await click('[title="Close settings"]');
  await search("no-matching-url");
  await until("document.querySelector('.grid-empty')", "Use an empty filter while retaining a full memory crawl");
  await menuItem("Quit");
  await until("document.querySelector('[role=\"alertdialog\"]')", "Quit must ask for confirmation");
  await until("document.activeElement.textContent === 'No'", "Quit confirmation must focus the safe choice");
  assert.ok(await evaluate("document.querySelector('[role=\"alertdialog\"]').textContent.includes('In-memory results will be lost')"), "An empty results filter must not hide the unsaved crawl warning");
  if (process.env.UI_SCREENSHOT) {
    const shot = await cdp("Page.captureScreenshot");
    await writeFile(`${process.env.UI_SCREENSHOT}.quit.png`, Buffer.from(shot.data, "base64"));
  }
  await evaluate("[...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'No').click()");
  assert.equal(await evaluate("testQuitCalls"), 0, "No must keep the app open without stopping its crawl");
  await evaluate("testRequestQuit()");
  await until("document.querySelector('[role=\"alertdialog\"]')", "Native close requests must use the same confirmation");
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await until("!document.querySelector('[role=\"alertdialog\"]')", "Escape must cancel quitting");
  await evaluate("testEmit({ kind: 'started' })");
  await evaluate("testRequestQuit(); testRequestQuit()");
  await until("document.querySelector('[role=\"alertdialog\"]')?.textContent.includes('A crawl is running')", "Active crawls must be explained before quitting");
  assert.equal(await evaluate("document.querySelectorAll('[role=\"alertdialog\"]').length"), 1, "Repeated close requests must not stack confirmation dialogs");
  await evaluate("window.testQuitFailure = true; [...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'Yes').click()");
  await until("document.querySelector('[role=\"alertdialog\"] [role=\"alert\"]')?.textContent.includes('Could not stop')", "A failed quit must remain visible and allow retrying");
  await evaluate("window.testQuitFailure = false; [...document.querySelectorAll('[role=\"alertdialog\"] button')].find((button) => button.textContent === 'Yes').click()");
  await until("testQuitCalls === 2", "Only Yes may request quitting, including retries");
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
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "4", "Unavailable storage must use the default configuration");
  await fill(setting("Threads"), "6");
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(setting("Threads"))}).value`), "6", "Settings must remain editable when storage is unavailable");
  await cdp("Page.removeScriptToEvaluateOnNewDocument", { identifier: deniedStorage.identifier });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__?startup-error` });
  await until("window.testStartupCalls === 1 && document.querySelector('[role=\"alert\"]')?.textContent.includes('Initial results')", "A failed first query must still release the splash and display the error");
  await cdp("Emulation.setDeviceMetricsOverride", { width: 920, height: 640, deviceScaleFactor: 1, mobile: false });
  await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/__smoke__?updates=available` });
  await until("window.testStartupCalls === 1 && window.testStartupHadGrid", "Update checks must not hold the splash open");
  await until("document.querySelector('.update-modal')?.textContent.includes('0.2.0')", "A newer release must be announced after the workspace is ready");
  assert.ok(await evaluate("document.querySelector('.update-modal').textContent.includes('0.1.0')"), "The update notice must identify the installed version");
  assert.equal(await evaluate("testUpdateChecks"), 1, "Startup must make only one update request");
  assert.ok(await evaluate("document.activeElement?.textContent.includes('Remind me later')"), "The update notice must focus its non-disruptive action");
  if (process.env.UI_SCREENSHOT) {
    for (const theme of ["dark", "light"]) {
      await evaluate(`document.documentElement.classList.toggle('dark', ${theme === "dark"})`);
      const shot = await cdp("Page.captureScreenshot");
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
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
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
  await cdp("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await until("!document.querySelector('.update-modal')", "The update dialog must dismiss after retry");

  await cdp("Emulation.setDeviceMetricsOverride", { width: 400, height: 280, deviceScaleFactor: 1, mobile: false });
  for (const preference of ["light", "dark", "system"]) {
    await evaluate(`localStorage.setItem('ferrous-frog-theme', ${JSON.stringify(preference)}); window.testBeforeReload = true`);
    await cdp("Page.navigate", { url: `http://127.0.0.1:${server.httpServer.address().port}/splash.html` });
    await until("!window.testBeforeReload && document.querySelector('.splash-screen progress')", "The real splash page must load independently of React");
    assert.equal(await evaluate("document.documentElement.classList.contains('dark')"), preference !== "light", "The splash must use the saved appearance before displaying");
    assert.ok(await evaluate("document.documentElement.scrollWidth <= innerWidth && document.querySelector('.splash-screen [role=\"status\"]').getBoundingClientRect().bottom <= innerHeight"), "The splash must fit its native window size");
    assert.ok(await contrast('.splash-screen p', '.splash-screen') >= 4.5, "Splash status text must remain readable");
    if (process.env.UI_SCREENSHOT) {
      const shot = await cdp("Page.captureScreenshot");
      await writeFile(`${process.env.UI_SCREENSHOT}.splash-${preference}.png`, Buffer.from(shot.data, "base64"));
    }
  }
  await systemTheme("light");
  await until("!document.documentElement.classList.contains('dark')", "System appearance must also update on the splash screen");
  assert.deepEqual(errors, [], "No browser runtime errors");
  console.log("UI smoke passed: workbench and paging, links and retries, filters and exports, live updates, keyboard navigation, themes and contrast, responsive layouts, settings persistence and recovery, aligned checkboxes, splash startup/failure/themes, quit confirmation/cancellation/focus/retry, and release checks/reminders/downloads/offline recovery.");
} finally {
  socket?.close();
  if (browser?.exitCode === null && browser.signalCode === null) {
    browser.kill();
    await new Promise((resolve) => browser.once("exit", resolve));
  }
  await server.close();
  await rm(profile, { recursive: true, force: true, maxRetries: 3 });
}
