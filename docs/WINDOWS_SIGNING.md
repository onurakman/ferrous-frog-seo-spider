# Windows signing onboarding

Status on 2026-09-19: Windows releases are unsigned. The maintainer has no code-signing certificate or signing-service account yet; the repository has no Actions secrets, variables or environments for signing. No signing integration or trusted signed installer has been verified. This is an external prerequisite, not a completed security-warning fix.

## Prepared SignPath application details

[SignPath Foundation accepts applications for free signing of qualifying open-source projects](https://signpath.org/apply.html). Approval is discretionary; review its [current conditions](https://signpath.org/terms.html) before applying. The following project information is ready to use:

| Field | Value |
| --- | --- |
| Project | Ferrous Frog SEO Spider |
| Repository and homepage | https://github.com/onurakman/ferrous-frog-seo-spider |
| Repository owner | `onurakman` |
| Description | Cross-platform desktop website crawler and technical SEO auditor built with Rust, Tauri and React. It crawls user-selected websites, inspects technical and on-page SEO signals, and exports audit results. |
| Declared workspace license | `MIT OR Apache-2.0` in `Cargo.toml`; the repository does not yet contain the corresponding license text files. Resolve this before claiming the application meets the licensing requirements. |
| Build system | GitHub-hosted Actions runners; `.github/workflows/release-please.yml` |
| Windows targets | x64 and ARM64; NSIS setup executables |
| Released examples | [v0.5.0](https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/v0.5.0), including `FerrousFrog_0.5.0_windows_x64-setup.exe` and `FerrousFrog_0.5.0_windows_arm64-setup.exe` |

The maintainer must provide their own contact/identity information, accept the provider's terms, and authorize the GitHub integration. Do not send account passwords, private keys or API tokens in chat or commit them. No application has been submitted and no paid service has been provisioned.

Network behavior for the application review: crawls request user-selected websites; optional integrations and AI features send requests to their configured providers. The desktop also automatically requests public release metadata from GitHub for update notifications, without crawl data or a GitHub token. Do not claim that every network request requires an explicit user action.

## Integration after approval

Use the approved provider's signing policy and supported tooling to sign the application executable, NSIS uninstaller and final installer. Preserve both Windows architectures and the existing draft-only upload/all-platform publication gate. Check trusted Authenticode signatures and timestamps before publication; failed signing must never silently produce an unsigned release.

SignPath's [documented composite formats](https://docs.signpath.io/artifact-configuration/reference) do not include NSIS. Signing only the outer setup executable does not sign its embedded application or uninstaller. Confirm the supported Tauri/NSIS signing sequence with the provider before wiring the workflow; do not replace it with an unverified generic artifact-signing step. Tauri exposes a [custom signing command](https://v2.tauri.app/distribute/sign/windows/#custom-sign-command) for signing during bundling.

Publish a new release after successful signing and Windows installation verification. Preserve the existing published installers. Record verified publisher identity and signature results for both architectures before marking the roadmap item complete.

## What the signature fixes

A trusted signature establishes publisher identity and file integrity. A self-signed certificate does not establish public Windows trust. New correctly signed downloads can still display SmartScreen reputation warnings; signing is not a guarantee that every first-install warning disappears. See [Microsoft's SmartScreen guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).

Microsoft documents [Store-distributed MSIX packages](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) as a separate route with Microsoft-managed signing. That requires a developer account and Store submission; the current NSIS downloads do not acquire that trust automatically.
