using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.WinForms;

sealed class BrowserSession(SavedSession saved, Uri startUrl, Action<string, int?, int?> log, Action refresh, bool demo = false)
{
    public static readonly Uri DemoUrl = new("https://queue-launcher.example/local-waiting-room.html");
    private const string ReadPage = """
        ({
            text: document.body?.innerText ?? '',
            errorID: document.body?.getAttribute('data-errorid') ?? '',
            challengeVisible: Array.from(document.querySelectorAll('iframe')).some(frame =>
                /recaptcha/i.test(frame.title) && frame.getClientRects().length > 0 &&
                getComputedStyle(frame).visibility === 'visible')
        })
        """;

    private QueueObservation? lastObservation;
    private CoreWebView2Profile? profile;
    private ulong navigationId;
    private bool navigating;
    private int documentVersion;
    public SavedSession Saved { get; } = saved;
    public WebView2 View { get; private set; } = NewView();
    public QueueStatus? Status { get; private set; }
    public string? Error { get; private set; }
    public bool Closed { get; set; }
    public bool Opening { get; private set; }
    public bool CanGoBack => !Closed && Error is null && !Opening && View.CanGoBack;
    public Uri? ShareUrl => !Closed && Status is { } status && View.Source is { } url
        ? demo && url == DemoUrl && status is { Ahead: not null, QueueId: { } id }
            ? new Uri(DemoUrl, $"?q={id:D}")
            : status.ShareUrl(url)
        : null;

    public async Task OpenAsync(CoreWebView2Environment environment, Panel pane)
    {
        Opening = true;
        Error = null;
        var view = View;
        try
        {
            pane.Controls.Add(view);
            await view.EnsureCoreWebView2Async(environment, ProfileOptions(environment, Saved.Profile));
            profile = view.CoreWebView2.Profile;
            if (Closed) return;
            var browser = view.CoreWebView2;
            browser.Settings.IsWebMessageEnabled = false;
            if (demo)
            {
                browser.SetVirtualHostNameToFolderMapping(DemoUrl.Host, Path.Combine(AppContext.BaseDirectory, "Demo"),
                    CoreWebView2HostResourceAccessKind.Deny);
            }
            browser.NavigationStarting += (_, args) =>
            {
                if (!Uri.TryCreate(args.Uri, UriKind.Absolute, out var url) ||
                    (demo && url != DemoUrl) ||
                    (url.Scheme is not ("http" or "https") && !(startUrl.IsFile && url == startUrl)))
                {
                    args.Cancel = true;
                    return;
                }
                navigating = true;
                navigationId = args.NavigationId;
                documentVersion++;
                Status = null;
                lastObservation = null;
                Error = null;
                refresh();
            };
            browser.NavigationCompleted += (_, args) =>
            {
                if (args.NavigationId != navigationId) return;
                navigating = false;
                if (args.WebErrorStatus == CoreWebView2WebErrorStatus.OperationCanceled) return;
                if (!args.IsSuccess && args.HttpStatusCode < 400)
                {
                    log("url_load_failed", (int)args.WebErrorStatus, null);
                    Error = $"Page couldn’t load ({args.WebErrorStatus}).";
                }
                else if (view.Source is { } url && QueueStatus.IsWaitingRoom(url))
                    log("waiting_room_page_loaded", null, null);
                else if (Saved.WaitingRoomUrl is not null)
                    log("left_waiting_room", null, null);
                refresh();
            };
            browser.HistoryChanged += (_, _) => refresh();
            browser.NewWindowRequested += (_, args) =>
            {
                args.Handled = true;
                browser.Navigate(args.Uri);
            };
            browser.ProcessFailed += (_, args) =>
            {
                if (args.ProcessFailedKind is not (CoreWebView2ProcessFailedKind.BrowserProcessExited
                    or CoreWebView2ProcessFailedKind.RenderProcessExited)) return;
                log("web_process_terminated", (int)args.ProcessFailedKind, null);
                Error = "This session’s web process closed.";
                Status = null;
                documentVersion++;
                // WebView2 may still be unable to answer property calls inside ProcessFailed.
                pane.BeginInvoke(refresh);
            };
            browser.Navigate((Saved.WaitingRoomUrl ?? startUrl).AbsoluteUri);
        }
        catch (Exception ex) when (IsBrowserError(ex))
        {
            if (Closed) return;
            ShowInitializationError(ex);
        }
        finally
        {
            Opening = false;
            if (!Closed) refresh();
        }
    }

    public async Task RetryAsync(CoreWebView2Environment environment, Panel pane)
    {
        View.Dispose();
        View = NewView();
        profile = null;
        await OpenAsync(environment, pane);
    }

    public void ShowInitializationError(Exception ex)
    {
        log("webview_initialization_failed", ex.HResult, null);
        Status = null;
        Error = ex is WebView2RuntimeNotFoundException
            ? "Install the Microsoft Edge WebView2 Runtime, then try again."
            : $"Couldn’t open this browser session (0x{ex.HResult:X8}).";
    }

    public async Task ObserveAsync()
    {
        if (Closed || Opening || navigating || Error is not null) return;
        try
        {
            if (View.CoreWebView2 is not { } browser || View.Source is not { } url ||
                !(QueueStatus.IsWaitingRoom(url) || url.IsFile || (demo && url == DemoUrl))) return;
            var version = documentVersion;
            var json = await browser.ExecuteScriptAsync(ReadPage);
            if (Closed || version != documentVersion || View.Source != url || Error is not null) return;
            using var result = JsonDocument.Parse(json);
            if (result.RootElement.ValueKind != JsonValueKind.Object) return;
            var page = result.RootElement;
            var code = int.TryParse(page.GetProperty("errorID").GetString(), out var number) ? (int?)number : null;
            Status = QueueStatus.Parse(page.GetProperty("text").GetString() ?? "", code);
            if (Status.Observation(page.GetProperty("challengeVisible").GetBoolean()) is { } observation &&
                observation != lastObservation)
            {
                lastObservation = observation;
                log(observation.Event, observation.Code, observation.MembersAhead);
            }

            if (Status.ResumeUrl(url) is { } resume) Saved.WaitingRoomUrl = resume;
        }
        catch (Exception ex) when (IsBrowserError(ex) || ex is JsonException) { }
    }

    public async Task<bool> DeleteProfileAsync()
    {
        if (profile is null) return false;
        View.CoreWebView2?.Stop();
        await ClearAndDeleteAsync(profile);
        return true;
    }

    public static async Task ClearAndDeleteAsync(CoreWebView2Profile profile)
    {
        await profile.ClearBrowsingDataAsync();
        profile.Delete();
    }

    public static CoreWebView2ControllerOptions ProfileOptions(CoreWebView2Environment environment, Guid id)
    {
        var options = environment.CreateCoreWebView2ControllerOptions();
        options.ProfileName = id.ToString("N");
        return options;
    }

    public static bool IsBrowserError(Exception ex) =>
        ex is COMException or InvalidOperationException or WebView2RuntimeNotFoundException or ArgumentException
            or IOException or UnauthorizedAccessException;

    private static WebView2 NewView() => new() { Dock = DockStyle.Fill, Visible = false, AllowExternalDrop = false };
}
