import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const { version } = readJson("package.json");
const lock = readJson("package-lock.json");
const cargo = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--locked", "--format-version", "1"], { encoding: "utf8" }));
const versions = {
  "package-lock.json": lock.version,
  "package-lock.json root package": lock.packages[""].version,
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "release manifest": readJson(".release-please-manifest.json")["."],
  ...Object.fromEntries(cargo.packages.map((pkg) => [pkg.name, pkg.version])),
};

for (const [source, actual] of Object.entries(versions)) {
  assert.equal(actual, version, `${source} must match package.json (${version})`);
}
if (process.env.RELEASE_TAG) {
  assert.equal(process.env.RELEASE_TAG, `v${version}`, "Release tag must match the packaged version");
}
console.log(`Version checks passed: ${version}`);
