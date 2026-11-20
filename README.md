# Queue Launcher

Queue Launcher is a desktop app for Windows and macOS that keeps multiple
waiting-room sessions in one window. Each session has its own browser data,
so you can work through the site's queue and switch between sessions without
reloading their pages. The app is built with Rust and Tauri.

## Download

Get the ZIP for your platform from
[GitHub Releases](https://github.com/icurnvs/queue-launcher/releases/latest).
SHA-256 checksum files are included with the downloads.

### Windows

Download the **windows-x64-portable.zip** and extract the entire ZIP to a
local folder. Keep its companion files alongside the executable.

- **Windows 11:** run **QueueLauncher.exe**.
- **Windows 10:** run **Open on Windows 10.cmd** once to grant the browser
  sandbox read access to the bundled runtime and launch the app.

The portable download includes Microsoft's WebView2 runtime.

### macOS

Download the **macos-arm64.zip**, extract it, and move **Queue Launcher.app**
to Applications. The download requires an **Apple Silicon Mac (M1 or newer)**
running **macOS 14 or later**.

The macOS release is ad-hoc signed and is not notarized. If macOS blocks it,
click **Done**, then open **System Settings → Privacy & Security** and click
**Open Anyway** for Queue Launcher. Confirm **Open** when prompted. See
[Apple's instructions](https://support.apple.com/en-us/102445).

## Use Queue Launcher

1. Paste a release URL and choose **1 to 50 sessions**.
2. Click **Open sessions**. Select a session in the sidebar to view its page.
3. Complete the site's CAPTCHA, then click **Mark ready** to track that session.
4. Use **Add session** to open another session from the same release URL.

Try **Demo mode** to explore the app with 12 offline sessions. Demo and live
batches have separate browser profiles, link history, logs, and preferences.

### Navigate and track sessions

The browser toolbar shows the selected session's current URL, including
redirects. Select the address to copy it. **Back** and **Forward** follow that
session's history. **Reload** changes to **Stop loading** while a page loads.

For supported waiting rooms, session cards show queue position, estimated
wait, purchase-window details, and reported errors. Enable **Sort by queue
position** to put the closest spots first; **Best spot** marks the fewest
members ahead. **Mark ready** records your manual status.

### Share links and resume

Use **Copy** to copy a session's shareable queue link, or **Copy and close**
to copy it and close the session. **Link history** keeps saved links available
after sessions close or the app restarts. The site controls whether a shared
link or queue position is still valid.

Quit and reopen the app to restore the batch, selected session, ready states,
and browser profiles. **End drop** closes all sessions in the active batch
and clears their browser data after confirmation. Saved link history and logs
remain available.

### Logs and local data

**Drop logs** opens the log folder. Timestamped JSON Lines logs record event
names and numeric details. URLs, queue IDs, cookies, CAPTCHA answers, and tokens
are excluded from these logs. Browser profiles and saved links stay on your
computer.

On Windows, application data is stored in
`%LOCALAPPDATA%\local.queue-launcher.tauri`. On macOS, application state is
stored in `~/Library/Application Support/local.queue-launcher.tauri`, with
session website data managed by WebKit.

## Development

See the [development guide](Tauri/README.md) for source layout, prerequisites,
build commands, packaging, and offline verification.

[Archived implementations and development notes](archive/README.md)
