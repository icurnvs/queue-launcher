# Queue Launcher for Windows and macOS

This is the TypeScript and Electron port of Queue Launcher. It bundles Chromium and opens up to 50 sessions in one window. Each session has its own persistent browser profile. The launcher keeps each page loaded while you switch between sessions. Complete any CAPTCHA in the page manually.

## Run from source

Use Node.js 22.12 or newer and an interactive desktop session. From the repository root:

```sh
cd Electron
npm ci
npm start
```

1. Enter an HTTP or HTTPS release URL and a count from 1 to 50. Click **Open sessions**.
2. Select a session or click **Next session**. Use **Mark ready** and **Mark pending** to track manual progress. **Back**, **Reload**, and **Try again** work within the selected profile.
3. Enable **Sort by queue position** to sort reported positions before unreported positions. **Next session** follows that order. When the Costco waiting room reports a position, the launcher also shows any available estimate, purchase window, countdown, and error code.
4. Use **Copy link** or **Copy and close** when a supported waiting-room link is available. Right-click a row for session actions. **Link history** keeps distinct links by batch and session, including the position when first saved. The waiting room controls expiry and eligibility.
5. Quit to restore the batch on relaunch. **Close session** clears one profile, and **End batch** clears the active batch after confirmation. Links and drop logs survive both operations.

**Use offline fixture** fills in the bundled fixture URL for a regular batch. Check the fixture’s **I'm human** box to simulate joining the queue. **Demo mode** opens an independent batch of 12 simulated waiting-room sessions. Its browser profiles, saved links, logs, and sort preference are separate. Switch back to return to the loaded live batch. Demo links use the reserved **.example** domain. Local testing uses the built-in fixture in place of a file URL.

## Data and logs

The port stores its data in **Queue Launcher Electron**, under **%APPDATA%** on Windows or **~/Library/Application Support** on macOS. It starts with fresh profiles and does not import native or prototype sessions. An open batch saves its original URL, profile IDs, ready flags, selection, and supported resume URLs. Chromium stores each profile separately.

Closing a session clears its browsing data immediately. The launcher records pending removals before teardown and deletes closed profile directories on the next launch. **Drop logs** opens the active mode’s log directory. Each batch writes JSON Lines with timestamps, event names, session numbers, and numeric queue details. Logs exclude URLs, queue IDs, cookies, CAPTCHA answers, and tokens. Saved queue links live in a separate local history file.

## Build a distributable

Run on the target desktop from this directory:

```sh
npm ci
npm run package
```

- **Windows** produces **build/electron/Queue Launcher-win32-x64** or the corresponding **arm64** directory at the repository root. Keep the directory intact and open **Queue Launcher.exe**. Neither Node.js nor WebView2 is needed to run the packaged app.
- **macOS** produces **Queue Launcher.app** inside the Darwin output directory. Run the build on a Mac for the intended architecture. Pass **-- --arch=arm64** or **-- --arch=x64** to choose explicitly. A local ad hoc signature can be applied with **codesign --force --deep --sign - "/path/to/Queue Launcher.app"**. Developer ID signing and notarization are separate release steps.

Electron 45 requires macOS 13 or later. Distribution builds are 64-bit. [Electron platform requirements](https://www.electronjs.org/docs/latest/breaking-changes/)

## Verify the port

Run **npm run verify** from this directory. The suite opens real Electron windows with disposable data and uses an HTTP loopback page for cookie and navigation checks. It checks batch restoration, selective cleanup, native clipboard copies with sample URLs, sorted cycling, independent demo history and logs, and the 50-session cap. It uses offline fixtures throughout. Live provider behavior and macOS execution require separate verification.
