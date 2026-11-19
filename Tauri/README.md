# Queue Launcher Rust/Tauri app and browser harness

Rust/Tauri is the primary application on `main`. For app builds, usage, and
the Windows portable ZIP, see the [repository README](../README.md).
The application lives in the top-level `Tauri` directory. The notes below
describe the earlier browser feasibility harness and its historical results.

This offline feasibility prototype exercises persistent browser sessions in one
native window. It uses a small HTML and JavaScript launcher with Tauri 2 and
Rust. It was branched from Queue Launcher commit `dbec19d`.

## Run

Enter this directory and run `cargo run --locked --bin queue-launcher-tauri-prototype`. On Windows, use an MSVC
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

By default each script runs an initial two-view process, relaunches with
the same session IDs, verifies deletion by reopening the same UUID, and
runs fresh 12- and 50-view batches. They print the scratch path at the end. When
finished, the Windows scratch files can be removed. On macOS, WebKit stores
are managed by UUID outside the scratch directory. Close test sessions in
the launcher using the appropriate scratch path to remove their stores.

## Platform and packaging notes

The child-webview API requires Tauri's `unstable` feature. macOS needs a
native verification run for isolation, persistence, view switching, and
WebKit store removal. The Apple Silicon Rust type check passed on Windows with DOCS_RS=1 to
skip compiling the Objective-C helper. It did not link or launch the app.

The directory-versus-profile comparison uses Evergreen WebView2. Tauri documents a
`bundle.windows.webviewInstallMode` option with `type: fixedRuntime` and the
extracted runtime folder for its installer. The named-profile check below uses the runtime from the existing portable
ZIP on Windows 11. A Tauri ZIP build and the Windows 10 app container
permissions remain untested.
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

Both modes passed the same isolation, switching, relaunch, and cleanup
checks on Windows. These are consecutive runs from the same debug build,
with a fresh data root for each scale run.

| Storage mode | Fixture views | Created | All views ready | Switch and fixture round trip | Browser processes | Total WebView2 processes | Profile files |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Per-session directories | 12 | 6.03 s | 6.16 s | 17-36 ms | 13 | 78 | 70 MiB |
| Named profiles | 12 | 3.01 s | 3.77 s | 16-28 ms | 2 | 23 | 42 MiB |
| Per-session directories | 50 | 25.52 s | 25.60 s | 27-78 ms | 51 | 306 | 280 MiB |
| Named profiles | 50 | 10.13 s | 11.80 s | 24-62 ms | 2 | 61 | 171 MiB |

Creation and readiness times start at application launch. Readiness is
when every fixture has reported its storage markers. Process counts
include the launcher webview. Named mode had one browser process for the
launcher and one for the shared session folder at both sizes. Previous
directory-mode runs reached 50 views in 33.73 s and 40.24 s, so treat
these timings as observations from this host.

At 50 views, summed working-set snapshots were 2,735 MiB for directories
and 3,126 MiB for named profiles. Summing working sets can count shared
pages more than once. These snapshots do not establish a memory saving.
WebView2 printed its window-class cleanup diagnostic at shutdown in both
modes while the verification processes returned success.

## Windows named profiles

Run `cargo run --locked --bin queue-launcher-tauri-prototype -- --named-profiles` to try the second Windows storage
mode. It keeps its sessions and sample links under the `named-profiles`
subdirectory of the prototype data root. Each session UUID becomes a WebView2
profile name in one shared user data folder. The launcher keeps its own
WebView2 environment and its existing Tauri permissions. The session panes
use Wry's Windows profile option directly because Tauri 2.12.5 does not
forward it through its child-webview builder. The fixture panes have no
launcher IPC handler.

Run both modes from the same build on Windows:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-windows.ps1 -NamedProfiles
powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-windows.ps1 -SkipBuild
```

On close, WebView2 clears the session profile's browsing data, then marks
the profile for deletion. The test reads the fixture's cookies and local
storage back after clearing, verifies that the other session's data and
in-memory page survive, and verifies after relaunch that the deleted
profile's directory is gone. On this runtime, reopening the old name
still returned `ERROR_DELETE_PENDING` after the directory disappeared.
The test checks that a fresh UUID starts with empty storage. Normal
session creation always uses a new UUID.
[WebView2 profile deletion documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile8?view=webview2-1.0.3912.50)


## Windows Fixed Version check

Use the extracted runtime folder from the existing Queue Launcher portable
bundle. The verification script sets `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`
for the run and confirms the executable path of every parent WebView2
browser process at both scale checkpoints.
[Microsoft Fixed Version distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution#the-fixed-version-runtime-distribution-mode)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\verify-windows.ps1 -NamedProfiles -FixedRuntimePath 'C:\path\to\WebView2Runtime'
```

Named profiles passed isolation, no-reload switching, relaunch persistence,
and targeted cleanup with the bundled x64 WebView2 157.0.4341.60 on
Windows 11. The old name still returned `ERROR_DELETE_PENDING` after
its directory was removed. A new UUID opened with empty storage.

| Fixture views | Created | All views ready | Switch and fixture round trip | Browser processes | Total WebView2 processes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 12 | 3.30 s | 3.90 s | 24-33 ms | 2 | 24 |
| 50 | 10.81 s | 11.94 s | 28-54 ms | 2 | 62 |

Both browser processes used the bundled executable at both sizes. The
processes returned success with the same WebView2 window-class diagnostic
observed at Evergreen shutdown. These are single-run measurements.
