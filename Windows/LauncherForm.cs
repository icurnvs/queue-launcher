using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Web.WebView2.Core;

sealed partial class LauncherForm : Form
{
    private const int MaxSessions = 50;
    private readonly TextBox releaseUrl = new() { Dock = DockStyle.Fill, AccessibleName = "Release URL", PlaceholderText = "https://…" };
    private readonly NumericUpDown count = new() { Minimum = 1, Maximum = MaxSessions, Value = 5, Width = 58, AccessibleName = "Session count" };
    private readonly Button open = Button("Open sessions");
    private readonly Button end = Button("End batch");
    private readonly Button logs = Button("Drop logs");
    private readonly Button back = Button("Back");
    private readonly Button reload = Button("Reload");
    private readonly Button add = Button("Add session");
    private readonly Button next = Button("Next session");
    private readonly Button mark = Button("Mark ready");
    private readonly Button copy = Button("Copy link");
    private readonly Button copyClose = Button("Copy and close");
    private readonly Button close = Button("Close session");
    private readonly Button retry = Button("Try again");
    private readonly Label ready = new() { Dock = DockStyle.Fill, AutoSize = true, Padding = new(8) };
    private readonly Label notice = new() { Dock = DockStyle.Fill, AutoSize = true, Padding = new(8), Visible = false };
    private readonly Label browserMessage = new() { AutoSize = true, TextAlign = ContentAlignment.MiddleCenter };
    private readonly Panel browserPane = new() { Dock = DockStyle.Fill, BackColor = SystemColors.Window };
    private readonly FlowLayoutPanel placeholder = new() { AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false };
    private readonly DataGridView sessionList = new()
    {
        Dock = DockStyle.Fill, ReadOnly = true, MultiSelect = false, RowHeadersVisible = false,
        AllowUserToAddRows = false, AllowUserToDeleteRows = false, AllowUserToResizeRows = false,
        SelectionMode = DataGridViewSelectionMode.FullRowSelect,
        ColumnHeadersVisible = false, BackgroundColor = SystemColors.Window, BorderStyle = BorderStyle.None,
        AccessibleName = "Browser sessions"
    };
    private readonly System.Windows.Forms.Timer timer = new() { Interval = 3000 };
    private bool changingRows;
    private bool busy;
    private bool exiting;

    public LauncherForm(string directory)
    {
        live = new LauncherMode(directory);
        offline = new LauncherMode(Path.Combine(directory, "Demo"), isDemo: true);
        mode = live;
        BuildLayout();
        AddSessionContextMenu();

        releaseUrl.TextChanged += (_, _) => RefreshControls();
        sessionList.SelectionChanged += (_, _) =>
        {
            if (!changingRows && sessionList.SelectedRows.Count == 1)
                SelectSession((BrowserSession)sessionList.SelectedRows[0].Tag!);
        };
        open.Click += async (_, _) => await RunAsync(StartAsync);
        add.Click += async (_, _) => await RunAsync(AddAsync);
        end.Click += async (_, _) =>
        {
            if (MessageBox.Show(this, "Close all sessions and discard their saved browser data?",
                    "End batch", MessageBoxButtons.OKCancel, MessageBoxIcon.Warning) == DialogResult.OK)
                await RunAsync(() => RemoveAsync([.. mode.Sessions]));
        };
        close.Click += async (_, _) =>
        {
            if (mode.Selected is { } session) await RunAsync(() => RemoveAsync([session]));
        };
        copy.Click += (_, _) =>
        {
            if (mode.Selected is { } session) CopySession(session);
        };
        copyClose.Click += async (_, _) =>
        {
            if (mode.Selected is { } session) await CopyAndCloseAsync(session);
        };
        next.Click += (_, _) =>
        {
            var ordered = OrderedSessions();
            SelectSession(ordered[(Array.IndexOf(ordered, mode.Selected) + 1) % ordered.Length]);
        };
        mark.Click += (_, _) => ToggleReady();
        back.Click += (_, _) => BrowserAction(session => session.View.GoBack());
        reload.Click += (_, _) => BrowserAction(session =>
        {
            Record("manual_reload", session.Saved.Number);
            session.View.Reload();
        });
        retry.Click += async (_, _) =>
        {
            if (mode.Selected is { } session)
                await RunAsync(async () => await session.RetryAsync(await GetEnvironmentAsync(), browserPane));
        };
        logs.Click += (_, _) => OpenLogs();
        history.Click += (_, _) => OpenLinkHistory();
        demo.Click += async (_, _) => await RunAsync(SwitchModeAsync);
        sortByPosition.CheckedChanged += (_, _) =>
        {
            if (mode.Loaded && !switchingModes)
            {
                try { mode.Store.SaveSortPreference(sortByPosition.Checked); }
                catch (Exception ex) when (IsStorageError(ex))
                {
                    ShowShareFeedback("Couldn’t save the sort preference.");
                }
            }
            RefreshControls();
        };
        Shown += async (_, _) =>
        {
            await RunAsync(RestoreAsync);
            if (!exiting) timer.Start();
        };
        timer.Tick += async (_, _) =>
        {
            activePoll = PollAsync();
            await activePoll;
            activePoll = null;
        };
        FormClosing += (_, args) =>
        {
            if (busy)
            {
                args.Cancel = true;
                return;
            }
            if (mode.NeedsSave && !TrySave(mode.Batch))
            {
                args.Cancel = MessageBox.Show(this, "Close without saving the latest changes?",
                    "Queue Launcher", MessageBoxButtons.YesNo, MessageBoxIcon.Warning) != DialogResult.Yes;
                if (args.Cancel) return;
            }
            exiting = true;
            timer.Dispose();
            foreach (var session in live.Sessions.Concat(offline.Sessions)) session.Closed = true;
        };
        RefreshControls();
    }

