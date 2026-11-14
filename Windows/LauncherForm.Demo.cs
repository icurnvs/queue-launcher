using Microsoft.Web.WebView2.Core;

sealed partial class LauncherForm
{
    private sealed class LauncherMode(string directory, bool isDemo = false)
    {
        public readonly BatchStore Store = new(directory);
        public readonly bool IsDemo = isDemo;
        public readonly List<BrowserSession> Sessions = [];
        public readonly HashSet<(Guid Batch, int Session, Uri Url)> KnownLinks = [];
        public HashSet<Guid> PendingCleanup = [];
        public CoreWebView2Environment? Environment;
        public SavedBatch? Batch;
        public BrowserSession? Selected;
        public List<QueueLink>? QueueLinks;
        public bool Loaded;
        public bool NeedsSave;
        public bool SortByPosition;
        public decimal Count = isDemo ? 12 : 5;
        public string UrlText = isDemo ? BrowserSession.DemoUrl.AbsoluteUri : "";
        public string ShareFeedback = "";
        public string? StateError;
        public string? CleanupError;
        public string? LogError;
        public string? HistoryError;
    }

    private readonly LauncherMode live;
    private readonly LauncherMode offline;
    private LauncherMode mode;
    private bool switchingModes;
    private Task? activePoll;

    private async Task SwitchModeAsync()
    {
        if (activePoll is not null) await activePoll;
        if (mode.NeedsSave && !TrySave(mode.Batch)) return;
        if (!CaptureQueueLinks()) return;
        mode.UrlText = releaseUrl.Text;
        mode.Count = count.Value;
        mode.SortByPosition = sortByPosition.Checked;
        mode.ShareFeedback = shareFeedback.Text;
        foreach (var session in mode.Sessions) session.View.Visible = false;

        switchingModes = true;
        try
        {
            mode = mode.IsDemo ? live : offline;
            releaseUrl.Text = mode.UrlText;
            count.Value = mode.Count;
            sortByPosition.Checked = mode.SortByPosition;
            shareFeedback.Text = mode.ShareFeedback;
            RebuildRows();
            if (!mode.Loaded)
            {
                await RestoreAsync();
                if (mode.IsDemo && mode.Batch is null) await StartAsync();
            }
        }
        finally { switchingModes = false; }
    }
}
