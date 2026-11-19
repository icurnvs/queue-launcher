# Queue Launcher

Queue Launcher uses Rust and Tauri for Windows and macOS. This is the primary
implementation on `main`; future features, fixes, and releases target it.
The application lives in [Tauri](Tauri).
The Swift, WinForms, Electron, and Avalonia versions are
[archived for reference](archive/README.md).

## Download for Windows

Download the portable Windows x64 ZIP from
[GitHub Releases](https://github.com/icurnvs/queue-launcher/releases/latest)
and extract the entire ZIP to a local folder.

- Windows 11: run **QueueLauncher.exe**.
- Windows 10: run **Open on Windows 10.cmd** once to grant the browser sandbox
  read access to the bundled runtime and launch the app.

The ZIP includes Microsoft's Fixed Version WebView2 runtime. Application data
is stored in `%LOCALAPPDATA%\local.queue-launcher.tauri`.

## Build and run

Install Rust. On Windows, also install the MSVC toolchain, Visual Studio C++
build tools, and the WebView2 Evergreen Runtime. On macOS 14 or newer, install
Xcode or its command-line tools. From the repository root:

```sh
cargo run --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

For an optimized release executable:

```sh
cargo build --release --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

If the checkout is in a synced folder that rejects Cargo's build files, pass
`--target-dir` with a writable local directory to the Cargo command.

## Windows portable packaging

Download Microsoft's signed WebView2 Fixed Version CAB matching the target
architecture, then run from PowerShell:

```powershell
.\Scripts\package-tauri-windows.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab'
```

For ARM64, install the Rust ARM64 MSVC target and pass `-Architecture arm64`
with the matching CAB. Use `-OutputDirectory` to choose a fresh output folder.
Repackage with a newer CAB to update the bundled Chromium engine.

## macOS packaging

With Node/npm and the native Rust and Xcode toolchains installed:

```sh
cd Tauri
CARGO_TARGET_DIR="$PWD/../build/tauri" \
    npx --yes --package @tauri-apps/cli@2.12.4 tauri build --bundles app --ci -- --locked
```

The locally signed app is written to
`build/tauri/release/bundle/macos/Queue Launcher.app` from the repository root.
See the [macOS verification notes](docs/tauri-macos-handoff.md) for native
acceptance results, signing details, and remaining distribution work.

## Usage

Start with **Demo mode** for 12 offline sessions. Demo and live batches keep
separate browser profiles, link history, drop logs, and preferences.

In a live batch, paste an HTTP, HTTPS, or local fixture URL and open 1 to 50
sessions. Each session has separate browser data and stays loaded as you
switch between sessions. Complete the site's CAPTCHA, then click **Mark ready**.
Use **Add session** to open one more session from the original URL.

The app restores the last selected session, manual ready state, and saved
browser profiles on relaunch. **Copy** copies a session's shareable queue link;
**Copy and close** also closes that session. **Link history** retains distinct
shareable links after sessions close. Queue links can expire according to the
site's rules. **End drop** closes the batch and clears its browser data after
confirmation.

**Drop logs** opens the log folder. Timestamped JSON Lines logs contain event
names and numeric details; URLs, queue IDs, cookies, CAPTCHA answers, and tokens
stay out of the logs. Logs and saved link history survive ending a drop.

Set `QUEUE_LAUNCHER_DATA` to use a separate data directory during development.
The Rust app keeps its data separate from the archived Swift and WinForms apps.

## Verification

Run the parser, link, and persistence tests:

```sh
cargo test --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher
```

Run the Windows offline application check with a fresh scratch directory:

```powershell
$env:QUEUE_LAUNCHER_DATA = Join-Path $env:TEMP ('queue-launcher-test-' + [guid]::NewGuid())
cargo run --locked --manifest-path Tauri/Cargo.toml --bin queue-launcher -- --verify-offline
Remove-Item Env:QUEUE_LAUNCHER_DATA
```

On a logged-in macOS desktop:

```sh
sh Tauri/verify-macos-app.sh
```

For a manual offline check, use the file URL for
[Scripts/local-waiting-room.html](Scripts/local-waiting-room.html). Each session
should have a different queue ID, retain it after reload, and retain its page
state while switching sessions. The earlier browser isolation and scale
harness remains documented in [Tauri/README.md](Tauri/README.md).