    private Uri? Destination => mode.IsDemo ? BrowserSession.DemoUrl : ParseDestination(releaseUrl.Text);

    private static Uri? ParseDestination(string text)
    {
        text = text.Trim();
        if (!Uri.TryCreate(text, UriKind.Absolute, out var uri)) return null;
        if (uri.IsFile)
            return text.StartsWith("file:", StringComparison.OrdinalIgnoreCase) &&
                string.IsNullOrEmpty(uri.Host) && File.Exists(uri.LocalPath) ? uri : null;
        return uri.Scheme is "http" or "https" && uri.Host.Length > 0 ? uri : null;
    }

    private async Task<CoreWebView2Environment> GetEnvironmentAsync() =>
        mode.Environment ??= await CoreWebView2Environment.CreateAsync(userDataFolder: mode.Store.BrowserDirectory);

    private async Task RestoreAsync()
    {
        mode.PendingCleanup = mode.Store.ReadCleanup();
        var saved = mode.Store.ReadBatch();
        try { sortByPosition.Checked = mode.Store.ReadSortPreference(); }
        catch (Exception ex) when (IsStorageError(ex)) { ShowShareFeedback("Couldn’t load the sort preference."); }
        mode.Loaded = true;
        if (mode.IsDemo)
        {
            releaseUrl.Text = BrowserSession.DemoUrl.AbsoluteUri;
            count.Value = 12;
        }
        if (saved is { Sessions.Count: > 0 })
        {
            mode.Batch = saved;
            releaseUrl.Text = saved.Url.AbsoluteUri;
            count.Value = Math.Clamp(saved.Sessions.Count, 1, MaxSessions);
            Record("batch_restored");
            await OpenSessionsAsync(saved.Sessions, saved.Selected, restored: true);
        }
        await CleanupAsync();
    }

    private async Task StartAsync()
    {
        if (mode.Batch is not null || Destination is not { } destination) return;
        var size = (int)count.Value;
        var saved = Enumerable.Range(1, size).Select(number => new SavedSession(Guid.NewGuid(), number)).ToList();
        var started = new SavedBatch(destination, Guid.NewGuid(), size + 1, 1, saved);
        await GetEnvironmentAsync();
        if (!TrySave(started)) return;
        shareFeedback.Text = "";
        mode.LogError = null;
        Record("batch_started");
        await OpenSessionsAsync(saved, 1);
    }

    private async Task AddAsync()
    {
        if (mode.Batch is not { } current || mode.Sessions.Count >= MaxSessions) return;
        var saved = new SavedSession(Guid.NewGuid(), current.NextNumber);
        if (!TrySave(current with
            {
                NextNumber = saved.Number + 1, Selected = saved.Number, Sessions = [.. current.Sessions, saved]
            })) return;
        await OpenSessionsAsync([saved], saved.Number);
    }

