# Queue Launcher

Open **build/Queue Launcher.app**, paste a release URL, enter a session count from 1 to 50, and click **Open sessions**. The default is five. Each session opens in the same app window with its own isolated WebKit data store.

Choose a session from the sidebar or click **Next session**. Complete the site’s CAPTCHA in the browser pane, then click **Mark ready**. **Add session** opens and selects one more session from the original URL, up to 50 total. Existing pages stay loaded and keep their ready status. Use the arrow in a square next to the link icon to **Copy and close** in one click. The **×** closes the session by itself. **End batch** closes the pages and discards the saved browser data after confirmation.

## Local test

Paste the file URL for `Scripts/local-waiting-room.html` into the launcher and open five sessions. To print the URL from this directory, run:

```sh
python3 -c 'from pathlib import Path; print(Path("Scripts/local-waiting-room.html").resolve().as_uri())'
```

Each session should show a different queue ID. Reload a session to confirm its ID stays the same. Use the fixture checkbox to simulate joining, then mark that session ready. Cycle through all five, check that the launcher shows **5 of 5 ready**, then end the batch. The fixture uses local storage and runs offline.

## Rebuild

With Xcode or its command-line tools installed, run `Scripts/package-app.sh`. The script builds and locally signs the app.

## Queue status and relaunch

For Costco’s waiting room, the sidebar shows the number of members ahead, the estimated wait, and any purchase window or time remaining displayed on the page. **Best spot** marks the fewest members ahead. Click the link icon on the session row to copy that session’s queue link. The recipient opens the same place in line, subject to Costco’s queue expiry and redirect limits. Attempt-limit and expiry messages appear in the same row, along with the Queue-it error ID when the page provides one.

Quit and reopen the app to restore a batch created in the updated build. The app reuses each session’s browser data and saved waiting-room URL with its queue ID when available. Costco controls whether the queue place and purchase window are still valid. Sessions opened by the earlier temporary-storage build cannot transfer automatically.

## Drop logs

Click **Drop logs** to open the log folder in Finder. Each batch writes a timestamped JSON Lines log. Use the session number to compare observed CAPTCHA, waiting-room, queue-position, and error events, including numeric error codes when available. **Mark ready** records a manual action. CAPTCHA visibility is sampled, so a brief prompt may go unrecorded.

Logs survive **End batch** and app relaunch. They contain event names and numeric details. URLs, queue IDs, cookies, CAPTCHA answers, and tokens stay out of the logs.
