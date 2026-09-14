// Linux desktop smoke: real WebKitGTK, Tauri IPC, crawler and SQLite; no browser mocks.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { constants } from "node:fs";
import { access, chmod, mkdir, mkdtemp, open, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { delimiter, dirname, isAbsolute, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setTimeout as delay } from "node:timers/promises";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: { app: { type: "string" }, "check-tools": { type: "boolean", default: false } } });
const toolsOnly = values["check-tools"];
assert.equal(process.platform, "linux", "This native smoke currently supports Linux only.");
const toolsDirectory = process.env.FF_NATIVE_TOOLS ?? "/tmp/ferrous-native-e2e-tools";
const application = resolve(values.app ?? process.env.FF_NATIVE_APP ?? "target/release/ferrous-frog");
const artifacts = await mkdtemp(join(tmpdir(), "ferrous-native-smoke-"));
const children = [];
const logFiles = [];
let session;
let driverUrl;
let appPid;
let fixture;
let success = false;
const requests = [];

async function executable(override, name, fallback) {
  for (const path of [override, ...process.env.PATH.split(delimiter).map((directory) => join(directory, name)), fallback].filter(Boolean)) {
    try { await access(path, constants.X_OK); return resolve(path); } catch { /* Try the next explicit location. */ }
  }
  throw new Error(`${name} is unavailable. See docs/NATIVE_TESTING.md; set ${override ? "the binary path" : "FF_NATIVE_TOOLS"} after preparing local tools.`);
}

async function start(command, commandArgs, options, logName, extraPipe = false) {
  const log = await open(join(artifacts, logName), "a");
  logFiles.push(log);
  const child = spawn(command, commandArgs, {
    ...options, detached: true,
    stdio: extraPipe ? ["ignore", log.fd, log.fd, "pipe"] : ["ignore", log.fd, log.fd],
  });
  children.push(child);
  await new Promise((resolve, reject) => { child.once("spawn", resolve); child.once("error", reject); });
  return child;
}

async function until(work, description, milliseconds = 30_000) {
  const deadline = Date.now() + milliseconds;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await work(); if (result) return result; } catch (error) { lastError = error; }
    await delay(100);
  }
  throw new Error(`Timed out: ${description}${lastError ? ` (${lastError.message})` : ""}`);
}

