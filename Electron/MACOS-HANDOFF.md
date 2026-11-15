# Queue Launcher macOS handoff

Validate the TypeScript and Electron port on a Mac, fix macOS-specific failures, and produce a locally launchable macOS ZIP. Use the `Electron/` app. The earlier feasibility prototype lives under `Prototypes/Electron/`.

## Get the source

In the Mac clone, fetch the Electron port branch and create a worktree for the macOS changes. The full port was implemented in commit `a774e3c756e98349ec2537a825fb0aefd83df603`.

```sh
git fetch origin
git worktree add -b codex/electron-macos ../queue-launcher-macos origin/codex/electron-port
cd ../queue-launcher-macos/Electron
```


## What already passed on Windows

`npm run verify` passed the parser tests and the real Electron fixture runs on Windows 11 x64. It checked cookie and local storage isolation, switching without page reloads, readiness, Back and Reload, restoration after quitting, selective data wipe and profile-directory removal after restart, separate demo state, sorted cycling, clipboard and link history, drop log privacy, and the 50-session cap. The Windows x64 packaged app loaded the launcher, answered its IPC state request, and closed with exit code zero. macOS and real waiting-room sites remain unverified.

## Verify on the Mac

1. Use an interactive desktop on macOS 13 or later with Node.js 22.12 or newer. Record `sw_vers`, `uname -m`, `node -v`, and `node -p process.arch` so the report distinguishes the Mac architecture from the Node runtime architecture. Use native Node on Apple Silicon for an arm64 build.
2. Run `npm ci` and `npm run verify` from `Electron/`. The tests start several Electron processes with temporary app data and a loopback HTTP fixture. Keep the exact failing command and error if a check fails. Fix issues in the Electron port and rerun the complete suite.
3. Run `npm start`. Use **Use offline fixture** and open two sessions. Change a checkbox and click **Increment page-only counter** in one session. Switch away and back, then quit via **Cmd+Q** and reopen. Confirm that the session selection, queue ID, and persisted checkbox survive relaunch while the counter resets. Close one session and check that the other still works.
4. Exercise the macOS interactions by hand. Check resizing, browser-pane focus, the row context menu, clipboard copy, and **Drop logs** opening in Finder. Confirm that the embedded page stays out of the way of **Link history** and the **End batch** confirmation. Enter and leave **Demo mode**, then check that the live page is still loaded. Verify both window close and **Cmd+Q** lead to clean relaunch. The normal app uses `~/Library/Application Support/Queue Launcher Electron` with separate `Live` and `Demo` folders. Use a fresh macOS account or test data if an existing Electron batch must be preserved.

## Package and smoke-test

From `Electron/`, run `npm run package -- --arch=arm64` on Apple Silicon or use `--arch=x64` for Intel. The host-architecture build is the first deliverable. Report the other architecture separately if you can execute it.

The arm64 output is `build/electron/Queue Launcher-darwin-arm64/Queue Launcher.app` relative to the repository root. Inspect the executable architecture and signature, then launch the bundle through Finder or `open`. Smoke-test the packaged app with the offline fixture, quit, and reopen to verify persistence. If the bundle needs a local signature, use an ad hoc signature for this smoke test and verify again. Developer ID signing and notarization are separate release steps.

```sh
app="../build/electron/Queue Launcher-darwin-arm64/Queue Launcher.app"
file "$app/Contents/MacOS/Queue Launcher"
codesign --verify --deep --strict "$app"
open "$app"
ditto -c -k --keepParent "$app" ../build/electron/Queue-Launcher-macOS-arm64.zip
shasum -a 256 ../build/electron/Queue-Launcher-macOS-arm64.zip
```

Adjust the architecture in the paths for an Intel build. Extract the ZIP into a fresh folder and launch the extracted app once to verify the archive preserves the bundle and its executable permissions.

## Files and completion evidence

- `Electron/src/main.ts` manages the window, IPC, shutdown, native menus, clipboard, and mode switching.
- `Electron/src/browser.ts` manages sandboxed browser views, isolated profiles, navigation, and page observation.
- `Electron/src/store.ts` persists batches and links, records drop logs, and removes closed profile directories on restart.
- `Electron/tests/run.cjs` and `Electron/src/verify.ts` run the offline desktop checks. `Electron/README.md` documents the app and build.

Return the tested macOS version and CPU architecture, verification results, packaged-app smoke results, the ZIP path and SHA-256, and any commits or remaining gaps. Use offline fixtures and sample URLs for this handoff. Drop logs and shared verification output should contain no real queue URLs, queue IDs, cookies, CAPTCHA answers, or tokens. Existing native profiles remain separate.
