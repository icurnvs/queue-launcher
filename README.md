# Queue Launcher

## Tauri on Windows

The Rust and Tauri port uses persistent named WebView2 profiles. Install
the Rust MSVC toolchain, Visual Studio C++ build tools, and the WebView2
Evergreen Runtime. From the repository root, run:

```powershell
cargo run --locked --manifest-path Prototype/Tauri/Cargo.toml
```

Start with **Demo mode** for 12 offline sessions. Demo and live batches
keep separate browser profiles, link history, drop logs, and preferences.
In a live batch, paste an HTTP, HTTPS, or local fixture URL and open up to
50 sessions. Sessions stay loaded while you switch between them. The app
restores the last selection, manual ready state, and saved browser
profiles on relaunch.

To create a portable ZIP with a Microsoft-signed Fixed Version runtime:

```powershell
.\Scripts\package-tauri-windows.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab'
```

Extract the ZIP to a local folder. On Windows 11, run **QueueLauncher.exe**.
For Windows 10, the ZIP includes **Open on Windows 10.cmd** to grant the
browser sandbox read access to the runtime folder before launch. For
ARM64, install the Rust ARM64 MSVC target and pass `-Architecture arm64`
with the matching CAB.

The port keeps its data in `%LOCALAPPDATA%\local.queue-launcher.tauri`.
Existing WinForms and Swift data remain in their existing locations.
A new Tauri batch starts with fresh profiles. To use a disposable data
directory for development, set `QUEUE_LAUNCHER_DATA`.

Run the parser and persistence tests, then the Windows offline workflow
check from PowerShell:

```powershell
cargo test --locked --manifest-path Prototype/Tauri/Cargo.toml --bin queue-launcher
$env:QUEUE_LAUNCHER_DATA = Join-Path $env:TEMP ('queue-launcher-test-' + [guid]::NewGuid())
cargo run --locked --manifest-path Prototype/Tauri/Cargo.toml -- --verify-offline
Remove-Item Env:QUEUE_LAUNCHER_DATA
```

The browser isolation and 12- and 50-session benchmark harness remains
available in [the Tauri experiment](Prototype/Tauri/README.md). The next
platform pass is native macOS validation.

## WinForms on Windows

Install the [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0) to build the app and the [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) to run it. From PowerShell in the repository root:

```powershell
dotnet run --project Windows/QueueLauncher.csproj
```

To package a Windows x64 ZIP with .NET and the browser runtime included, download Microsoft's [WebView2 Fixed Version x64 CAB](https://developer.microsoft.com/microsoft-edge/webview2/) and run:

```powershell
.\Scripts\package-windows.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab'
```

Extract the entire ZIP to a local folder. On Windows 11, run **QueueLauncher.exe**. On Windows 10, run **Open on Windows 10.cmd** once to grant the browser sandbox read access to the bundled runtime folder and launch the app. For an ARM64 package, use an ARM64 CAB and pass `-Architecture arm64 -OutputDirectory build/windows-arm64-portable`. Repackage with a newer Fixed Version CAB to update its Chromium engine.

To publish a smaller build that uses the PC’s installed Evergreen WebView2 Runtime:

```powershell
dotnet publish Windows/QueueLauncher.csproj -c Release -r win-x64 --self-contained true -o build/windows-x64
```

Launch **QueueLauncher.exe** from the published folder and keep its companion files alongside it. For Windows on ARM, use `win-arm64` as the runtime identifier. Each session uses its own WebView2 profile. Windows batches and drop logs are stored under `%LOCALAPPDATA%\Queue Launcher`. **Copy link**, **Copy and close**, and **Close session** sit under the selected session’s heading above the browser. Close and **End batch** clear the affected profiles’ browsing data and let WebView2 remove their profile files.

To get the offline test URL on Windows, run:

```powershell
([uri](Resolve-Path Scripts/local-waiting-room.html).Path).AbsoluteUri
```

Click **Demo mode** in the Windows launcher to switch the main window to an offline batch of 12 sessions with sample queue links. Click **Exit demo mode** to return to your live sessions. The demo has its own browser profiles, history, and logs. Copy actions use the Windows clipboard.

Run the Windows queue-status and link checks with `dotnet run --project Tests/Windows`.

## Usage

Paste a release URL, enter a session count from 1 to 50, and click **Open sessions**. The default is five. Each session opens in the same app window with its own browser data.

Choose a session from the sidebar or click **Next session**. Complete the site’s CAPTCHA in the browser pane, then click **Mark ready**. **Add session** opens and selects one more session from the original URL, up to 50 total. Existing pages stay loaded and keep their ready status. **Copy and close** copies the link and closes the session in one action. **End batch** closes the pages and discards the saved browser data after confirmation.

## Local test

Paste the file URL for `Scripts/local-waiting-room.html` into the launcher and open five sessions. To print the URL on macOS, run:

```sh
python3 -c 'from pathlib import Path; print(Path("Scripts/local-waiting-room.html").resolve().as_uri())'
```

Each session should show a different queue ID. Reload a session to confirm its ID stays the same. Use the fixture checkbox to simulate joining, then mark that session ready. Cycle through all five, check that the launcher shows **5 of 5 ready**, then end the batch. The fixture uses local storage and runs offline.

## Swift on macOS

With Xcode or its command-line tools installed, run `Scripts/package-app.sh`. The script builds and locally signs **build/Queue Launcher.app**.

## Queue status and relaunch

On Windows, enable **Sort by queue position** to keep the closest spots at the top. Sessions with no reported position appear below them. **Next session** follows the displayed order.

**Link history** saves each distinct shareable link automatically, with its session number and the position shown when the link was saved. You can copy links from history after closing sessions, ending a batch, or reopening the app. Saved links may expire. Windows stores them locally in `queue-links.json`, separate from the drop logs.

For Costco’s waiting room, the sidebar shows the number of members ahead, the estimated wait, and any purchase window or time remaining displayed on the page. **Best spot** marks the fewest members ahead. Click the link icon on the session row to copy that session’s queue link. The recipient opens the same place in line, subject to Costco’s queue expiry and redirect limits. Attempt-limit and expiry messages appear in the same row, along with the Queue-it error ID when the page provides one.

Quit and reopen the app to restore a batch created in the updated build. The app reuses each session’s browser data and saved waiting-room URL with its queue ID when available. Costco controls whether the queue place and purchase window are still valid. Sessions opened by the earlier temporary-storage build cannot transfer automatically.

## Drop logs

Click **Drop logs** to open the log folder. Each batch writes a timestamped JSON Lines log. Use the session number to compare observed CAPTCHA, waiting-room, queue-position, and error events, including numeric error codes when available. **Mark ready** records a manual action. CAPTCHA visibility is sampled, so a brief prompt may go unrecorded.

Logs survive **End batch** and app relaunch. They contain event names and numeric details. URLs, queue IDs, cookies, CAPTCHA answers, and tokens stay out of the logs.
