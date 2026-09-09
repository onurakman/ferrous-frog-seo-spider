# Building and releasing Ferrous Frog

## Local builds

Use the Rust version in `rust-toolchain.toml`, Node.js from `.node-version`, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.

```bash
npm ci
make ci
make build
make release
```

`make build` compiles the frontend and an optimized desktop executable without installers. `make release` also produces installers. The equivalent packaging command, including on Windows, is:

```bash
npm run tauri:build -- -- --locked
```

To select formats, use `--bundles deb,rpm,appimage` on Linux, `--bundles app,dmg` on macOS, or `--bundles nsis` on Windows before the final `-- --locked`. Linux AppImage packaging needs FUSE 2 (`libfuse2` on Ubuntu 22.04, `libfuse2t64` on Ubuntu 24.04), or `APPIMAGE_EXTRACT_AND_RUN=1` in environments without FUSE. Outputs are under the workspace root's `target/release/bundle/`, or `target/<target>/release/bundle/` when a Rust target is specified.

The standard release excludes the optional Chrome CDP backend. Developers can add `--features js-rendering` to build it and must provide Chrome/Chromium at runtime. CI compiles this path separately and runs real-browser tests for crawling, subresource politeness, pause, resume and stop.

## GitHub setup

1. Push the repository, including both lockfiles, to GitHub with `master` or `main` as the default branch.
2. Under **Settings → Actions → General → Workflow permissions**, enable **Allow GitHub Actions to create and approve pull requests**. The workflows declare their own token permissions; CI only needs read access.
3. Run **CI** once and inspect the result. CI installs a pinned Chrome for Testing build and passes its executable through `CHROME_BIN`. Local UI checks also accept `CHROME_BIN`, defaulting to `google-chrome`. The smoke test waits up to 30 seconds for a working debugging endpoint and page, and reports browser exit errors and stderr when startup fails.
4. Use Conventional Commits for releasable changes. The default branch's **Release Please** workflow maintains a version/changelog PR. Merge it when ready to release.

