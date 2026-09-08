import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const workflow = readFileSync(new URL("../.github/workflows/release-please.yml", import.meta.url), "utf8");
const step = workflow.split("      - name: Publish the complete release\n")[1];
const script = step?.match(/        run: \|\n((?:          .*\n?)+)/)?.[1].replace(/^          /gm, "");
assert.ok(script, "The release publication step must be available for testing");

const root = mkdtempSync(join(tmpdir(), "ferrous-frog-release-test-"));
try {
  const bin = join(root, "bin");
  mkdirSync(bin);
  writeFileSync(join(bin, "gh"), `#!/usr/bin/env node
const { appendFileSync, mkdirSync, writeFileSync } = require("node:fs");
const args = process.argv.slice(2);
if (args[0] === "release" && args[1] === "view") {
  if (process.env.TEST_API_FAILURE) { console.error("GitHub API unavailable"); process.exit(1); }
  const info = { databaseId: Number(process.env.TEST_RELEASE_ID), isDraft: process.env.TEST_DRAFT === "true" };
  const query = args[args.indexOf("--jq") + 1];
  console.log(query.includes("@tsv") ? info.databaseId + "\\t" + info.isDraft : info[query.slice(1)]);
} else {
  appendFileSync(process.env.TEST_ACTIONS, JSON.stringify(args) + "\\n");
  if (args[1] === "download") {
    mkdirSync("release-assets");
    writeFileSync("release-assets/FerrousFrog_test.exe", "installer fixture");
  }
}
`, { mode: 0o755 });

  for (const [name, id, draft, message] of [
    ["published duplicate", "202", "false", /202.*101/],
    ["draft duplicate", "202", "true", /202.*101/],
    ["already published", "101", "false", /already published/i],
    ["API failure", "101", "true", /GitHub API unavailable/],
    ["original draft", "101", "true", null],
  ]) {
    const cwd = join(root, name);
    mkdirSync(cwd);
    const result = spawnSync("bash", ["-e", "-c", script], {
      cwd, encoding: "utf8", env: {
        ...process.env, PATH: `${bin}:${process.env.PATH}`,
        GH_REPO: "example/project", RELEASE_TAG: "v0.2.0", RELEASE_ID: "101",
        TEST_ACTIONS: join(cwd, "actions.jsonl"),
        TEST_RELEASE_ID: id, TEST_DRAFT: draft, TEST_API_FAILURE: name === "API failure" ? "1" : "",
      },
    });
    if (message) {
      assert.notEqual(result.status, 0, `${name} must prevent publication`);
      assert.match(result.stderr + result.stdout, message, `${name} must explain why publication stopped`);
      assert.throws(() => readFileSync(join(cwd, "actions.jsonl")), { code: "ENOENT" }, `${name} must not download, upload or edit releases`);
    } else {
      assert.equal(result.status, 0, result.stderr);
      const actions = readFileSync(join(cwd, "actions.jsonl"), "utf8").trim().split("\n").map(JSON.parse);
      assert.deepEqual(actions.map((args) => args.slice(0, 3)), [
        ["release", "download", "v0.2.0"], ["release", "upload", "v0.2.0"], ["release", "edit", "v0.2.0"],
      ]);
      const digest = createHash("sha256").update("installer fixture").digest("hex");
      assert.equal(readFileSync(join(cwd, "release-assets/SHA256SUMS"), "utf8"), `${digest}  FerrousFrog_test.exe\n`);
    }
  }
  console.log("Release workflow checks passed: draft identity, duplicate releases, published releases, API failures and checksums.");
} finally {
  rmSync(root, { recursive: true, force: true });
}
