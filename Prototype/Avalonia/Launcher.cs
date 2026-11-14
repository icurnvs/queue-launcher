using System.Diagnostics;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Platform;
using Avalonia.Threading;

namespace QueueLauncher.Prototype;

internal sealed class Launcher : Window
{
    private const string SampleLink = "https://queue-launcher.example/sample";
    private readonly IBrowserStore store = OperatingSystem.IsWindows() ? new WindowsStore() : new MacStore();
    private readonly Grid views = new();
    private readonly ComboBox picker = new() { MinWidth = 130 };
    private readonly Button add = new() { Content = "Add session" };
    private readonly Button remove = new() { Content = "Close session" };
    private readonly TextBlock status = new() { Text = "Starting fixture…", Margin = new Thickness(10, 6) };
    private readonly List<Session> sessions = [];
    private readonly string activePath = Path.Combine(App.Root, "sessions.json");
    private readonly string linkPath = Path.Combine(App.Root, "saved-link.txt");
    private bool busy = true;

    public Launcher()
    {
        Title = "Queue Launcher · Avalonia prototype";
        Width = 980;
        Height = 670;
        MinWidth = 520;
        MinHeight = 430;

        var save = new Button { Content = "Save sample link" };
        save.Click += (_, _) => SaveLink();
        add.Click += async (_, _) => await Run(async () =>
        {
            var session = await AddAsync(Guid.NewGuid(), select: true);
            SaveSessions();
            status.Text = $"Session {sessions.IndexOf(session) + 1} ready";
        });
        remove.Click += async (_, _) => await Run(async () =>
        {
            var selected = sessions[picker.SelectedIndex];
            await RemoveAsync(selected);
            status.Text = File.Exists(linkPath) ? $"Session removed · Saved link {File.ReadAllText(linkPath)}" : "Session removed";
        });
        picker.SelectionChanged += (_, _) =>
        {
            if (picker.SelectedIndex >= 0)
                ShowSession(sessions[picker.SelectedIndex]);
        };
        Closing += (_, e) => e.Cancel = busy;

        var toolbar = new StackPanel
        {
            Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(10),
            Children = { picker, add, remove, save }
        };
        DockPanel.SetDock(toolbar, Dock.Top);
        DockPanel.SetDock(status, Dock.Bottom);
        Content = new DockPanel { Children = { toolbar, status, views } };
        Opened += async (_, _) => await Run(InitializeAsync, initial: true);
        UpdateButtons();
    }

    private async Task Run(Func<Task> operation, bool initial = false)
    {
        if (busy && !initial)
            return;
        busy = true;
        UpdateButtons();
        try
        {
            await operation();
        }
        catch (Exception error)
        {
            status.Text = $"Operation failed ({error.GetType().Name}). See console.";
            Console.Error.WriteLine($"{error.GetType().Name}: {error.Message}");
            App.ExitCode = 1;
        }
        finally
        {
            busy = false;
            UpdateButtons();
            if (App.Mode != "interactive")
                Close();
        }
    }

    private void UpdateButtons()
    {
        picker.IsEnabled = !busy;
        add.IsEnabled = !busy && sessions.Count < 50;
        remove.IsEnabled = !busy && sessions.Count > 0;
    }

    private async Task InitializeAsync()
    {
        var timer = Stopwatch.StartNew();
        var saved = File.Exists(activePath) ? Read<Guid[]>(activePath) : [];
        var count = App.Mode switch { "12" => 12, "50" => 50, _ => 2 };
        if (saved.Length == 0 && !File.Exists(activePath))
        {
            Require(App.Mode is not ("restore" or "settle" or "reopen"), "Run seed first.");
            saved = Enumerable.Range(0, count).Select(_ => Guid.NewGuid()).ToArray();
        }

        foreach (var id in saved)
            await AddAsync(id, select: sessions.Count == 0);
        SaveSessions();
        Console.WriteLine($"Ready: {sessions.Count} views in {timer.Elapsed.TotalSeconds:F2}s");
        status.Text = File.Exists(linkPath) ? $"Saved link {File.ReadAllText(linkPath)}" : $"{sessions.Count} sessions ready";
        if (App.Mode == "interactive")
            return;
        if (App.Mode is "seed" or "restore" or "settle" or "reopen")
            await VerifyPersistenceAsync();
        else
            await VerifyScaleAsync(count);
    }