No custom secret is required for the default unsigned/ad-hoc-signed pipeline. With `GITHUB_TOKEN`, bot-created release PRs do not automatically trigger another workflow. Run **CI → Run workflow** with the release PR branch in `ref` before merging; the release pipeline always repeats CI on the exact release commit before packaging. To trigger checks automatically on bot PRs, optionally supply `RELEASE_PLEASE_TOKEN` with repository contents, pull requests and issues write permissions, using a fine-grained token or a GitHub App token. See [Release Please's token documentation](https://github.com/googleapis/release-please-action#other-actions-on-release-please-prs).

## Versioning

Release Please uses the Node strategy to update `package.json` and both root version entries in `package-lock.json`. Targeted extra-file updaters change `workspace.package.version` in `Cargo.toml`, the local packages in `Cargo.lock`, and `src-tauri/tauri.conf.json`. All eight crates inherit the workspace version. The Rust strategy is unsuitable here because its updater expects a root `[package]` and concrete member versions.

`make check-versions` verifies every workspace member, npm lock entry, Tauri version and release manifest. Packaging additionally checks that the tag equals `v<version>`. The Cargo lockfile selector updates packages without a registry/git `source`; in this workspace those are exactly the eight local crates. Keep new local packages on the shared version.

| Commit | Release effect |
| --- | --- |
| `fix: persist crawl settings` | Patch |
| `feat: add an export format` | Minor |
| `feat!: change the archive format` | Minor before 1.0; major after 1.0 |
| `docs: update setup`, `ci: update Actions` | No release on their own |

The existing version is `0.1.0`. If the initial imported history has no `feat:` or `fix:` commit, Release Please has nothing releasable to collect; use a Conventional Commit for the first releasable change. Do not manually bump individual manifests. Review the generated version PR and `CHANGELOG.md` together.

## Installer matrix

| Platform | Architecture | Runner | Downloads |
| --- | --- | --- | --- |
| Linux | x64 | Ubuntu 22.04 | `.deb`, `.rpm`, `.AppImage` |
| Linux | ARM64 | Ubuntu 22.04 ARM | `.deb`, `.rpm`, `.AppImage` |
| macOS | Intel x64 | macOS 15 Intel | `.dmg`, `.app.tar.gz` |
| macOS | Apple Silicon | macOS 15 ARM | `.dmg`, `.app.tar.gz` |
| Windows | x64 | Windows 2022 | NSIS `-setup.exe` |
| Windows | ARM64 | Windows 2022, cross-compiled | NSIS `-setup.exe` |

Linux uses native GNU/WebKitGTK builds. Its installers are not static musl executables; they require compatible desktop libraries. Ubuntu 22.04 is the build baseline. An AppImage still depends on the host's graphics and system libraries. Windows uses NSIS for both architectures. The Windows release executable does not open a console window.

Linux CI and packaging install dependencies from the Ubuntu 22.04 runner's `/etc/apt/sources.list`, using APT's `Dir::Etc::sourceparts=-` option for both update and install. This prevents unrelated preinstalled repositories, such as Chrome's APT repository, from blocking builds with package-index errors. Package verification remains enabled; test Chrome is installed separately at its pinned version. Revisit this source path when upgrading runners: [the runner image uses `ubuntu.sources` on newer Ubuntu versions](https://github.com/actions/runner-images/blob/main/images/ubuntu/scripts/build/configure-apt.sh).

## Release lifecycle and retries

1. Merging the version PR creates a tag and a draft release with changelog notes.
2. CI checks the release commit, including UI smoke tests and optional rendering compilation.
3. Six independent jobs check out that same SHA, use `npm ci` and locked Cargo dependencies, then upload installers to the draft. A failed job does not cancel the other platforms.
4. Only after every build succeeds, the final job verifies that the tag still resolves to the original draft release ID, downloads the installers, computes `SHA256SUMS`, uploads it and publishes the release.

The release workflow calls CI directly, so it does not depend on bot-created tags triggering another workflow. Concurrent release runs are serialized. Updates are downloaded through the browser; this workflow does not generate a signed feed for in-app installation.

For a failed run, use **Re-run failed jobs**. To rebuild an existing draft later, select **Release Please → Run workflow**, leave the default branch selected, and enter its existing tag, such as `v0.1.0`. Manual retries reject published releases; create a new patch release instead of replacing downloads that users already installed. If only checksum/publication failed, rerun that failed job.

After fixing a workflow, use **Run workflow** from the updated default branch with the existing draft tag. **Re-run failed jobs** retains the old workflow definition. The manual run uses the updated workflow while still checking out and packaging the original tagged release commit.

Do not create another release for the tag or publish it manually while the workflow is running. GitHub can hold a draft and a published release with the same tag; tag-based CLI commands then select the published entry, even when all installers belong to the draft. The publish job reports both release IDs if they differ. Inspect the entries with `gh api repos/OWNER/REPO/releases --jq '.[] | {id, tag_name, draft, assets: (.assets | length)}'`. If an accidentally published duplicate has no assets, remove only that duplicate release by ID after confirming it is the unwanted entry, preserve the tag and the draft containing installers, then rerun only the failed publication job. Rebuilding the six platforms is unnecessary in that case. Do not delete a published release containing installers; resolve that case with a new patch release.

`make test-release` exercises the publication step with local GitHub CLI responses, including duplicate IDs, already-published releases, API failures and successful checksum generation. It does not contact or modify GitHub.

On Linux, verify downloaded assets with `sha256sum -c SHA256SUMS`; on macOS use `shasum -a 256 -c SHA256SUMS`. On Windows use `Get-FileHash .\FerrousFrog_*.exe -Algorithm SHA256` and compare with the manifest. Download every listed asset for a complete `-c` check, or check the line for your selected installer.

## Update notifications

After the workspace is ready, the desktop app checks this repository's public [latest stable release](https://docs.github.com/en/rest/releases/releases#get-the-latest-release). It compares the tag with the compiled application version using semantic version precedence, ignoring drafts, prereleases and build-metadata-only changes. Requests have a 10-second timeout, use no GitHub token, and do not include crawl data. A repository without a published release produces no notification.

**More > Check for updates** runs the same check on demand and displays failures with a retry action. Automatic failures remain quiet. **Remind me later**, Escape and closing an available-update notice postpone automatic checks for 24 hours, including after a restart; manual checks bypass that reminder. **Download update** opens the checked release's GitHub page in the default browser, where users choose the installer for their system. The current application and crawl keep running until the user closes them.

Published releases become visible to the app only after the all-platform build gate finishes. Signed, in-app download/install support is a separate capability that requires the [Tauri updater signing setup](https://v2.tauri.app/plugin/updater/#signing-updates).

## Signing and validation limits

Windows installers are unsigned. macOS bundles use Tauri's ad-hoc signing identity (`-`) so Apple Silicon code has a signature, but they are not Developer ID signed or notarized. Operating-system trust prompts are expected. Before distributing trusted signed installers, configure [macOS signing and notarization](https://v2.tauri.app/distribute/sign/macos/) or [Windows signing](https://v2.tauri.app/distribute/sign/windows/) with your own certificates; replace the ad-hoc macOS identity when doing so. Checksums detect changed downloads and do not establish publisher identity.

CI runs Rust tests and the real React screen with synthetic Tauri IPC on Linux. Installer builds prove compilation and bundling for each target, not native GUI operation. Test installation, the splash window, system appearance and native quit confirmation on each supported desktop before announcing the first release. The first hosted matrix run remains to be verified after this repository is pushed.

The workflow follows the [official Tauri GitHub pipeline](https://v2.tauri.app/distribute/pipelines/github/), with release-commit verification and publication deferred until all assets are ready.
