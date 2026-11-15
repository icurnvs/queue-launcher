# Queue Launcher Tauri macOS handoff

Finish the macOS side of the existing Rust and Tauri port on a Mac with a
logged-in desktop. The implementation baseline is `8d2ae76` on
`codex/rust-tauri-prototype`. Work from that branch's history so the shared
launcher and the Windows named WebView2 profile implementation carry forward.

## Get the Windows port onto the Mac

If the branch is already available in the Mac checkout, branch from it. If you
received `queue-launcher-tauri-macos.bundle`, fetch its branch from an existing
Queue Launcher checkout and create a macOS branch:

```sh
git fetch /path/to/queue-launcher-tauri-macos.bundle refs/heads/codex/rust-tauri-prototype
git switch -c codex/tauri-macos FETCH_HEAD
git merge-base --is-ancestor 8d2ae76 HEAD
```

The Windows branch was local when this handoff was written. A clone of
`origin/main` alone starts from the older Swift app. The bundle carries the
source, lockfile, fixtures, and this handoff.

## Build and establish the baseline

Use macOS 14 or newer, the native stable Rust toolchain, and Xcode or its
command-line tools. Start in the repository root:

```sh
sw_vers
uname -m
xcode-select -p
rustc --version
cargo test --locked --manifest-path Prototype/Tauri/Cargo.toml --bin queue-launcher
sh Prototype/Tauri/verify-macos.sh
```

`verify-macos.sh` drives the earlier browser harness. It checks two same-origin
stores, switching without reloading, relaunch, selective deletion, and 12
and 50 views. Use the results to isolate browser issues. The ported app has
its own entry point and also needs testing:

```sh
scratch="$(mktemp -d "${TMPDIR:-/tmp}/queue-launcher-mac.XXXXXXXX")"
QUEUE_LAUNCHER_DATA="$scratch"   cargo run --locked --manifest-path Prototype/Tauri/Cargo.toml --bin queue-launcher
```

Reuse that exact scratch path when relaunching. Demo uses loopback port 43193
and the harness uses 43192. Stop other test instances if either port is occupied.
The Windows-only `--verify-offline` flag exercises production workflows on
Windows. Use the native Mac UI and extend the offline verification where it
helps make a macOS regression reproducible.

## Finish the macOS adapter

- **Navigation and process lifecycle.** In [browser.rs](../Prototype/Tauri/src/app/browser.rs)
  the macOS code builds a child WKWebView with the saved UUID, uses Tauri page-load
  callbacks, and returns `false` from `can_go_back`. Wire up native WebKit
  back navigation and the enabled state. Exercise redirects, allowed popups in
  the same session, reload, provisional load errors, and retry. The current
  callbacks use a constant navigation ID and carry no load error. Check that
  overlapping navigations and stale page observations behave correctly.
- **Stores and cleanup.** In [launcher.rs](../Prototype/Tauri/src/app/launcher.rs)
  the shared state journals pending cleanup before closing a session. The Mac
  adapter closes the child, waits 250 ms, and dispatches Wry
  `remove_data_store` on the main thread. Verify that WebKit has released the
  view, that successful removal clears the journal, and that an interrupted
  close can be recovered at relaunch. Check live and Demo stores separately.
  The [existing Swift app](../Sources/QueueLauncher/QueueLauncher.swift) is
  the native reference for UUID-backed stores and detaching before removal.
- **Window and desktop integration.** Check the selected child view's placement
  and focus on resize, including Retina scaling. Open Link history and End
  batch confirmation and verify that the child hides and returns in the same
  page state. Check the clipboard and that Drop logs opens Finder. Preserve
  the launcher capability boundary in [app.json](../Prototype/Tauri/capabilities/app.json).
  Site views use the read-only page probe and have no host command permissions.
- **Packaging.** Tauri bundling is currently disabled in
  [tauri.conf.json](../Prototype/Tauri/tauri.conf.json). Produce a local macOS
  `.app` for the Tauri binary and validate launch from Finder, including a
  second launch against the same data. The existing
  [package-app.sh](../Scripts/package-app.sh) builds the Swift app and is a
  reference for local signing. Report which architecture and signing mode you
  tested. Distribution signing and notarization can be handled separately.

## Acceptance on the Mac

1. Run the native build and tests. Record macOS version, architecture, and
   the exact commands. Windows only reached an Apple Silicon Rust type check
   with `DOCS_RS=1` to skip the Objective-C helper.
2. Verify two same-origin sessions have different cookies and local-storage
   values. Switch between them and check an in-memory marker. Quit and relaunch
   with the same IDs and verify the corresponding stores reopen.
3. Close one session. Prove its UUID store is removed or that reopening it
   starts empty, and show the surviving session and saved sample link still
   work. Check End batch and a cleanup retry across relaunch.
4. Run the ported launcher with 12 and 50 offline sessions. Record time until
   all fixture pages are ready, switch responsiveness, and failures or limits.
   Also exercise mark ready, queue status and sorting, link history,
   copy-and-close, and switching between Demo and a live-mode fixture batch.
5. Launch the packaged `.app` from Finder. Report native results, any gaps, and
   reproduction instructions. Keep changes focused on the macOS adapter and
   shared fixes required by the Mac evidence.

Use local fixtures and synthetic links. Keep live queue URLs, IDs, cookies,
and tokens out of logs and test reports. Use fresh UUIDs and scratch data for
cleanup checks so existing WinForms, Swift, and Tauri profiles stay separate.

## Verified Windows reference

Nine Rust tests passed. The real Windows UI restored the selected session and
ready state, and the offline production check passed selective close, End
batch, separate Demo state, history, and log redaction. The 12- and 50-view
browser harness passed with named profiles. The extracted Windows x64 ZIP
loaded its adjacent Fixed Version WebView2 157.0.4341.60 and restored a
three-session offline batch. See the [port README](../README.md) and the
[browser harness results](../Prototype/Tauri/README.md) for reproduction.