    private async Task OpenSessionsAsync(IEnumerable<SavedSession> saved, int? selection, bool restored = false)
    {
        var owner = mode;
        var current = mode.Batch!;
        var added = saved.Select(item => new BrowserSession(item, current.Url,
            (name, code, ahead) => Record(name, item.Number, code, ahead, current.LogId, owner),
            RefreshControls, owner.IsDemo)).ToArray();
        mode.Sessions.AddRange(added);
        mode.Selected = mode.Sessions.FirstOrDefault(session => session.Saved.Number == selection) ?? mode.Sessions.FirstOrDefault();
        RebuildRows();
        foreach (var session in added)
            Record(restored ? "session_restored" : "session_opened", session.Saved.Number);

        try
        {
            var browserEnvironment = await GetEnvironmentAsync();
            await Task.WhenAll(added.Select(session => session.OpenAsync(browserEnvironment, browserPane)));
        }
        catch (Exception ex) when (BrowserSession.IsBrowserError(ex))
        {
            foreach (var session in added) session.ShowInitializationError(ex);
        }
    }

    private void SelectSession(BrowserSession session)
    {
        if (mode.Selected == session) return;
        mode.Selected = session;
        if (mode.Batch is { } current)
        {
            mode.Batch = current with { Selected = session.Saved.Number };
            mode.NeedsSave = !TrySave(mode.Batch);
        }
        UpdateSelection();
        RefreshControls();
    }

    private void ToggleReady()
    {
        if (mode.Selected is not { } session) return;
        session.Saved.Ready = !session.Saved.Ready;
        if (!TrySave(mode.Batch))
        {
            session.Saved.Ready = !session.Saved.Ready;
            return;
        }
        Record(session.Saved.Ready ? "marked_ready" : "marked_pending", session.Saved.Number);
        RefreshControls();
    }

    private async Task RemoveAsync(BrowserSession[] removed)
    {
        var current = mode.Batch!;
        if (!CaptureQueueLinks()) return;
        var remaining = mode.Sessions.Except(removed).ToList();
        var selection = mode.Selected;
        if (selection is not null && removed.Contains(selection))
        {
            var ordered = OrderedSessions();
            var index = Array.IndexOf(ordered, selection);
            selection = ordered.Skip(index + 1).FirstOrDefault(session => !removed.Contains(session))
                ?? ordered.Take(index).LastOrDefault(session => !removed.Contains(session));
        }
        var scheduled = new HashSet<Guid>(mode.PendingCleanup);
        scheduled.UnionWith(removed.Select(session => session.Saved.Profile));
        mode.Store.SaveCleanup(scheduled);
        mode.PendingCleanup = scheduled;
        if (!TrySave(remaining.Count == 0 ? null : current with
            {
                Sessions = remaining.Select(session => session.Saved).ToList(), Selected = selection?.Saved.Number
            })) return;

        foreach (var session in removed)
        {
            session.Closed = true;
            Record("session_closed", session.Saved.Number, batchId: current.LogId);
            try
            {
                if (await session.DeleteProfileAsync()) mode.PendingCleanup.Remove(session.Saved.Profile);
            }
            catch (Exception ex) when (BrowserSession.IsBrowserError(ex)) { }
            finally { session.View.Dispose(); }
        }
        if (remaining.Count == 0) Record("batch_ended", batchId: current.LogId);
        mode.Sessions.RemoveAll(removed.Contains);
        mode.Selected = selection;
        RebuildRows();
        await CleanupAsync();
    }

    private async Task CleanupAsync()
    {
        var active = mode.Sessions.Select(session => session.Saved.Profile).ToHashSet();
        var remaining = mode.PendingCleanup.Where(id => !active.Contains(id)).ToArray();
        foreach (var id in remaining)
        {
            CoreWebView2Controller? controller = null;
            try
            {
                var browserEnvironment = await GetEnvironmentAsync();
                controller = await browserEnvironment.CreateCoreWebView2ControllerAsync(Handle,
                    BrowserSession.ProfileOptions(browserEnvironment, id));
                await BrowserSession.ClearAndDeleteAsync(controller.CoreWebView2.Profile);
                mode.PendingCleanup.Remove(id);
            }
            catch (COMException ex) when ((uint)ex.HResult == 0x8007012F)
            {
                // WebView2 has already accepted the profile deletion.
                mode.PendingCleanup.Remove(id);
            }
            catch (Exception ex) when (BrowserSession.IsBrowserError(ex)) { }
            finally { controller?.Close(); }
        }
        if (remaining.Length > 0 || Directory.Exists(mode.Store.BrowserDirectory)) mode.Store.SaveCleanup(mode.PendingCleanup);
        mode.CleanupError = mode.PendingCleanup.Any(id => !active.Contains(id))
            ? "Some browser profiles could not be cleared. Cleanup will retry when the app reopens."
            : null;
    }