    private async Task<Session> AddAsync(Guid id, bool select)
    {
        var view = new NativeWebView
        {
            AdapterPreference = [OperatingSystem.IsWindows() ? WebViewAdapterType.WebView2 : WebViewAdapterType.WkWebView]
        };
        var ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        view.EnvironmentRequested += (_, args) => store.Configure(id, args);
        view.NavigationStarted += (_, args) => args.Cancel = args.Request != App.Fixture;
        view.NewWindowRequested += (_, args) => args.Handled = true;
        view.NavigationCompleted += (_, args) =>
        {
            if (args.IsSuccess && args.Request == App.Fixture)
                ready.TrySetResult();
        };
        var session = new Session(id, view);
        views.Children.Add(view);
        try
        {
            view.Source = App.Fixture;
            // Visibility is explicitly changed after the adapter has initialized.
            await ready.Task.WaitAsync(TimeSpan.FromSeconds(25));
#if WINDOWS_BUILD
            if (OperatingSystem.IsWindows() && store is WindowsStore windows)
                windows.CheckRuntime(view, id);
#endif
            sessions.Add(session);
            picker.Items.Add($"Session {sessions.Count}");
            if (select)
                picker.SelectedIndex = sessions.Count - 1;
            else
                view.IsVisible = false;
            return session;
        }
        catch (TimeoutException)
        {
            var reason = (view.AdapterInfo as DetailedWebViewAdapterInfo)?.UnavailableReason;
            views.Children.Remove(view);
            throw new InvalidOperationException($"Fixture did not load. {reason}");
        }
        catch
        {
            views.Children.Remove(view);
            throw;
        }
    }

    private void ShowSession(Session selected)
    {
        foreach (var session in sessions)
            session.View.IsVisible = session == selected;
    }

    private async Task RemoveAsync(Session session)
    {
        await store.DeleteAsync(session.Id, session.View, () => views.Children.Remove(session.View));
        // File replacement prevents partial JSON after interrupted writes.
        picker.SelectedIndex = -1;
        sessions.Remove(session);
        picker.Items.Clear();
        for (var index = 0; index < sessions.Count; index++)
            picker.Items.Add($"Session {index + 1}");
        if (sessions.Count > 0)
            picker.SelectedIndex = 0;
        SaveSessions();
    }

    private void SaveSessions() => WriteJson(activePath, sessions.Select(session => session.Id).ToArray());

    private void SaveLink()
    {
        File.WriteAllText(linkPath, SampleLink);
        status.Text = $"Saved link {SampleLink}";
    }

    private async Task VerifyPersistenceAsync()
    {
        var expectedPath = Path.Combine(App.Root, "expected.json");
        Require(sessions.Count == (App.Mode is ("settle" or "reopen") ? 1 : 2), "Unexpected session count.");
        if (App.Mode == "seed")
        {
            var first = await SnapshotAsync(sessions[0]);
            var second = await SnapshotAsync(sessions[1]);
            Require(Different(first, second), "Initial storage values must differ.");
            await InvokeAsync(sessions[0], "document.querySelector('#human').click(); document.querySelector('#tick').click(); true");
            picker.SelectedIndex = 1;
            await NextFrameAsync();
            Require(await SnapshotAsync(sessions[1]) == second, "Session 2 changed after writing session 1.");
            picker.SelectedIndex = 0;
            await NextFrameAsync();
            first = await SnapshotAsync(sessions[0]);
            Require(first.Admitted && first.Ticks == 1, "Switching lost the live page state.");
            WriteJson(expectedPath, new Evidence(sessions[0].Id, first, sessions[1].Id, second));
            SaveLink();
            await Task.Delay(2000); // Give the browser time to flush its persistent store.
            Console.WriteLine("PASS: distinct cookies and local storage, isolated write, live state survived switching.");
            return;
        }

        var expected = Read<Evidence>(expectedPath);
        if (App.Mode == "restore")
        {
            Require(sessions[0].Id == expected.FirstId && sessions[1].Id == expected.SecondId,
                "Saved IDs did not reopen.");
            Require(await SnapshotAsync(sessions[0]) == expected.First with { Ticks = 0 }
                && await SnapshotAsync(sessions[1]) == expected.Second with { Ticks = 0 },
                "Relaunch did not preserve exactly the persistent state.");
            Console.WriteLine("PASS: both IDs, cookies and local storage survived relaunch. In-page counter reset.");
            await RemoveAsync(sessions[0]);
            Require(await SnapshotAsync(sessions[0]) == expected.Second, "Removing session 1 affected session 2.");
            Console.WriteLine("PASS: store removal returned successfully and the other live session was intact.");
        }
        else
        {
            Require(sessions[0].Id == expected.SecondId && await SnapshotAsync(sessions[0]) == expected.Second,
                "Surviving session failed to restore.");
            if (App.Mode == "reopen")
            {
                var reopened = await AddAsync(expected.FirstId, select: true);
                var clean = await SnapshotAsync(reopened);
                Require(Different(clean, expected.First) && !clean.Admitted && clean.Ticks == 0,
                    "Reopening the deleted ID found stale state.");
                await RemoveAsync(reopened);
                Require(await SnapshotAsync(sessions[0]) == expected.Second, "Removing reopened store affected survivor.");
                Console.WriteLine("PASS: reopening deleted ID produced fresh cookie and storage, surviving ID was intact.");
            }
            else
            {
                Console.WriteLine("PASS: surviving store intact after browser restart.");
            }
        }

        Require(File.ReadAllText(linkPath) == SampleLink, "Saved sample link disappeared.");
        Console.WriteLine("PASS: saved sample link retained.");
    }

