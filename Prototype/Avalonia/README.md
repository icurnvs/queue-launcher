# Queue Launcher Avalonia prototype

This is an offline feasibility experiment with a shared Avalonia window, named WebView2 profiles on Windows, and UUID-backed WKWebView stores on macOS 14+. Each view remains attached when another session is selected. The loopback fixture keeps the same HTTP origin across launches. The launcher stores the session list and sample link separately from browser data. Only the local fixture can be navigated to from the host, and the app registers no inbound page-message handlers.

## Run

From this worktree, use .NET 8 SDK. Windows needs WebView2 Evergreen or a Fixed Version runtime. macOS needs macOS 14+ and the Xcode command-line tools (`xcode-select --install`) for the small WebKit cleanup library.

```sh
dotnet run --project Prototype/Avalonia
```

For interactive checks, compare the two queue IDs and cookies. Tick **I'm human** and increment the counter in session 1, switch to 2 and back, then quit and relaunch. The admission state and identifiers should persist while the counter resets. Save the sample link, close session 1, and verify that the link and session 2 remain.

On Windows, the launcher uses an adjacent `WebView2Runtime` folder if present. For the existing portable package, point the prototype at its extracted runtime directory.

```powershell
$env:QUEUE_PROTOTYPE_WEBVIEW2 = 'C:\path\to\WebView2Runtime'
dotnet run --project Prototype/Avalonia
```

## Reproduce the acceptance checks

**Windows PowerShell**

```powershell
dotnet build Prototype/Avalonia
$app = 'Prototype/Avalonia/bin/Debug/net8.0/QueueLauncher.Prototype.exe'
$scratch = Join-Path $env:TEMP ('queue-avalonia-' + [guid]::NewGuid().ToString('N'))
foreach ($mode in 'seed', 'restore', 'settle') {
    & $app $mode $scratch
    if ($LASTEXITCODE) { throw "$mode failed" }
}
& $app reopen $scratch
# If WebView2 returns 0x8007012F, allow its profile deletion to settle and rerun reopen.

foreach ($count in 12, 50) {
    & $app "$count" "$scratch-$count"
    if ($LASTEXITCODE) { throw "$count-view check failed" }
}
```

**macOS 14+**

```sh
dotnet build Prototype/Avalonia
app=Prototype/Avalonia/bin/Debug/net8.0/QueueLauncher.Prototype
scratch="$(mktemp -d)"
for mode in seed restore reopen; do "$app" "$mode" "$scratch" || exit 1; done
"$app" 12 "$scratch-12" && "$app" 50 "$scratch-50"
```

Each scale run needs a fresh scratch directory. Keeping the scratch directory also keeps the fixture origin. If the saved port is occupied, free that port before relaunching to preserve the same cookies and local storage.

## Evidence

**Windows**. Evergreen 157.0.4341.46 and Fixed Version 157.0.4341.60 both passed two-store isolation, the live page counter through a switch, restoration of both IDs and their exact persistent state, and selective deletion while retaining the survivor and sample link. The Fixed run checked the executable path of the actual browser process. The closed profile directory disappeared after browser exit. Reopening the deleted ID later yielded a fresh cookie and queue ID on both runtimes. WebView2 returned `ERROR_DELETE_PENDING` on immediate reuse even when its old directory was already absent, including after a quick extra relaunch. New sessions always get new IDs. Chromium also emitted a window-class teardown diagnostic on exit.

| Evergreen views | Time to initialize | Slowest select + fixture read | Browser processes | Browser data on disk after quit |
| ---: | ---: | ---: | ---: | ---: |
| 12 | 4.85 s | 43 ms | 16 (12 renderers) | 57.2 MiB |
| 50 | 28.16 s | 89 ms | 54 (50 renderers) | 219.0 MiB |

Both scale runs found unique cookies and local-storage values in every view. Another 50-view run took 17.10 s. The samples measure sequential startup and a local script probe, with no hard storage or process ceiling reached at 50. They do not predict behavior on real queue sites or under sustained load.

**macOS**. The adapter selects the UUID store before WKWebView construction and detaches before invoking WebKit's store removal API. macOS build and runtime evidence are pending because the available host is Windows. Run the commands above to verify store retention, on-screen switching, deletion, and scale on a Mac.

Avalonia.Controls.WebView 12.1.2 is MIT-licensed, targets .NET 8, and exposes per-view profile configuration via [EnvironmentRequested](https://docs.avaloniaui.net/controls/web/webview-environment). Cleanup uses [Apple's store removal API](https://developer.apple.com/documentation/webkit/wkwebsitedatastore/remove%28foridentifier%3Acompletionhandler%3A%29?language=o_6) and the WebView2 SDK's profile APIs. On Windows, Avalonia exposes the underlying COM pointer and the prototype uses the pinned Microsoft SDK's nonpublic constructor to wrap it. That dependency needs a supported interop path before production use. Windows demonstrates feasibility with a shared host. The macOS lifecycle and a supported Windows cleanup integration are still open before a migration decision.
