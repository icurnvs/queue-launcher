# Queue Launcher Tauri prototype

This offline feasibility prototype exercises persistent browser sessions in one
native window. It uses a small HTML and JavaScript launcher with Tauri 2 and
Rust. It was branched from Queue Launcher commit `dbec19d`.

## Run

Enter this directory and run `cargo run --locked`. On Windows, use an MSVC
Rust toolchain, the Visual Studio C++ build tools and the installed Evergreen
WebView2 runtime. On macOS 14 or newer, use Rust and the Xcode command-line
tools. The bundled fixture listens only on `127.0.0.1:43192`. Run one copy at a
time and leave that port available so the fixture origin is stable across
launches.

1. Add two sessions. Each shows its own queue ID and cookie marker.
2. Increment the counter in session 1, switch to session 2, and switch back.
   The counter and page load marker stay the same.
3. Save the sample link and quit with the window close button. Run again. The
   IDs, cookies, and saved link return. The in-memory counter starts over.
4. Close session 1. Session 2 remains open and the saved link remains visible.

Normal runs keep data under the prototype's app-local data directory, in
`prototype`. Set `QUEUE_TAURI_PROTOTYPE_DATA` to use a separate scratch root.
Close queues deletion and retries pending deletions on relaunch. On Windows
each UUID names a dedicated WebView2 user data directory. On macOS each UUID
selects a WKWebsiteDataStore and deletion uses Wry's
`WebViewExtDarwin::remove_data_store` after closing the webview.

The launcher command is scoped to the trusted launcher webview by Tauri's
capability manifest. Fixture pages can load only the exact loopback URL and
cannot open new windows or invoke the launcher command. The verification
fixture posts synthetic values to an in-process loopback receiver. The
baseline probe stores those synthetic values in its scratch directory and
prints only pass/fail outcomes and timing. The sample link is display-only.

## Repeat the checks

Windows, from PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-windows.ps1
```

macOS 14+ on a logged-in desktop:

```sh
sh ./verify-macos.sh
```

Both scripts run an initial two-view process, relaunch the same session IDs
in a second process, verify deletion by reopening the same UUID, and run
fresh 12- and 50-view batches. They print the scratch path at the end. When
finished, the Windows scratch files can be removed. On macOS, WebKit stores
are managed by UUID outside the scratch directory. Close test sessions in
the launcher using the appropriate scratch path to remove their stores.

## Platform and packaging notes

The child-webview API requires Tauri's `unstable` feature. macOS needs a
native verification run for isolation, persistence, view switching, and
WebKit store removal. The Apple Silicon Rust type check passed on Windows with DOCS_RS=1 to
skip compiling the Objective-C helper. It did not link or launch the app.

The Windows run uses Evergreen WebView2. Tauri documents a
`bundle.windows.webviewInstallMode` option with `type: fixedRuntime` and the
extracted runtime folder for its installer. The existing portable ZIP would
need a separate check using the bundled runtime and the Windows 10 app
container read permissions from the existing packaging script.
[Windows installer documentation](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)

The fixture is the only supported page. Window dimensions are fixed for the
prototype, and session creation is serialized through the UI thread. The
scale measurements use an idle local page and three programmatic switches.
They leave live page workload, prolonged use, and lower-memory machines
unmeasured.

## Windows results

Observed on Windows 11 Pro with 16 GiB RAM, WebView2 Evergreen 157.0.4341.46,
and the unoptimized Rust build. The same-origin checks passed for cookie and
local-storage isolation, retention of the in-memory page on switches,
relaunch with the same UUIDs, and removal of one store without affecting
the other. The fixture had no access to the launcher command, and the
sample link survived both relaunch and deletion. The launcher UI was
also exercised interactively for adding sessions, switching, and saving a link.

| Fixture views | All views ready | Switch and fixture round trip | Browser processes | Total WebView2 processes | Profile files |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 12 | 7.05 s | 25-43 ms | 13 | 78 | 65 MiB |
| 50 | 33.73 s | 18-75 ms | 51 | 306 | 276 MiB |

Ready time runs from application start until every fixture has reported
its cookie and local-storage marker. Process counts include the launcher
webview. A separate 50-view run took 40.24 s to reach readiness. No hard
limit was observed through 50 fixture views. The process tree grows by
roughly six processes per session with this directory-per-view approach.

The Windows sampler observed 2,420 MiB summed working set at 12 views and
1,470 MiB at 50. These are point-in-time resident-page sums that can count
shared pages more than once. The lower snapshot at 50 does not establish a
smaller memory requirement. WebView2 printed a window-class cleanup
diagnostic on shutdown while the verification processes returned success.

The Windows result establishes feasibility for this local workload. The
50-view startup time and process count are material costs of this isolation
strategy. Compare with the existing named WebView2 profile approach and run
the native macOS checks before deciding on a production migration.