    private async Task VerifyScaleAsync(int count)
    {
        Require(sessions.Count == count, "Use a new scratch directory for each scale.");
        var cookies = new HashSet<Guid>();
        var local = new HashSet<Guid>();
        var timer = Stopwatch.StartNew();
        double slowest = 0;
        if (OperatingSystem.IsWindows())
        {
#if WINDOWS_BUILD
            var environment = WindowsStore.Core(sessions[0].View).Environment;
            var processes = environment.GetProcessInfos();
            Console.WriteLine($"WebView2 {environment.BrowserVersionString}: {processes.Count} browser processes ({string.Join(", ", processes.GroupBy(process => process.Kind).Select(group => $"{group.Count()} {group.Key}"))}).");
#endif
        }
        foreach (var session in sessions)
        {
            var start = timer.Elapsed;
            picker.SelectedIndex = sessions.IndexOf(session);
            await NextFrameAsync();
            var snapshot = await SnapshotAsync(session);
            Require(cookies.Add(snapshot.Cookie) && local.Add(snapshot.Local), "Scale run found a shared store.");
            slowest = Math.Max(slowest, (timer.Elapsed - start).TotalMilliseconds);
        }
        Console.WriteLine($"PASS: {count} distinct cookie and local stores. Select + probe total {timer.Elapsed.TotalSeconds:F2}s, slowest {slowest:F0}ms.");
    }

    private static Task NextFrameAsync() => Dispatcher.UIThread.InvokeAsync(() => { }, DispatcherPriority.Background).GetTask();

    private static async Task InvokeAsync(Session session, string script) =>
        await session.View.InvokeScript(script).WaitAsync(TimeSpan.FromSeconds(10));

    private static async Task<Snapshot> SnapshotAsync(Session session)
    {
        var raw = await session.View.InvokeScript("window.queueSnapshot()").WaitAsync(TimeSpan.FromSeconds(10));
        if (string.IsNullOrWhiteSpace(raw))
            throw new InvalidOperationException("Fixture did not return a reading.");
        var result = JsonSerializer.Deserialize<Snapshot>(raw.StartsWith('"') ? JsonSerializer.Deserialize<string>(raw)! : raw)!;
        Require(result.Cookie != Guid.Empty && result.Local != Guid.Empty, "Fixture data was missing.");
        return result;
    }

    private static bool Different(Snapshot first, Snapshot second) =>
        first.Cookie != second.Cookie && first.Local != second.Local;

    private static void Require(bool condition, string message)
    {
        if (!condition)
            throw new InvalidOperationException(message);
    }

    private static T Read<T>(string file) => JsonSerializer.Deserialize<T>(File.ReadAllText(file))!;

    private static void WriteJson<T>(string file, T value)
    {
        File.WriteAllText(file + ".tmp", JsonSerializer.Serialize(value));
        File.Move(file + ".tmp", file, overwrite: true);
    }

    private sealed record Session(Guid Id, NativeWebView View);
    private sealed record Snapshot(Guid Cookie, Guid Local, bool Admitted, int Ticks);
    private sealed record Evidence(Guid FirstId, Snapshot First, Guid SecondId, Snapshot Second);
}
