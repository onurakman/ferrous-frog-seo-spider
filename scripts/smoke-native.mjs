// Linux desktop smoke: real WebKitGTK, Tauri IPC, crawler and SQLite; no browser mocks.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { access, chmod, mkdir, mkdtemp, open, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { delimiter, isAbsolute, join, resolve } from "node:path";
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
const element = (selector) => webdriver("POST", `/session/${session}/element`, { using: "css selector", value: selector });
const elementId = (element) => element["element-6066-11e4-a52e-4f735466cecf"];
async function click(selector) {
  const target = await until(() => element(selector), `find ${selector}`);
  await webdriver("POST", `/session/${session}/element/${elementId(target)}/click`, {});
}
async function fill(selector, text) {
  const target = await element(selector);
  const path = `/session/${session}/element/${elementId(target)}`;
  await webdriver("POST", `${path}/clear`, {});
  await webdriver("POST", `${path}/value`, { text });
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
    // The native process can exit before WebDriver acknowledges the click.
    if (appRunning()) throw error;
  }
  await until(() => !appRunning(), "confirmed quit exits the native application", 20_000);
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

try {
  const xvfb = await executable(process.env.XVFB_BIN, "Xvfb", join(toolsDirectory, "extracted/usr/bin/Xvfb"));
  const nativeDriver = await executable(process.env.WEBKIT_WEBDRIVER, "WebKitWebDriver", join(toolsDirectory, "extracted/usr/bin/WebKitWebDriver"));
  const tauriDriver = await executable(process.env.TAURI_DRIVER, "tauri-driver", join(toolsDirectory, "tauri/bin/tauri-driver"));
  if (!toolsOnly) await access(application, constants.X_OK).catch(() => {
    throw new Error(`The desktop executable is unavailable at ${application}. Build it with make build or pass --app; see docs/NATIVE_TESTING.md.`);
  });
  for (const directory of ["data", "config", "cache", "runtime"]) await mkdir(join(artifacts, directory), { mode: 0o700 });
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
    await fill('[aria-label="Crawl URL"]', `${origin}/`);
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
    await quit();
    await writeFile(join(artifacts, "report.json"), JSON.stringify({ application, saved: after.saved, rows: after.rows, requests }, null, 2));
    console.log("Native smoke passed: library → polite crawl → persisted reopen → cancel/confirm quit → process exit.");
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
