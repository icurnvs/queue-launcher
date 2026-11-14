using System.Runtime.InteropServices;
using System.Text.Json;
using System.ComponentModel;

sealed partial class LauncherForm
{
    private static bool IsStorageError(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or JsonException;

    private List<QueueLink> LoadQueueLinks()
    {
        if (mode.QueueLinks is not null) return mode.QueueLinks;
        mode.QueueLinks = mode.Store.ReadLinks();
        foreach (var entry in mode.QueueLinks) mode.KnownLinks.Add((entry.Batch, entry.Session, entry.Url));
        return mode.QueueLinks;
    }

    private bool CaptureQueueLinks()
    {
        if (mode.Batch is not { } current) return true;
        var shareable = mode.Sessions.Select(session => (Session: session, Url: session.ShareUrl))
            .Where(item => item.Url is not null).ToArray();
        if (shareable.Length == 0) return true;
        try
        {
            var links = LoadQueueLinks();
            var added = shareable.Where(item => !mode.KnownLinks.Contains((current.LogId, item.Session.Saved.Number, item.Url!)))
                .Select(item => new QueueLink(current.LogId, item.Session.Saved.Number,
                    item.Url!, DateTimeOffset.Now, item.Session.Status?.Ahead)).ToArray();
            if (added.Length == 0) return true;

            var updated = new List<QueueLink>(links);
            updated.AddRange(added);
            mode.Store.SaveLinks(updated);
            mode.QueueLinks = updated;
            foreach (var entry in added) mode.KnownLinks.Add((entry.Batch, entry.Session, entry.Url));
            mode.HistoryError = null;
            return true;
        }
        catch (Exception ex) when (IsStorageError(ex))
        {
            mode.HistoryError = "Couldn’t save queue links. Check the app data folder.";
            return false;
        }
    }

    private void OpenLinkHistory()
    {
        CaptureQueueLinks();
        List<QueueLink> entries;
        try { entries = LoadQueueLinks(); }
        catch (Exception ex) when (IsStorageError(ex))
        {
            MessageBox.Show(this, "Couldn’t read saved queue links.", "Link history", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return;
        }

        using var dialog = new Form
        {
            Text = "Link history", StartPosition = FormStartPosition.CenterParent,
            ClientSize = new(900, 440), MinimumSize = new(600, 280), ShowInTaskbar = false,
            Font = Font, BackColor = Paper, ForeColor = Ink
        };
        var layout = Stack();
        layout.Padding = new(12);
        layout.RowStyles.Add(new(SizeType.AutoSize));
        layout.RowStyles.Add(new(SizeType.Percent, 100));
        layout.RowStyles.Add(new(SizeType.AutoSize));
        dialog.Controls.Add(layout);
        var explanation = Heading(entries.Count == 0 ? "No queue links saved yet."
            : "Saved queue links may expire at the waiting room.", LabelFont);
        explanation.Margin = new(0, 0, 0, 8);
        layout.Controls.Add(explanation, 0, 0);
        var grid = new DataGridView
        {
            Dock = DockStyle.Fill, ReadOnly = true, MultiSelect = false, RowHeadersVisible = false,
            AllowUserToAddRows = false, AllowUserToDeleteRows = false, BackgroundColor = Paper,
            SelectionMode = DataGridViewSelectionMode.FullRowSelect, AccessibleName = "Saved queue links"
        };
        grid.Columns.AddRange(
            HistoryColumn("Batch first seen", typeof(DateTimeOffset), "g"),
            HistoryColumn("Session", typeof(int)),
            HistoryColumn("Ahead when saved", typeof(int), "N0"),
            HistoryColumn("Link saved", typeof(DateTimeOffset), "g"),
            HistoryColumn("Queue link", typeof(string)));
        grid.Columns[4].AutoSizeMode = DataGridViewAutoSizeColumnMode.Fill;
        grid.Columns[4].MinimumWidth = 160;
        var sortedColumn = grid.Columns[3];
        var direction = ListSortDirection.Descending;
        sortedColumn.HeaderCell.SortGlyphDirection = SortOrder.Descending;
        grid.ColumnHeaderMouseClick += (_, args) =>
        {
            if (args.Button != MouseButtons.Left || grid.Rows.Count == 0) return;
            var column = grid.Columns[args.ColumnIndex];
            direction = column == sortedColumn && direction == ListSortDirection.Ascending
                ? ListSortDirection.Descending : ListSortDirection.Ascending;
            grid.Sort(column, direction);
            sortedColumn.HeaderCell.SortGlyphDirection = SortOrder.None;
            sortedColumn = column;
            sortedColumn.HeaderCell.SortGlyphDirection = direction == ListSortDirection.Ascending
                ? SortOrder.Ascending : SortOrder.Descending;
        };
        var batchDates = entries.GroupBy(entry => entry.Batch)
            .ToDictionary(group => group.Key, group => group.Min(entry => entry.Seen));
        foreach (var entry in entries.OrderByDescending(entry => entry.Seen))
        {
            var index = grid.Rows.Add(batchDates[entry.Batch].ToLocalTime(), entry.Session,
                entry.Ahead, entry.Seen.ToLocalTime(), entry.Url.AbsoluteUri);
            grid.Rows[index].Tag = entry;
        }
        layout.Controls.Add(grid, 0, 1);
        var copyHistory = Button("Copy link");
        var dismiss = Button("Close");
        ApplyButton(copyHistory, prominent: true);
        ApplyButton(dismiss);
        dismiss.DialogResult = DialogResult.Cancel;
        dialog.CancelButton = dismiss;
        var result = Heading("", DetailFont);
        copyHistory.Enabled = entries.Count > 0;
        copyHistory.Click += (_, _) =>
        {
            if (grid.CurrentRow?.Tag is not QueueLink entry) return;
            try
            {
                Clipboard.SetText(entry.Url.AbsoluteUri);
                result.Text = $"Copied Session {entry.Session} link.";
            }
            catch (ExternalException) { result.Text = "The clipboard is busy. Try again."; }
        };
        var actions = Across();
        actions.Controls.Add(result, 0, 0);
        actions.Controls.Add(Bar(copyHistory, dismiss), 1, 0);
        layout.Controls.Add(actions, 0, 2);
        dialog.ShowDialog(this);
    }

    private static DataGridViewTextBoxColumn HistoryColumn(string label, Type valueType, string format = "") => new()
    {
        HeaderText = label, ValueType = valueType, SortMode = DataGridViewColumnSortMode.Programmatic,
        AutoSizeMode = DataGridViewAutoSizeColumnMode.AllCells,
        DefaultCellStyle = new DataGridViewCellStyle { Format = format, NullValue = "" }
    };
}
