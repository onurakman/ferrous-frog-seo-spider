// Generate synthetic input with:
// FF_AUDIT_OFFLINE_FIXTURE_DIR=/tmp/ff-audit-offline-fixture cargo test -p ferrous-frog-export --locked portable_report_exports_every
// Then: node scripts/check-offline-audit-report.mjs /tmp/ff-audit-offline-fixture
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

assert.ok(process.argv[2], "Pass the generated report package directory.");
const directory = resolve(process.argv[2]);
const manifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"));
const comparison = Boolean(manifest.baselineReportId);
const occurrences = (finding) => comparison ? finding.added + finding.persisting + finding.resolved + finding.notObserved : finding.counts.occurrences;
assert.equal(manifest.status, "complete");
assert.equal(manifest.findingCount, manifest.findings.length);
assert.equal(manifest.evidenceRows, manifest.findings.reduce((total, finding) => total + occurrences(finding), 0));
for (const file of manifest.files) {
  assert.equal(file, basename(file), "Package filenames must be internal basenames.");
  const content = await readFile(join(directory, file), "utf8");
  if (file.endsWith(".html")) {
    for (const match of content.matchAll(/(?:href|src)="([^"]+)"/g)) {
      assert.ok(match[1].startsWith("#") ? content.includes(`id="${match[1].slice(1)}"`) : manifest.files.includes(match[1]), `Nonlocal or missing asset/link: ${match[1]}`);
    }
    assert.ok(!content.includes("<script>window.INJECTED"));
  }
}
const fixture = manifest.findings.find((finding) => finding.findingId === "title.missing");
assert.equal(fixture ? occurrences(fixture) : undefined, 1205, "Use the synthetic 1,205-row fixture.");
assert.equal(fixture.htmlPages.length, 2);

const profile = await mkdtemp(join(tmpdir(), "ff-offline-report-chrome-"));
let browser;
let socket;
try {
  browser = spawn(process.env.CHROME_BIN ?? "google-chrome", ["--headless", "--no-sandbox", "--disable-gpu", "--no-first-run", "--disable-background-networking", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let log = "";
  let launchError;
  browser.stderr.on("data", (chunk) => { log = (log + chunk).slice(-8192); });
  browser.on("error", (error) => { launchError = error; });
  let target;
  const deadline = Date.now() + 30_000;
  while (!target && Date.now() < deadline) {
    if (launchError || browser.exitCode !== null) throw new Error(`Chrome failed: ${launchError ?? log}`);
    try {
      const port = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0];
      assert.match(port, /^\d+$/);
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1000) });
      target = (await response.json()).find((candidate) => candidate.type === "page" && candidate.webSocketDebuggerUrl);
    } catch { await delay(100); }
  }
  assert.ok(target, `Chrome did not become ready. ${log}`);
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let nextId = 0;
  const pending = new Map();
  const errors = [];
  const externalRequests = [];
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.method === "Runtime.exceptionThrown") errors.push(message.params.exceptionDetails.text);
    if (message.method === "Network.requestWillBeSent" && /^https?:/.test(message.params.request.url)) externalRequests.push(message.params.request.url);
    const request = pending.get(message.id);
    if (request) {
      pending.delete(message.id);
      clearTimeout(request.timer);
      message.error ? request.reject(new Error(message.error.message)) : request.resolve(message.result);
    }
  };
  const cdp = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Timed out: ${method}`)); }, 10_000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, returnByValue: true });
    assert.ok(!result.exceptionDetails, result.exceptionDetails?.text);
    return result.result.value;
  };
  const until = async (expression) => {
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      if (await evaluate(`Boolean(${expression})`)) return;
      await delay(50);
    }
    assert.fail(`Timed out waiting for ${expression}`);
  };
  const navigate = async (file) => {
    await cdp("Page.navigate", { url: pathToFileURL(join(directory, file)).href });
    await until(`document.readyState === "complete" && location.pathname.endsWith(${JSON.stringify(file)})`);
  };
  await cdp("Runtime.enable");
  await cdp("Network.enable");
  await cdp("Network.emulateNetworkConditions", { offline: true, latency: 0, downloadThroughput: 0, uploadThroughput: 0 });
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  await navigate("index.html");
  assert.equal(await evaluate('document.querySelectorAll(".finding-card").length'), manifest.findingCount);
  assert.equal(await evaluate('typeof window.INJECTED'), "undefined");
  assert.equal(await evaluate('getComputedStyle(document.querySelector(".metrics")).display'), "grid");
  if (!comparison) {
  await evaluate('document.getElementById("severity-filter").value="error";document.getElementById("severity-filter").dispatchEvent(new Event("change"))');
  assert.equal(await evaluate('document.querySelectorAll("[data-finding]:not([hidden])").length'), 1);
  await evaluate('document.getElementById("severity-filter").value="";document.getElementById("severity-filter").dispatchEvent(new Event("change"))');
  }
  const screenshot = await cdp("Page.captureScreenshot", { format: "png" });
  const screenshotPath = join(tmpdir(), comparison ? "ff-audit-comparison-index.png" : "ff-audit-report-index.png");
  await writeFile(screenshotPath, Buffer.from(screenshot.data, "base64"));
  await evaluate(`document.querySelector('a[href="${fixture.htmlPages[0]}"]').click()`);
  await until('document.querySelectorAll(".evidence-row").length === 1000');
  assert.match(await evaluate('document.querySelector(".evidence-heading .lead").textContent'), comparison ? /1–1000 \/ 1205/ : /1–1000 of 1205/);
  await evaluate('document.querySelector("a[rel=next]").click()');
  await until('document.querySelectorAll(".evidence-row").length === 205');
  assert.match(await evaluate('document.querySelector(".evidence-heading .lead").textContent'), comparison ? /1001–1205 \/ 1205/ : /1001–1205 of 1205/);
  assert.equal(await evaluate('document.querySelector(".evidence-row:last-child .url").textContent'), "https://example.test/1204");
  await evaluate('document.querySelector(".evidence-row:last-child details").open=true');
  assert.match(await evaluate('document.querySelector(".evidence-row:last-child details pre").textContent'), comparison ? /Fixed title/ : /İstanbul 🐸/);
  assert.equal(await evaluate('typeof window.INJECTED'), "undefined");
  if (!comparison) {
  await evaluate('document.getElementById("page-search").value="/1204";document.getElementById("page-search").dispatchEvent(new Event("input"))');
  assert.equal(await evaluate('document.querySelectorAll(".evidence-row:not([hidden])").length'), 1);
  }
  await cdp("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
  await navigate("index.html");
  assert.equal(await evaluate('document.documentElement.scrollWidth <= innerWidth'), true);
  await cdp("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  assert.equal(await evaluate('getComputedStyle(document.body).backgroundColor'), "rgb(19, 37, 31)");
  await cdp("Emulation.setEmulatedMedia", { media: "print" });
  if (!comparison) assert.equal(await evaluate('getComputedStyle(document.querySelector(".filters")).display'), "none");
  assert.deepEqual(errors, []);
  assert.deepEqual(externalRequests, []);
  console.log(`Offline ${comparison ? "comparison" : "report"} passed: all 1,205 rows reachable, local navigation${comparison ? "" : "/search and filters"}, escaped content, narrow/dark/print views. Screenshot: ${screenshotPath}`);
} finally {
  socket?.close();
  if (browser && browser.exitCode === null) {
    const stopped = new Promise((resolve) => browser.once("exit", resolve));
    browser.kill("SIGTERM");
    const timeout = setTimeout(() => browser.kill("SIGKILL"), 5000);
    await stopped;
    clearTimeout(timeout);
  }
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
