# Queue Launcher

Queue Launcher keeps multiple waiting-room sessions in one window on Windows
and macOS. Each session has its own browser data, so you can work through
the site's queue and switch between sessions without reloading their pages.
The app is built with Rust and Tauri.

## Download

Get the package for your platform from
[GitHub Releases](https://github.com/icurnvs/queue-launcher/releases/latest).
The downloads include SHA-256 checksum files.

### Windows

Choose one Windows x64 download:

| Package filename ending | Best for | WebView2 browser runtime |
| --- | --- | --- |
| `windows-x64-setup.exe` | **Recommended for most users.** Installs Queue Launcher for your Windows account. | Uses installed Evergreen WebView2; downloads and installs it if missing (requires internet access). |
| `windows-x64-portable-evergreen.zip` | Small portable download. Extract it and run `QueueLauncher.exe` without installing the app. | Evergreen WebView2 must already be installed. If missing, [install it from Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download). |
| `windows-x64-portable.zip` | Self-contained portable download. Extract the entire ZIP and keep all companion files beside `QueueLauncher.exe`. | Includes a fixed WebView2 runtime, making this ZIP much larger. Useful when installing the runtime is unavailable. |

For the **self-contained portable ZIP** on Windows 11, run `QueueLauncher.exe`.
On Windows 10, run `Open on Windows 10.cmd` once to grant the browser sandbox
read access to the bundled runtime and launch the app. The installer and
Evergreen portable ZIP do not need this launcher.

### macOS

Download the macos-arm64.zip, extract it, and move Queue Launcher.app
to Applications. The download requires an Apple Silicon Mac (M1 or newer)
running macOS 14 or later.

The macOS release has an ad-hoc signature but is not notarized. If macOS blocks it,
click Done, then open System Settings > Privacy & Security and click
Open Anyway for Queue Launcher. Confirm Open when prompted. See
[Apple's instructions](https://support.apple.com/en-us/102445).

## Using Queue Launcher

1. Paste a release URL and choose your starting number of sessions. Start with a low number:
   CAPTCHAs expire, so open only as many sessions as you can work through promptly.
2. Click Open sessions. Select a session in the sidebar to view its page.
3. Complete the site's CAPTCHA, then click Mark ready to track that session.
4. Use Add session to add more sessions from the same release URL as you're
   ready to complete their CAPTCHAs.

Open **Settings → Demo mode** to try 12 offline sessions. Demo and live
batches have separate browser profiles, link history, logs, and preferences.

### Tracking sessions

For supported waiting rooms, session cards show queue position, estimated
wait, purchase-window details, and reported errors. Enable Sort by queue
position to put the closest spots first; Best spot marks the session with the fewest
members ahead. Mark ready records the status you set manually.

### Sharing links and resuming a batch

A confirmed waiting-room session with a valid Queue ID can be shared even when
no members-ahead count is displayed. Use Copy to copy its queue link, or Copy and close
to copy it and close the session. Link history keeps saved links available
after sessions close or the app restarts. The site controls whether a shared
link or queue position is still valid.

Quit and reopen the app to restore the batch, selected session, ready states,
and browser profiles. End drop closes all sessions in the active batch
and clears their browser data after confirmation. Saved link history and logs
remain available.

### Logs and local data

**Settings → Drop logs** opens the log folder. Timestamped JSON Lines logs record event
names and numeric details. They exclude URLs, queue IDs, cookies, CAPTCHA
answers, and tokens. Browser profiles and saved links stay on your
computer.

Windows stores application data in
`%LOCALAPPDATA%\local.queue-launcher.tauri`. macOS stores application state in `~/Library/Application Support/local.queue-launcher.tauri`.
WebKit manages session website data.

## Development

See the [development guide](Tauri/README.md) for the source layout, prerequisites,
build commands, packaging, and offline verification.

[Archived implementations and development notes](archive/README.md)

Queue rows show both members ahead and estimated wait when available. Enable
**Sort by position / wait** to use member counts first, then estimated waits for
sessions without a member count. Shorter known waits appear before unknown
waits. The sidebar recognizes estimates such as “more than an hour.”
