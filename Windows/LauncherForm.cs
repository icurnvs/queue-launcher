using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Web.WebView2.Core;

sealed partial class LauncherForm : Form
{
    private const int MaxSessions = 50;
    private readonly BatchStore store;
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
    private readonly List<BrowserSession> sessions = [];
    private HashSet<Guid> pendingCleanup = [];
    private CoreWebView2Environment? environment;
    private SavedBatch? batch;
    private BrowserSession? selected;
    private bool changingRows;
    private bool busy;
    private bool exiting;
    private bool stateLoaded;
    private bool needsSave;
    private string? stateError;
    private string? cleanupError;
    private string? logError;

    public LauncherForm(string directory)
    {
        store = new BatchStore(directory);
        BuildLayout();

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
                await RunAsync(() => RemoveAsync([.. sessions]));
        };
        close.Click += async (_, _) =>
        {
            if (selected is { } session) await RunAsync(() => RemoveAsync([session]));
        };
        copy.Click += (_, _) => CopySelected();
        copyClose.Click += async (_, _) =>
        {
            if (selected is { } session && CopySelected()) await RunAsync(() => RemoveAsync([session]));
        };
        next.Click += (_, _) => SelectSession(sessions[(sessions.IndexOf(selected!) + 1) % sessions.Count]);
        mark.Click += (_, _) => ToggleReady();
        back.Click += (_, _) => BrowserAction(session => session.View.GoBack());
        reload.Click += (_, _) => BrowserAction(session =>
        {
            Record("manual_reload", session.Saved.Number);
            session.View.Reload();
        });
        retry.Click += async (_, _) =>
        {
            if (selected is { } session)
                await RunAsync(async () => await session.RetryAsync(await GetEnvironmentAsync(), browserPane));
        };
        logs.Click += (_, _) => OpenLogs();
        Shown += async (_, _) =>
        {
            await RunAsync(RestoreAsync);
            if (!exiting) timer.Start();
        };
        timer.Tick += async (_, _) => await PollAsync();
        FormClosing += (_, args) =>
        {
            if (busy)
            {
                args.Cancel = true;
                return;
            }
            if (needsSave && !TrySave(batch))
            {
                args.Cancel = MessageBox.Show(this, "Close without saving the latest changes?",
                    "Queue Launcher", MessageBoxButtons.YesNo, MessageBoxIcon.Warning) != DialogResult.Yes;
                if (args.Cancel) return;
            }
            exiting = true;
            timer.Dispose();
            foreach (var session in sessions) session.Closed = true;
        };
        RefreshControls();
    }

    private Uri? Destination => ParseDestination(releaseUrl.Text);

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
        environment ??= await CoreWebView2Environment.CreateAsync(userDataFolder: store.BrowserDirectory);

    private async Task RestoreAsync()
    {
        pendingCleanup = store.ReadCleanup();
        var saved = store.ReadBatch();
        stateLoaded = true;
        if (saved is { Sessions.Count: > 0 })
        {
            batch = saved;
            releaseUrl.Text = saved.Url.AbsoluteUri;
            count.Value = Math.Clamp(saved.Sessions.Count, 1, MaxSessions);
            Record("batch_restored");
            await OpenSessionsAsync(saved.Sessions, saved.Selected, restored: true);
        }
        await CleanupAsync();
    }

    private async Task StartAsync()
    {
        if (batch is not null || Destination is not { } destination) return;
        var size = (int)count.Value;
        var saved = Enumerable.Range(1, size).Select(number => new SavedSession(Guid.NewGuid(), number)).ToList();
        var started = new SavedBatch(destination, Guid.NewGuid(), size + 1, 1, saved);
        await GetEnvironmentAsync();
        if (!TrySave(started)) return;
        logError = null;
        Record("batch_started");
        await OpenSessionsAsync(saved, 1);
    }

    private async Task AddAsync()
    {
        if (batch is not { } current || sessions.Count >= MaxSessions) return;
        var saved = new SavedSession(Guid.NewGuid(), current.NextNumber);
        if (!TrySave(current with
            {
                NextNumber = saved.Number + 1, Selected = saved.Number, Sessions = [.. current.Sessions, saved]
            })) return;
        await OpenSessionsAsync([saved], saved.Number);
    }

    private async Task OpenSessionsAsync(IEnumerable<SavedSession> saved, int? selection, bool restored = false)
    {
        var current = batch!;
        var added = saved.Select(item => new BrowserSession(item, current.Url,
            (name, code, ahead) => Record(name, item.Number, code, ahead), RefreshControls)).ToArray();
        sessions.AddRange(added);
        selected = sessions.FirstOrDefault(session => session.Saved.Number == selection) ?? sessions.FirstOrDefault();
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
        if (selected == session) return;
        selected = session;
        if (batch is { } current)
        {
            batch = current with { Selected = session.Saved.Number };
            needsSave = !TrySave(batch);
        }
        UpdateSelection();
        RefreshControls();
    }

    private void ToggleReady()
    {
        if (selected is not { } session) return;
        session.Saved.Ready = !session.Saved.Ready;
        if (!TrySave(batch))
        {
            session.Saved.Ready = !session.Saved.Ready;
            return;
        }
        Record(session.Saved.Ready ? "marked_ready" : "marked_pending", session.Saved.Number);
        RefreshControls();
    }

    private async Task RemoveAsync(BrowserSession[] removed)
    {
        var current = batch!;
        var remaining = sessions.Except(removed).ToList();
        var selection = selected;
        if (selection is not null && removed.Contains(selection))
            selection = remaining.Count == 0 ? null : remaining[Math.Min(sessions.IndexOf(selection), remaining.Count - 1)];
        var scheduled = new HashSet<Guid>(pendingCleanup);
        scheduled.UnionWith(removed.Select(session => session.Saved.Profile));
        store.SaveCleanup(scheduled);
        pendingCleanup = scheduled;
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
                if (await session.DeleteProfileAsync()) pendingCleanup.Remove(session.Saved.Profile);
            }
            catch (Exception ex) when (BrowserSession.IsBrowserError(ex)) { }
            finally { session.View.Dispose(); }
        }
        if (remaining.Count == 0) Record("batch_ended", batchId: current.LogId);
        sessions.RemoveAll(removed.Contains);
        selected = selection;
        RebuildRows();
        await CleanupAsync();
    }

    private async Task CleanupAsync()
    {
        var active = sessions.Select(session => session.Saved.Profile).ToHashSet();
        var remaining = pendingCleanup.Where(id => !active.Contains(id)).ToArray();
        foreach (var id in remaining)
        {
            CoreWebView2Controller? controller = null;
            try
            {
                var browserEnvironment = await GetEnvironmentAsync();
                controller = await browserEnvironment.CreateCoreWebView2ControllerAsync(Handle,
                    BrowserSession.ProfileOptions(browserEnvironment, id));
                await BrowserSession.ClearAndDeleteAsync(controller.CoreWebView2.Profile);
                pendingCleanup.Remove(id);
            }
            catch (COMException ex) when ((uint)ex.HResult == 0x8007012F)
            {
                // WebView2 has already accepted the profile deletion.
                pendingCleanup.Remove(id);
            }
            catch (Exception ex) when (BrowserSession.IsBrowserError(ex)) { }
            finally { controller?.Close(); }
        }
        if (remaining.Length > 0 || Directory.Exists(store.BrowserDirectory)) store.SaveCleanup(pendingCleanup);
        cleanupError = pendingCleanup.Any(id => !active.Contains(id))
            ? "Some browser profiles could not be cleared. Cleanup will retry when the app reopens."
            : null;
    }

    private async Task PollAsync()
    {
        if (busy || exiting || sessions.Count == 0) return;
        timer.Stop();
        var snapshot = sessions.ToArray();
        var urls = snapshot.Select(session => session.Saved.WaitingRoomUrl).ToArray();
        try
        {
            await Task.WhenAll(snapshot.Select(session => session.ObserveAsync()));
            if (exiting) return;
            needsSave |= snapshot.Where((session, index) => !session.Closed && session.Saved.WaitingRoomUrl != urls[index]).Any();
            if (needsSave && !busy) TrySave(batch);
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
            stateError = ex is WebView2RuntimeNotFoundException
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
            store.SaveBatch(value);
            batch = value;
            needsSave = false;
            stateError = null;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            stateError = "Couldn’t save the batch. Check that the app data folder is writable.";
            RefreshControls();
            return false;
        }
    }

    private void Record(string name, int? session = null, int? code = null, int? ahead = null, Guid? batchId = null)
    {
        if ((batchId ?? batch?.LogId) is not { } id) return;
        try { store.Log(id, name, session, code, ahead); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            logError = "Some drop log entries couldn’t be written.";
        }
    }

    private void OpenLogs()
    {
        try
        {
            Directory.CreateDirectory(store.LogDirectory);
            Process.Start(new ProcessStartInfo(store.LogDirectory) { UseShellExecute = true });
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            logError = "Couldn’t open the drop log folder.";
            RefreshControls();
        }
    }

    private bool CopySelected()
    {
        if (selected?.ShareUrl is not { } url) return false;
        try { Clipboard.SetText(url.AbsoluteUri); }
        catch (ExternalException)
        {
            MessageBox.Show(this, "The clipboard is busy. Try again.", Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return false;
        }
        return true;
    }

    private void BrowserAction(Action<BrowserSession> action)
    {
        if (selected is not { } session) return;
        try { action(session); }
        catch (Exception ex) when (BrowserSession.IsBrowserError(ex))
        {
            session.ShowInitializationError(ex);
            RefreshControls();
        }
    }

    private void RebuildRows()
    {
        changingRows = true;
        sessionList.Rows.Clear();
        foreach (var session in sessions)
        {
            var index = sessionList.Rows.Add("");
            sessionList.Rows[index].Tag = session;
        }
        changingRows = false;
        UpdateSelection();
        RefreshControls();
    }

    private void UpdateSelection()
    {
        changingRows = true;
        var index = selected is null ? -1 : sessions.IndexOf(selected);
        if (index >= 0 && !sessionList.Rows[index].Selected)
        {
            sessionList.CurrentCell = sessionList.Rows[index].Cells[0];
            sessionList.Rows[index].Selected = true;
        }
        changingRows = false;
    }

    private void RefreshControls()
    {
        if (exiting) return;
        var best = sessions.Min(session => session.Status?.Ahead);
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
            var text = string.Join(Environment.NewLine, lines);
            if (!Equals(row.Cells[0].Value, text)) row.Cells[0].Value = text;
            var height = SessionRowHeight(row, lines);
            if (row.Height != height) row.Height = height;
        }

        var active = selected;
        foreach (var session in sessions)
        {
            if (session.View.IsDisposed) continue;
            session.View.Visible = session == active && !session.Opening && session.Error is null &&
                session.View.CoreWebView2 is not null;
        }
        placeholder.Visible = active is null || !active.View.Visible;
        RefreshLayout(active);
        retry.Visible = active?.Error is not null;
        retry.Enabled = !busy;
        if (placeholder.Visible) placeholder.BringToFront();
        releaseUrl.Enabled = count.Enabled = stateLoaded && !busy && batch is null;
        open.Enabled = releaseUrl.Enabled && Destination is not null;
        add.Enabled = !busy && batch is not null && sessions.Count < MaxSessions;
        end.Enabled = !busy && sessions.Count > 0;
        next.Enabled = !busy && sessions.Count > 1;
        close.Enabled = mark.Enabled = !busy && active is not null;
        copy.Enabled = copyClose.Enabled = !busy && active?.ShareUrl is not null;
        back.Enabled = !busy && active?.CanGoBack == true;
        reload.Enabled = !busy && active is { Error: null, Opening: false } && active.View.CoreWebView2 is not null;
        mark.Text = active?.Saved.Ready == true ? "Mark pending" : "Mark ready";
        notice.Text = string.Join("  ", new[] { stateError, cleanupError, logError }.OfType<string>());
        notice.Visible = notice.Text.Length > 0;
    }

}
