# Queue Launcher Electron prototype

This feasibility prototype checks isolated, persistent queue sessions in a single Electron window. It uses Electron 45.3.2 and TypeScript. It serves a copy of the existing local waiting-room fixture on loopback. The saved sample link is a placeholder and stays in the launcher manifest.

## Run it

Use a desktop session with Node.js 22.12 or later. From the repository root:

```sh
cd Prototypes/Electron
npm ci
npm start
```

1. Add two sessions. Each shows its own queue ID and fixture cookie.
2. In the first session, check **I'm human** and click **Increment page-only counter**. Switch to the second session and back. The first keeps its counter and admission state.
3. Click **Save sample link**, close the window, and run `npm start` again. The two session entries, queue IDs, cookies, and admission state survive. The counter resets.
4. Select the first entry and click **Close selected session**. The other session and saved link remain.

The fixture defaults to port 38147 on 127.0.0.1. Keep the same port between manual runs because local storage is scoped to the origin. Manual runs store their manifest and Chromium partitions in Electron's user-data directory for this prototype. Verification uses separate temporary directories and loopback ports.

## Reproduce the checks on Windows or macOS

From `Prototypes/Electron` after `npm ci`:

```sh
npm run verify
```

The runner opens real Electron windows and exits between phases. It checks same-origin cookie and local storage isolation, the sandbox boundary, switching without reloads, persistence by UUID, and selective cleanup. It probes the closed session's original partition at the same origin both before and after a subsequent Electron restart. It also verifies the saved sample link, checks unique queue IDs and cookies across both batch sizes, and confirms the 50-session cap. Output includes aggregate metrics and pass messages.

## Windows results

Windows 11 Pro x64 (build 26200), Ryzen 9 5900X, 16 GiB RAM, Node.js 22.22.0. The verification command exited successfully. These timings are from one run with the offline fixture.

| Views | Operation | Time | Queue renderer processes | Total Electron processes | Summed working set | Switch median / p95 / max |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 12 | Sequential adds | 1.73 s | 12 | 16 | 1,328 MiB | 31 / 34 / 34 ms |
| 12 | Restore to window show | 0.46 s | 12 | 15 | 1,057 MiB | 10 / 104 / 104 ms |
| 50 | Sequential adds | 8.53 s | 50 | 54 | 4,597 MiB | 31 / 52 / 63 ms |
| 50 | Restore to window show | 1.71 s | 50 | 54 | 3,885 MiB | 6 / 31 / 33 ms |

Sequential adds measure the loop of launcher IPC calls, including page loads. Restore time is Electron process uptime when the loaded window is shown. Switch timing starts before the selection IPC call and ends at the selected view's next requestAnimationFrame callback. It measures renderer responsiveness and excludes physical display latency. The working-set sum can count shared pages more than once. The extracted Windows x64 Electron distribution occupied 357 MiB before adding the prototype or an installer.

## macOS and open questions

A macOS host was unavailable. Run the same command on a Mac to verify the fixture, restart and cleanup behavior, process count, and timings. Windows results establish feasibility with simple fixture pages. Production queue pages, lower-memory machines, sustained background use, packaging and signing, and actual disk consumption across repeated session churn remain unmeasured.

Each stable UUID owns a persistent Chromium partition. Closing a session destroys its web contents and clears the partition through Electron. Chromium may leave the partition directory on disk. If physical directory reclamation becomes a requirement, verify it separately after Electron releases the profile. The sandboxed queue views receive only the local fixture and have no launcher preload. The launcher manifest holds the saved sample link separately from browser data.

[Electron session API](https://www.electronjs.org/docs/latest/api/session) · [WebContentsView API](https://www.electronjs.org/docs/latest/api/web-contents-view) · [Electron security guide](https://www.electronjs.org/docs/latest/tutorial/security)
