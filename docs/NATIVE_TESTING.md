# Linux native desktop smoke

`make test-native` exercises the built desktop application through WebKitGTK and Tauri's external WebDriver. It uses real UI controls, IPC, HTTP requests and SQLite storage. It checks the library launch, a local crawl with robots disallow and crawl delay, a planted 404, saved results after restarting the application, and cancel/confirm quit with native process exit.

Use Node 24 from `.node-version`, Linux GTK/WebKitGTK runtime dependencies, `dbus-run-session`, `Xvfb`, `WebKitWebDriver`, and `tauri-driver`. The harness creates its own virtual display, D-Bus session and temporary XDG data/config/cache/runtime directories. It preserves `HOME` and existing crawl files.

Validation status: the isolated Xvfb/D-Bus/Tauri/WebKit driver preflight and embedded-assets desktop build passed locally. The first native attempt opened the library but failed while clearing the crawl URL input, before crawling. Its unscoped input selector requires investigation; the full native workflow remains unverified.

## Prepare tools without a system installation

The harness checks `PATH`, explicit binary overrides and the following temporary tool layout. On Debian/Ubuntu, inspect package candidates and dependencies first:

```sh
apt-cache policy xvfb webkit2gtk-driver
apt-cache depends xvfb webkit2gtk-driver
mkdir -p /tmp/ferrous-native-e2e-tools/packages /tmp/ferrous-native-e2e-tools/extracted
cd /tmp/ferrous-native-e2e-tools/packages
apt download xvfb webkit2gtk-driver
```

On newer Ubuntu versions, `webkit2gtk-driver` is a transitional package whose dependency contains the executable. If the dependency output names `webkitgtk-webdriver`, download it too:

```sh
apt download webkitgtk-webdriver
```

Extract packages locally and check their runtime dependencies. Download and extract any missing library package into the same directory; use an `LD_LIBRARY_PATH` pointing at its extracted library directory only when needed.

```sh
for package in *.deb; do
  dpkg-deb -x "$package" /tmp/ferrous-native-e2e-tools/extracted
done
ldd /tmp/ferrous-native-e2e-tools/extracted/usr/bin/Xvfb
ldd /tmp/ferrous-native-e2e-tools/extracted/usr/bin/WebKitWebDriver
cargo install tauri-driver --version 2.0.6 --locked \
  --root /tmp/ferrous-native-e2e-tools/tauri \
  --target-dir /tmp/ferrous-native-e2e-tools/tauri-target
```

The driver version above was verified for this harness. Tauri's [manual WebDriver setup](https://v2.tauri.app/develop/tests/webdriver/manual-setup/) explains the native driver requirements. Match the WebKit driver package to the installed WebKitGTK runtime.

## Run

From the repository root:

```sh
node scripts/smoke-native.mjs --check-tools
make build
make test-native
```

For a faster development build with embedded frontend assets:

```sh
npm run tauri:build -- --debug --no-bundle -- --locked
make test-native NATIVE_APP=target/debug/ferrous-frog
```

Use the Tauri build command so the executable includes the frontend. A plain Cargo development build may expect the Vite development server instead.

Overrides:

- `FF_NATIVE_TOOLS`: alternate temporary tools directory.
- `XVFB_BIN`, `WEBKIT_WEBDRIVER`, `TAURI_DRIVER`: explicit executable paths.
- `FF_NATIVE_APP` or `--app`: application binary when invoking the script directly.
- `FF_NATIVE_KEEP_ARTIFACTS=1`: preserve successful-run logs, SQLite fixtures and the JSON report.

Failed runs retain their temporary directory, driver/display logs, and a screenshot and HTML document when the webview is reachable. The harness prints the directory path. Successful runs remove their temporary crawl data by default.

The test sets `TAURI_AUTOMATION` and `TAURI_WEBVIEW_AUTOMATION` for the driver process. It remains a separate Linux check: Windows, macOS, installer signing and release publication require their own platform validation.