    private async Task PollAsync()
    {
        if (busy || exiting || mode.Sessions.Count == 0) return;
        timer.Stop();
        var snapshot = mode.Sessions.ToArray();
        var urls = snapshot.Select(session => session.Saved.WaitingRoomUrl).ToArray();
        try
        {
            await Task.WhenAll(snapshot.Select(session => session.ObserveAsync()));
            if (exiting) return;
            if (!busy) CaptureQueueLinks();
            mode.NeedsSave |= snapshot.Where((session, index) => !session.Closed && session.Saved.WaitingRoomUrl != urls[index]).Any();
            if (mode.NeedsSave && !busy) TrySave(mode.Batch);
            RefreshControls();
        }
        finally
        {
            if (!exiting) timer.Start();
        }
    }

    private async Task RunAsync(Func<Task> action)
    {
        if (busy || exiting) return;
        busy = true;
        RefreshControls();
        try { await action(); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException ||
            BrowserSession.IsBrowserError(ex))
        {
            mode.StateError = ex is WebView2RuntimeNotFoundException
                ? "Install the Microsoft Edge WebView2 Runtime to open sessions."
                : $"Queue Launcher couldn’t complete the operation. {ex.Message}";
        }
        finally
        {
            busy = false;
            RefreshControls();
        }
    }

    private bool TrySave(SavedBatch? value)
    {
        try
        {
            mode.Store.SaveBatch(value);
            mode.Batch = value;
            mode.NeedsSave = false;
            mode.StateError = null;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            mode.StateError = "Couldn’t save the batch. Check that the app data folder is writable.";
            RefreshControls();
            return false;
        }
    }

    private void Record(string name, int? session = null, int? code = null, int? ahead = null,
        Guid? batchId = null, LauncherMode? owner = null)
    {
        owner ??= mode;
        if ((batchId ?? owner.Batch?.LogId) is not { } id) return;
        try { owner.Store.Log(id, name, session, code, ahead); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            owner.LogError = "Some drop log entries couldn’t be written.";
        }
    }

    private void OpenLogs()
    {
        try
        {
            Directory.CreateDirectory(mode.Store.LogDirectory);
            Process.Start(new ProcessStartInfo(mode.Store.LogDirectory) { UseShellExecute = true });
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            mode.LogError = "Couldn’t open the drop log folder.";
            RefreshControls();
        }
    }

    private void CopySession(BrowserSession session)
    {
        if (CopyLink(session)) ShowShareFeedback($"Copied Session {session.Saved.Number} link.");
    }

    private async Task CopyAndCloseAsync(BrowserSession session)
    {
        if (!CopyLink(session)) return;
        await RunAsync(() => RemoveAsync([session]));
        var result = mode.Sessions.Contains(session) ? "Session is still open." : "Session closed.";
        ShowShareFeedback($"Copied Session {session.Saved.Number} link. {result}");
    }

