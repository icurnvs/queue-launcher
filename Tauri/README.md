# Queue Launcher development

Queue Launcher uses a shared Rust backend and HTML/CSS/JavaScript interface
with native browser adapters for Windows and macOS. For downloads and usage,
see the [repository README](../README.md).

Run the commands below from the repository root unless stated otherwise.

## Source layout

| Path | Responsibility |
| --- | --- |
| `src/app/main.rs` | Application startup and launcher window |
| `src/app/launcher.rs` | Session actions, snapshots, and browser lifecycle |
| `src/app/store.rs` | Persistent batches, link history, and cleanup journal |
| `src/app/browser.rs` | Browser adapter and Windows WebView2 integration |
| `src/app/browser/mac.rs` | Native macOS WebKit integration |
| `src/queue_status.rs`, `src/read_queue_page.js` | Waiting-room observation and link parsing |
| `ui/app.*` | Application interface |
| `capabilities/app.json` | Launcher permissions |
| `src/app/launcher/smoke.rs`, `src/app/launcher/mac_smoke.rs` | Platform offline application checks |

Paths in this table are relative to `Tauri`. Session pages have no launcher
command permissions. Windows sessions use named WebView2 profiles; macOS
sessions use UUID-backed WebKit website data stores. The child-webview API
requires Tauri's `unstable` feature.

## Prerequisites

- Rust and Cargo.
- **Windows:** the Rust MSVC toolchain, Visual Studio C++ build tools, and the
  WebView2 Evergreen Runtime for development.
- **macOS:** macOS 14 or later and Xcode or its command-line tools. Packaging
  also uses Node.js and npm.

## Build and run

```sh
cargo run --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

Build an optimized executable:

```sh
cargo build --release --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

Set `QUEUE_LAUNCHER_DATA` to use a separate application data directory during
development. Use a new directory for destructive cleanup checks. Demo mode
uses loopback port **43193**; leave it available.

If a synced folder rejects Cargo's build files, pass `--target-dir` with a
writable local directory to the Cargo command.

## Package for Windows

### Evergreen installer

From `Tauri`, build a per-user NSIS setup executable:

```powershell
npx.cmd --yes --package @tauri-apps/cli@2.12.4 tauri build --bundles nsis --ci -- --locked
```

The installer is written to `Tauri/target/release/bundle/nsis/`. It uses the
installed Evergreen WebView2 Runtime and downloads Microsoft's bootstrapper
if the runtime is missing. That fallback requires internet access. The
Windows-specific Tauri configuration leaves macOS packaging unchanged.

### Small portable ZIP

From the repository root:

```powershell
.\Scripts\package-tauri-windows.ps1
```

This creates `build/tauri-windows-x64-portable-evergreen.zip`. Extract it and
run `QueueLauncher.exe`; there is no app installer. Evergreen WebView2 must
already be installed, or the user must install it using the link in the
included README. This ZIP does not automatically install the runtime.
Application data still lives in `%LOCALAPPDATA%\local.queue-launcher.tauri`,
shared with installed copies, rather than beside the executable.

### Self-contained portable ZIP