async function freePort() {
  const server = createServer();
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function webdriver(method, path, body, timeout = 15_000) {
  const response = await fetch(`${driverUrl}${path}`, {
    method, headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(timeout),
  });
  const result = await response.json();
  if (!response.ok || result.value?.error) throw new Error(`${method} ${path}: ${result.value?.message ?? JSON.stringify(result)}`);
  return result.value;
}

const evaluate = (script, args = []) => webdriver("POST", `/session/${session}/execute/sync`, { script, args });
const element = (selector) => evaluate(`return [...document.querySelectorAll(${JSON.stringify(selector)})].find((candidate) => {
  const style = getComputedStyle(candidate);
  const rect = candidate.getBoundingClientRect();
  return !candidate.hidden && style.display !== "none" && style.visibility !== "hidden" && Number(style.opacity) > 0
    && rect.width > 0 && rect.height > 0 && rect.top < innerHeight && rect.bottom > 0 && rect.left < innerWidth && rect.right > 0;
})`);
const elementId = (element) => element["element-6066-11e4-a52e-4f735466cecf"];
async function click(selector) {
  const target = await until(() => element(selector), `find ${selector}`);
  await webdriver("POST", `/session/${session}/element/${elementId(target)}/click`, {});
}
async function fill(selector, text) {
  const target = await until(() => element(selector), `find ${selector}`);
  const path = `/session/${session}/element/${elementId(target)}`;
  await webdriver("POST", `${path}/clear`, {});
  await webdriver("POST", `${path}/value`, { text });
}

async function clickText(selector, text) {
  const target = await until(() => evaluate(`const target = [...document.querySelectorAll(arguments[0])].find(item => item.textContent.trim() === arguments[1] && !item.disabled);
    target?.scrollIntoView({ block: 'center' }); return target;`, [selector, text]), `find ${text}`);
  await webdriver("POST", `/session/${session}/element/${elementId(target)}/click`, {});
}

async function openAuditReports() {
  await click('[aria-label="More tools"]');
  await clickText('[role="menuitem"]', "Audit reports");
  await until(() => element('.audit-report-modal [aria-label="Audit report title"]'), "the native audit report workspace opens");
}

async function launch() {
  const result = await webdriver("POST", "/session", {
    capabilities: { alwaysMatch: { "tauri:options": { application: join(artifacts, "launch-app") } } },
  }, 60_000);
  session = result.sessionId;
  assert.ok(session, "The native driver must return a session.");
  await webdriver("POST", `/session/${session}/timeouts`, { implicit: 0, script: 10_000, pageLoad: 30_000 });
  await until(async () => {
    const handles = await webdriver("GET", `/session/${session}/window/handles`);
    for (const handle of handles) {
      try {
        await webdriver("POST", `/session/${session}/window`, { handle });
        await evaluate("localStorage.setItem('ferrous-frog-update-reminder-until', String(Date.now() + 86400000))");
        if (await evaluate("return Boolean(document.querySelector('.crawl-home'))")) return true;
      } catch { /* The startup splash may close while we enumerate its handle. */ }
    }
    return false;
  }, "the native main window opens its crawl library", 45_000);
  assert.ok(await evaluate("return Boolean(window.__TAURI_INTERNALS__)"), "This must be a native Tauri webview.");
  appPid = Number((await readFile(join(artifacts, "app.pid"), "utf8")).trim());
  assert.ok(Number.isInteger(appPid) && appPid > 1, "The native application PID must be recorded.");
}

function appRunning() {
  if (!appPid) return false;
  try { process.kill(appPid, 0); return true; } catch (error) { if (error.code === "ESRCH") return false; throw error; }
}

async function quit(cancelFirst = false) {
  const openQuit = async () => {
    await click('[aria-label="More tools"]');
    const item = await evaluate("return [...document.querySelectorAll('[role=menuitem]')].find(item => item.textContent.trim() === 'Quit')");
    assert.ok(item, "The real tools menu must expose Quit.");
    await webdriver("POST", `/session/${session}/element/${elementId(item)}/click`, {});
    await until(() => evaluate("return Boolean(document.querySelector('.quit-modal[role=alertdialog]'))"), "quit confirmation");
  };
  await openQuit();
  if (cancelFirst) {
    await click(".quit-modal button:not(.destructive)");
    await until(() => evaluate("return !document.querySelector('.quit-modal')"), "cancel keeps the application open");
    assert.ok(appRunning(), "Cancelling quit must keep the native process alive.");
    await openQuit();
  }
  try { await click(".quit-modal button.destructive"); } catch (error) {
    // The native process can close the driver session before replying to its final click.
    if (!error.message.includes("Session terminated without a reply")) throw error;
  }
  await until(() => !appRunning(), "confirmed quit drains report work and exits the native application", 30_000);
  appPid = undefined;
  await webdriver("DELETE", `/session/${session}`).catch(() => {});
  session = undefined;
}

function savedRecords() {
  const data = join(artifacts, "data", "com.ferrousfrog.seospider");
  const index = new DatabaseSync(join(data, "ferrous-frog-sessions.sqlite3"), { readOnly: true });
  try {
    const sessions = index.prepare("SELECT id, database_path, status, crawled FROM crawl_sessions").all();
    assert.equal(sessions.length, 1, "The smoke must create exactly one independent saved crawl.");
    const saved = sessions[0];
    assert.ok(isAbsolute(saved.database_path) && saved.database_path.startsWith(`${data}/`), "The crawl must stay inside isolated app data.");
    const database = new DatabaseSync(saved.database_path, { readOnly: true });
    try {
      const rows = database.prepare("SELECT url, status_code, status_text FROM crawl_records ORDER BY url").all();
      return { saved, rows };
    } finally { database.close(); }
  } finally { index.close(); }
}

async function savedReport() {
  const directory = join(artifacts, "data", "com.ferrousfrog.seospider", "audit-reports");
  const files = await readdir(directory);
  const reports = files.filter((name) => name.endsWith(".sqlite3"));
  assert.equal(reports.length, 1, "The smoke must create exactly one frozen report.");
  assert.ok(!files.some((name) => name.endsWith(".ai")), "AI must remain off; no generation sidecar may be created.");
  assert.deepEqual(files, reports, "Completed preparation must leave no temporary report files.");
  const path = await realpath(join(directory, reports[0]));
  assert.ok(path.startsWith(`${directory}/`), "The report must stay inside isolated app data.");
  const database = new DatabaseSync(path, { readOnly: true });
  try {
    const summary = JSON.parse(database.prepare("SELECT payload FROM audit_report WHERE id=1").get().payload);
    const evidence = database.prepare("SELECT payload FROM audit_evidence ORDER BY finding_id, sequence").all().map((row) => JSON.parse(row.payload));
    const records = database.prepare("SELECT url, status_code, status_text FROM crawl_records ORDER BY url").all();
    assert.equal(summary.sourceRecords, 4);
    assert.equal(summary.scopeRecords, 4);
    assert.equal(summary.request.sourceStatus, "completed");
    assert.equal(records.length, 4, "The frozen report must retain unaffected and blocked source records too.");
    assert.ok(evidence.length > 0, "The report must retain measured evidence.");
    return { path, summary, records, evidence };
  } finally { database.close(); }
}

async function verifyReportPackage(indexPath, frozen) {
  const isolatedData = await realpath(join(artifacts, "data"));
  const actualIndex = await realpath(indexPath);
  assert.ok(actualIndex.startsWith(`${isolatedData}/`), "Native exports must stay inside the isolated XDG data/download directory.");
  assert.equal(actualIndex.split("/").at(-1), "index.html");
  const directory = dirname(actualIndex);
  const manifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"));
  assert.equal(manifest.status, "complete");
  assert.equal(manifest.reportId, frozen.summary.request.id);
  assert.equal(manifest.findingCount, frozen.summary.findingCount);
  assert.equal(manifest.evidenceRows, frozen.evidence.length);
  assert.equal(manifest.aiAnnotatedFindings, 0);
  assert.ok(!manifest.aiOverview, "The offline-only workflow must not create an AI overview.");
  assert.deepEqual(manifest.summary, frozen.summary);
  for (const filename of manifest.files) {
    const path = await realpath(join(directory, filename));
    assert.ok(path.startsWith(`${directory}/`), "Manifest assets must remain inside the published folder.");
    await access(path, constants.R_OK);
  }
  const index = await readFile(actualIndex, "utf8");
  for (const finding of manifest.findings) {
    const rows = frozen.evidence.filter((row) => row.findingId === finding.findingId);
    assert.equal(finding.counts.occurrences, rows.length);
    assert.equal(finding.htmlPages.length, Math.ceil(rows.length / 1_000));
    assert.ok(index.includes(finding.csvFile) && index.includes(finding.htmlPages[0]), "The index must link to the full CSV and evidence pages.");
  }
  // Python's standard CSV reader handles quoted JSON and multiline cells without a second parser.
  execFileSync("python3", ["-c", `import csv, html, json, pathlib, sys
fixture = json.load(sys.stdin)
root = pathlib.Path(fixture['directory'])
count = 0
for finding in fixture['manifest']['findings']:
    expected = [row for row in fixture['evidence'] if row['findingId'] == finding['findingId']]
    final_page = html.unescape((root / finding['htmlPages'][-1]).read_text(encoding='utf-8'))
    assert expected[-1]['id'] in final_page and expected[-1]['originalUrl'] in final_page, 'The final stored evidence row must be reachable in HTML'
    with (root / finding['csvFile']).open(newline='', encoding='utf-8') as source:
        rows = list(csv.DictReader(source))
    assert len(rows) == len(expected), 'CSV must retain every evidence row'
    assert [json.loads(row['evidenceJson']) for row in rows] == expected, 'CSV evidence must exactly match the frozen report, including its final row'
    count += len(rows)
assert count == fixture['manifest']['evidenceRows'], 'CSV and manifest totals must agree'
`], { input: JSON.stringify({ directory, manifest, evidence: frozen.evidence }), encoding: "utf8" });
  assert.ok(!(await readdir(dirname(directory))).includes(".preparing"), "Published exports must leave no preparation folder.");
  return { indexPath: actualIndex, findingCount: manifest.findingCount, evidenceRows: manifest.evidenceRows };
}

try {
  const xvfb = await executable(process.env.XVFB_BIN, "Xvfb", join(toolsDirectory, "extracted/usr/bin/Xvfb"));
  const nativeDriver = await executable(process.env.WEBKIT_WEBDRIVER, "WebKitWebDriver", join(toolsDirectory, "extracted/usr/bin/WebKitWebDriver"));
  const tauriDriver = await executable(process.env.TAURI_DRIVER, "tauri-driver", join(toolsDirectory, "tauri/bin/tauri-driver"));
  if (!toolsOnly) await access(application, constants.X_OK).catch(() => {
    throw new Error(`The desktop executable is unavailable at ${application}. Build it with make build or pass --app; see docs/NATIVE_TESTING.md.`);
  });
  for (const directory of ["data", "config", "cache", "runtime"]) await mkdir(join(artifacts, directory), { mode: 0o700 });
  await mkdir(join(artifacts, "data", "Downloads"), { mode: 0o700 });
  await writeFile(join(artifacts, "config", "user-dirs.dirs"), `XDG_DOWNLOAD_DIR="${join(artifacts, "data", "Downloads")}"\n`);
  await writeFile(join(artifacts, "launch-app"), '#!/bin/sh\nprintf "%s\\n" "$$" > "$FF_NATIVE_PID_FILE"\nexec "$FF_NATIVE_APP"\n');
  await chmod(join(artifacts, "launch-app"), 0o700);
  const display = await start(xvfb, ["-displayfd", "3", "-screen", "0", "1280x960x24", "-nolisten", "tcp"], {}, "xvfb.log", true);
  let displayNumber = "";
  display.stdio[3].on("data", (value) => { displayNumber += value.toString(); });
  await until(() => /^\d+\n/.test(displayNumber), "the private Xvfb display starts", 10_000);
  const port = await freePort();
  let nativePort = await freePort();
  while (nativePort === port) nativePort = await freePort();
  driverUrl = `http://127.0.0.1:${port}`;
  await start("dbus-run-session", ["--", tauriDriver, "--port", String(port), "--native-port", String(nativePort), "--native-host", "127.0.0.1", "--native-driver", nativeDriver], {
    env: {
      ...process.env, DISPLAY: `:${displayNumber.trim()}`, GDK_BACKEND: "x11",
      XDG_DATA_HOME: join(artifacts, "data"), XDG_CONFIG_HOME: join(artifacts, "config"),
      XDG_CACHE_HOME: join(artifacts, "cache"), XDG_RUNTIME_DIR: join(artifacts, "runtime"),
      TAURI_AUTOMATION: "1", TAURI_WEBVIEW_AUTOMATION: "true", LIBGL_ALWAYS_SOFTWARE: "1",
      FF_NATIVE_APP: application, FF_NATIVE_PID_FILE: join(artifacts, "app.pid"),
    },
  }, "driver.log");
  await until(() => webdriver("GET", "/status"), "tauri-driver and WebKitWebDriver are ready", 10_000);
  console.log("Native tools ready: private Xvfb, WebKitWebDriver and tauri-driver.");
  if (!toolsOnly) {
    fixture = createServer((request, response) => {
      requests.push({ path: request.url, method: request.method, userAgent: request.headers["user-agent"], at: Date.now() });
      if (request.url === "/robots.txt") {
        response.writeHead(200, { "content-type": "text/plain" });
        response.end("User-agent: *\nDisallow: /private\nCrawl-delay: 1\n");
      } else {
        const status = request.url === "/" || request.url === "/ok" ? 200 : 404;
        response.writeHead(status, { "content-type": "text/html; charset=utf-8" });
        const links = request.url === "/" ? '<a href="/ok">Good page</a><a href="/missing">Broken page</a><a href="/private">Blocked page</a>' : "";
        response.end(`<!doctype html><html><head><title>Native smoke ${request.url}</title><meta name="description" content="Local native smoke fixture"></head><body><h1>Native smoke</h1>${links}</body></html>`);
      }
    });
    await new Promise((resolve, reject) => { fixture.once("error", reject); fixture.listen(0, "127.0.0.1", resolve); });
    const origin = `http://127.0.0.1:${fixture.address().port}`;
    await launch();
    assert.equal(requests.length, 0, "Opening the library must not start crawl requests.");
    await fill('.crawl-launcher [aria-label="Crawl URL"]', `${origin}/`);
    await click('[data-action="start-new-crawl"]');
    await until(() => evaluate("return document.querySelector('.status-main strong')?.textContent === 'Finished'"), "the real crawler finishes", 60_000);
    await until(() => evaluate("return [...document.querySelectorAll('.data-table tbody tr')].some(row => row.textContent.includes('/missing') && row.textContent.includes('404'))"), "the planted 404 appears in the native results grid");
    assert.ok(requests.some((request) => request.path === "/robots.txt"), "The crawler must fetch robots.txt.");
    assert.ok(!requests.some((request) => request.path?.startsWith("/private")), "The crawler must respect robots disallow.");
    const pages = requests.filter((request) => ["/", "/ok", "/missing"].includes(request.path));
    assert.equal(pages.length, 3, "Each permitted fixture page must be fetched once.");
    assert.ok(pages.every((request) => request.userAgent?.length > 0), "Every crawl request must have a User-Agent.");
    for (let index = 1; index < pages.length; index++) assert.ok(pages[index].at - pages[index - 1].at >= 700, "The one-second robots crawl delay must pace requests.");
    const before = savedRecords();
    assert.ok(before.rows.some((row) => row.url === `${origin}/missing` && row.status_code === 404));
    assert.ok(before.rows.some((row) => row.url === `${origin}/private` && row.status_text === "Blocked by robots.txt"));
    console.log(`Native crawl complete: ${before.rows.length} SQLite records; robots and 404 verified.`);
    await quit(true);
    const requestCount = requests.length;
    await launch();
    await click('[aria-label="Saved crawls"][aria-controls="crawl-history-panel"]');
    await until(() => element('[data-action="open-saved-crawl"]'), "the saved SQLite crawl appears after process restart");
    await click('[data-action="open-saved-crawl"]');
    await until(() => evaluate("return [...document.querySelectorAll('.data-table tbody tr')].some(row => row.textContent.includes('/missing') && row.textContent.includes('404'))"), "opening the saved crawl restores native results");
    await delay(1_200);
    assert.equal(requests.length, requestCount, "Reopening a saved crawl must not send new requests.");
    const after = savedRecords();
    assert.equal(after.saved.id, before.saved.id);
    assert.deepEqual(after.rows, before.rows);
    assert.equal(after.rows.length, 4, "The completed source fixture must contain all four records.");

    await openAuditReports();
    await fill('[aria-label="Audit report title"]', "Native saved audit report");
    assert.equal(await evaluate('return document.querySelector(\'[aria-label="Use current audit filter"]\').checked'), false);
    await clickText(".audit-report-launcher button", "Create report");
    await until(() => evaluate("return document.querySelector('.audit-report-summary h3')?.textContent === 'Native saved audit report' && Boolean(document.querySelector('[data-audit-finding-id]'))"), "the native report freezes the completed crawl and displays measured findings");
    const frozen = await savedReport();
    assert.equal(frozen.summary.request.sourceSessionId, after.saved.id);
    assert.deepEqual(frozen.records, after.rows);
    assert.ok(frozen.evidence.some((row) => row.findingId === "response.clientError" && row.originalUrl === `${origin}/missing`));
    assert.ok(await evaluate("return Boolean(document.querySelector('.audit-report-ai')) && !document.querySelector('.audit-ai-preview')"), "AI must remain optional and unapproved.");
    await click('[title="Close audit reports"]');
    await quit();

    await launch();
    await openAuditReports();
    await click(`[data-audit-report-id="${frozen.summary.request.id}"]`);
    await until(() => evaluate("return document.querySelector('.audit-report-summary h3')?.textContent === 'Native saved audit report' && Boolean(document.querySelector('[data-audit-finding-id]'))"), "the saved report reopens after a native process restart");
    assert.deepEqual(await savedReport(), frozen, "Reopening must retain the same frozen report and every evidence row.");
    await evaluate('document.querySelector(\'[data-audit-finding-id="response.clientError"]\').scrollIntoView({ block: "center" })');
    await click('[data-audit-finding-id="response.clientError"]');
    await until(() => evaluate("return [...document.querySelectorAll('[data-audit-evidence-id]')].some(row => row.textContent.includes('/missing'))"), "the saved 404 evidence is available through native paging");
    await clickText(".audit-report-summary button", "Export complete report folder");
    const indexPath = await until(() => evaluate("const status = [...document.querySelectorAll('.audit-report-summary [role=status]')].find(item => item.textContent.startsWith('Report folder: ')); return status && [...status.childNodes].filter(node => node.nodeType === Node.TEXT_NODE).map(node => node.textContent).join('').replace(/^Report folder: /, '').trim();"), "the native report package publishes successfully", 60_000);
    const exported = await verifyReportPackage(indexPath, frozen);
    assert.equal(requests.length, requestCount, "Report preparation, restart, evidence paging and export must not fetch the source again.");
    assert.deepEqual(savedRecords(), after, "Report operations must not start another crawl or change its saved records.");
    await savedReport(); // No AI sidecar or partial report may appear during the export.
    await click('[title="Close audit reports"]');
    await quit();
    assert.deepEqual(await savedReport(), frozen, "Clean shutdown must preserve the complete saved report.");
    await writeFile(join(artifacts, "report.json"), JSON.stringify({ application, saved: after.saved, rows: after.rows, report: frozen.summary, exported, requests }, null, 2));
    console.log("Native smoke passed: polite crawl → saved reopen → frozen report → report restart/reopen → complete offline export → clean quit.");
  }
  success = true;
} catch (error) {
  await writeFile(join(artifacts, "failure.json"), JSON.stringify({ application, error: error.message, requests }, null, 2));
  if (session) {
    try { await writeFile(join(artifacts, "failure.png"), Buffer.from(await webdriver("GET", `/session/${session}/screenshot`), "base64")); } catch { /* Preserve other diagnostics if the window has closed. */ }
    try { await writeFile(join(artifacts, "failure.html"), await webdriver("GET", `/session/${session}/source`)); } catch { /* A failed launch may have no document. */ }
  }
  console.error(`Native smoke failed. Diagnostics: ${artifacts}`);
  throw error;
} finally {
  if (session) await webdriver("DELETE", `/session/${session}`, undefined, 2_000).catch(() => {});
  if (appRunning()) { try { process.kill(appPid, "SIGTERM"); } catch {} }
  for (const child of children.reverse()) {
    try { process.kill(-child.pid, "SIGTERM"); } catch {}
  }
  await delay(200);
  for (const child of children) { try { process.kill(-child.pid, "SIGKILL"); } catch {} }
  if (fixture) { fixture.closeAllConnections(); await new Promise((resolve) => fixture.close(resolve)); }
  for (const file of logFiles) await file.close();
  if (success && process.env.FF_NATIVE_KEEP_ARTIFACTS !== "1") await rm(artifacts, { recursive: true, force: true });
  else console.log(`Native smoke artifacts: ${artifacts}`);
}