    private bool CopyLink(BrowserSession session)
    {
        if (busy || session.ShareUrl is not { } url) return false;
        try { Clipboard.SetText(url.AbsoluteUri); }
        catch (ExternalException)
        {
            MessageBox.Show(this, "The clipboard is busy. Try again.", Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return false;
        }
        return true;
    }

    private void ShowShareFeedback(string message)
    {
        shareFeedback.Text = message;
        shareFeedback.Visible = true;
        shareFeedback.AccessibilityObject.RaiseLiveRegionChanged();
    }

    private void BrowserAction(Action<BrowserSession> action)
    {
        if (mode.Selected is not { } session) return;
        try { action(session); }
        catch (Exception ex) when (BrowserSession.IsBrowserError(ex))
        {
            session.ShowInitializationError(ex);
            RefreshControls();
        }
    }

    private BrowserSession[] OrderedSessions() => sortByPosition.Checked
        ? mode.Sessions.OrderBy(session => session.Status?.Ahead is null)
            .ThenBy(session => session.Status?.Ahead).ThenBy(session => session.Saved.Number).ToArray()
        : mode.Sessions.ToArray();

    private void RebuildRows()
    {
        SyncRows(OrderedSessions());
        RefreshControls();
    }

    private void SyncRows(BrowserSession[] ordered)
    {
        if (sessionList.Rows.Cast<DataGridViewRow>().Select(row => row.Tag).SequenceEqual(ordered)) return;
        changingRows = true;
        var first = sessionList.FirstDisplayedScrollingRowIndex;
        var anchor = first < 0 ? null : sessionList.Rows[first].Tag as BrowserSession;
        sessionList.Rows.Clear();
        foreach (var session in ordered)
        {
            var index = sessionList.Rows.Add("");
            sessionList.Rows[index].Tag = session;
        }
        var anchorIndex = Array.IndexOf(ordered, anchor);
        if (anchorIndex >= 0 && sessionList.Visible)
            sessionList.FirstDisplayedScrollingRowIndex = anchorIndex;
        UpdateSelection();
    }

    private void UpdateSelection()
    {
        changingRows = true;
        var row = mode.Selected is null ? null : sessionList.Rows.Cast<DataGridViewRow>().FirstOrDefault(row => row.Tag == mode.Selected);
        if (row is not null)
        {
            if (sessionList.CurrentCell?.RowIndex != row.Index)
                sessionList.CurrentCell = row.Cells[0];
            row.Selected = true;
            if (sessionList.Visible && !row.Displayed)
                sessionList.FirstDisplayedScrollingRowIndex = row.Index;
        }
        changingRows = false;
    }

    private void RefreshControls()
    {
        if (exiting) return;
        SyncRows(OrderedSessions());
        var best = mode.Sessions.Min(session => session.Status?.Ahead);
        foreach (DataGridViewRow row in sessionList.Rows)
        {
            var session = (BrowserSession)row.Tag!;
            var status = session.Status;
            var lines = new List<string> { $"Session {session.Saved.Number} · {(session.Saved.Ready ? "Ready" : "Pending")}" };
            if (status?.Message is { } message) lines.Add(message);
            if (status?.ErrorId is { } code) lines.Add($"Queue-it error {code}");
            if (status?.Ahead is { } ahead) lines.Add($"{ahead:N0} ahead{(ahead == best ? " · Best spot" : "")}");
            if (status?.EstimatedWait is { } wait) lines.Add($"Estimated wait {wait}");
            if (status?.TimeRemaining is { } left) lines.Add($"{left} left");
            else if (status?.PurchaseWindow is { } window) lines.Add($"{window} once admitted");
            if (session.ShareUrl is not null) lines.Add("Share link available");
            var text = string.Join(Environment.NewLine, lines);
            if (!Equals(row.Cells[0].Value, text)) row.Cells[0].Value = text;
            var height = SessionRowHeight(row, lines);
            if (row.Height != height) row.Height = height;
        }

        var active = mode.Selected;
        foreach (var session in mode.Sessions)
        {
            if (session.View.IsDisposed) continue;
            session.View.Visible = session == active && !session.Opening && session.Error is null &&
                session.View.CoreWebView2 is not null;
        }
        placeholder.Visible = active is null || !active.View.Visible;
        RefreshLayout(active);
        if (active is not null && busy) UpdateSelection();
        retry.Visible = active?.Error is not null;
        retry.Enabled = !busy;
        if (placeholder.Visible) placeholder.BringToFront();
        releaseUrl.Enabled = !mode.IsDemo && mode.Loaded && !busy && mode.Batch is null;
        count.Enabled = mode.Loaded && !busy && mode.Batch is null;
        open.Enabled = count.Enabled && Destination is not null;
        add.Enabled = !busy && mode.Batch is not null && mode.Sessions.Count < MaxSessions;
        history.Enabled = !busy;
        demo.Enabled = mode.Loaded && !busy;
        end.Enabled = !busy && mode.Sessions.Count > 0;
        next.Enabled = !busy && mode.Sessions.Count > 1;
        close.Enabled = mark.Enabled = !busy && active is not null;
        copy.Enabled = copyClose.Enabled = !busy && active?.ShareUrl is not null;
        copy.AccessibleName = active is null ? copy.Text : $"Copy link for Session {active.Saved.Number}";
        copyClose.AccessibleName = active is null ? copyClose.Text : $"Copy link and close Session {active.Saved.Number}";
        back.Enabled = !busy && active?.CanGoBack == true;
        reload.Enabled = !busy && active is { Error: null, Opening: false } && active.View.CoreWebView2 is not null;
        mark.Text = active?.Saved.Ready == true ? "Mark pending" : "Mark ready";
        notice.Text = string.Join("  ", new[] { mode.StateError, mode.CleanupError, mode.LogError, mode.HistoryError }.OfType<string>());
        notice.Visible = notice.Text.Length > 0;
    }

}