Download Microsoft's signed
[WebView2 Fixed Version CAB](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
matching the target architecture. From PowerShell:

```powershell
.\Scripts\package-tauri-windows.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab'
```

The script builds the release executable and creates
`build/tauri-windows-x64-portable.zip` with the browser runtime and Windows 10
launcher. For ARM64, install the `aarch64-pc-windows-msvc` Rust target and pass
`-Architecture arm64` with the matching CAB. Use `-OutputDirectory` to choose
a fresh output folder. Repackage with a newer CAB to update the bundled engine.

### Automated Windows releases

The `Windows release` GitHub Actions workflow uses this PC's self-hosted
runner to build the x64 installer, Evergreen portable ZIP, and self-contained
portable ZIP. Each package has a SHA-256 checksum file. The self-hosted job
has read-only repository access; a separate GitHub-hosted job uploads the
packages to Releases. No personal access token is stored in the workflow.

To publish a new version, update `Tauri/Cargo.toml` (including `Cargo.lock`)
and `Tauri/tauri.conf.json`, commit and push, then push the matching `v` tag,
for example `v0.1.2`. The workflow builds that exact tagged commit and creates
or updates its GitHub Release. Publishing a release through GitHub also
triggers the workflow. Only matching version tags are accepted for `v` tags.
macOS releases continue to be built separately and can be attached to the
same release.

For a test, select **Actions > Windows release > Run workflow**, leave `tag`
empty, and leave `publish` unchecked. Packages are available as a workflow
artifact for seven days. To rebuild a release, enter its existing tag and
check `publish`. Uploads replace the matching Windows assets and preserve
other release assets, including macOS. An existing draft stays a draft.

The runner lives at `%LOCALAPPDATA%\QueueLauncherRunner`, outside Nextcloud.
It starts in the background when this Windows user signs in, using a shortcut
in the user's Startup folder. Keep the PC awake, online, and signed in; locking
the screen is fine. This setup is not a Windows service and does not run
before sign-in. Diagnostics are in the runner's `_diag` directory and
`runner-console.log`. To start it manually, run its `start-runner.ps1` script.

To provision the same setup again, authenticate `gh` with repository admin
access, install Rust/MSVC, Git, and Node.js, and run:

```powershell
.\Scripts\setup-windows-runner.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.157.0.4341.60.x64.cab'
```

The setup verifies the runner download's published SHA-256 checksum and the
CAB's Microsoft signature and pinned checksum. The CAB is cached under the
runner's `_work/_tool/WebView2`. To upgrade the fixed runtime, update
`.github/windows-runtime.json` with the new CAB filename and checksum, then
rerun setup with that signed CAB. The Evergreen packages do not bundle it.
This public repository's runner workflow intentionally has no pull-request
trigger. Keep untrusted PR code off this PC's runner.

## Package for macOS

```sh
cd Tauri
CARGO_TARGET_DIR="$PWD/../build/tauri" \
    npx --yes --package @tauri-apps/cli@2.12.4 tauri build --bundles app --ci -- --locked
```

The app is written to
`build/tauri/release/bundle/macos/Queue Launcher.app` from the repository root.
The default macOS configuration uses ad-hoc signing and builds for the host
architecture. Developer ID signing and notarization require separate Apple
credentials; the default command does not produce a notarized app.

## Verify the application

Run the Rust unit tests:

```sh
cargo test --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

On Windows, run the offline application check from PowerShell:

```powershell
$env:QUEUE_LAUNCHER_DATA = Join-Path $env:TEMP ('queue-launcher-test-' + [guid]::NewGuid())
try {
    cargo run --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher -- --verify-offline
} finally {
    Remove-Item Env:QUEUE_LAUNCHER_DATA
}
```

On a logged-in macOS desktop:

```sh
sh Tauri/verify-macos-app.sh
```

The application checks run in debug builds with scratch data. The macOS script
checks navigation, storage isolation, relaunch, cleanup recovery, and 12- and
50-session batches. Its fixture uses port **43194**, and Demo uses **43193**.
Check packaged release apps separately through their normal UI.

For a manual offline check, open the file URL for
[Scripts/local-waiting-room.html](../Scripts/local-waiting-room.html) in a batch.
Each session should have a different queue ID, retain it after reload, and
retain its page state while switching sessions.

## Browser isolation harness

The separate `queue-launcher-tauri-prototype` binary exercises browser stores
and view switching with a local fixture. It uses port **43192** and separate
data under `QUEUE_TAURI_PROTOTYPE_DATA`. Run one harness process at a time.

From `Tauri`, run:

```sh
cargo run --locked --features browser-harness --bin queue-launcher-tauri-prototype
```

For scripted checks, use `sh Tauri/verify-macos.sh` on macOS or
`powershell -NoProfile -ExecutionPolicy Bypass -File .\Tauri\verify-windows.ps1 -NamedProfiles`
on Windows, from the repository root. The Windows script also accepts
`-FixedRuntimePath` to check an extracted WebView2 runtime.

These scripts exercise isolation, switching, relaunch, cleanup, and 12- and
50-view batches. Use the application checks above for production workflows.
