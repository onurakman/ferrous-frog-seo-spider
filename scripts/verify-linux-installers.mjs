// Verify Linux installer payloads without installing them on the host.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { constants } from "node:fs";
import { access, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: { deb: { type: "string" }, appimage: { type: "string" }, smoke: { type: "string" }, revision: { type: "string" }, keep: { type: "boolean", default: false } } });
assert.ok(values.deb && values.appimage, "Pass both --deb and --appimage paths.");
const deb = resolve(values.deb);
const appImage = resolve(values.appimage);
const smoke = resolve(values.smoke ?? "scripts/smoke-native.mjs");
const work = await mkdtemp(join(tmpdir(), "ferrous-linux-installers-"));
let success = false;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: "inherit", ...options });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(" ")} failed.`);
}

async function executable(path) {
  await access(path, constants.X_OK);
  return path;
}

try {
  await Promise.all([access(deb, constants.R_OK), access(appImage, constants.R_OK), access(smoke, constants.R_OK)]);
  const metadata = execFileSync("dpkg-deb", ["-f", deb, "Package", "Version", "Architecture"], { encoding: "utf8" }).trim().split("\n");
  assert.equal(metadata.length, 3, "The Debian package must expose package, version and architecture metadata.");
  console.log(`Debian package: ${metadata.join(" · ")}`);
  run("dpkg-deb", ["-c", deb]);
  const debRoot = join(work, "deb");
  run("dpkg-deb", ["-x", deb, debRoot]);
  const debApp = await executable(join(debRoot, "usr", "bin", "ferrous-frog"));

  const offset = execFileSync(appImage, ["--appimage-offset"], { encoding: "utf8" }).trim();
  assert.match(offset, /^\d+$/, "The AppImage must provide a numeric SquashFS offset.");
  const appRoot = join(work, "appimage");
  run("unsquashfs", ["-q", "-offset", offset, "-d", appRoot, appImage]);
  const appRun = await executable(join(appRoot, "AppRun"));

  await writeFile(join(work, "verification.json"), JSON.stringify({ revision: values.revision ?? null, deb, appImage, metadata, offset, debApp, appRun }, null, 2));
  console.log(`Private installer extraction: ${work}`);
  for (const app of [debApp, appRun]) run(process.execPath, [smoke, "--app", app]);
  success = true;
  console.log(`Linux installer verification passed${values.revision ? ` for ${values.revision}` : ""}: deb and AppImage payloads completed the native smoke.`);
} finally {
  if (success && !values.keep) await rm(work, { recursive: true, force: true });
  else console.log(`Linux installer verification artifacts: ${work}`);
}
