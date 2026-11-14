using System.Drawing.Drawing2D;

sealed partial class LauncherForm
{
    private static readonly Color Canvas = Theme(0xEDF0F6, SystemColors.Control);
    private static readonly Color Paper = Theme(0xFFFFFF, SystemColors.Window);
    private static readonly Color Workspace = Theme(0xF7F9FC, SystemColors.Window);
    private static readonly Color Ink = Theme(0x182440, SystemColors.WindowText);
    private static readonly Color Muted = Theme(0x4D5B73, SystemColors.WindowText);
    private static readonly Color Rule = Theme(0xCBD3E2, SystemColors.WindowText);
    private static readonly Color Masthead = Theme(0x172540, SystemColors.Control);
    private static readonly Color MastheadText = Theme(0xF4F7FE, SystemColors.ControlText);
    private static readonly Color Accent = Theme(0x254AC8, SystemColors.Highlight);
    private static readonly Color OnAccent = Theme(0xFFFFFF, SystemColors.HighlightText);
    private static readonly Color Selection = Theme(0xE5EBFF, SystemColors.Window);
    private static readonly Color Success = Theme(0x176044, SystemColors.WindowText);
    private static readonly Color Danger = Theme(0x9B302F, SystemColors.WindowText);
    private static readonly Color BestSpot = Theme(0x714600, SystemColors.WindowText);
    private static readonly Color BestSpotBackground = Theme(0xFDF0CC, SystemColors.Window);
    private static readonly Font TitleFont = new("Segoe UI", 21, FontStyle.Bold);
    private static readonly Font SectionFont = new("Segoe UI", 14, FontStyle.Bold);
    private static readonly Font LabelFont = new("Segoe UI Semibold", 10);
    private static readonly Font DetailFont = new("Segoe UI", 9);
    private static readonly Font PositionFont = new("Segoe UI", 13, FontStyle.Bold);
    private readonly Label browserTitle = Heading("Browser", SectionFont);
    private readonly Label browserHint = Heading("", DetailFont);
    private readonly Label shareFeedback = new()
    {
        AutoSize = true, Dock = DockStyle.Fill, Font = LabelFont, ForeColor = Success,
        BackColor = Workspace, Padding = new(12, 8, 12, 8), Margin = Padding.Empty,
        UseMnemonic = false, Visible = false, LiveSetting = System.Windows.Forms.Automation.AutomationLiveSetting.Polite
    };
    private readonly Label emptySessions = Heading("Sessions will appear here.", DetailFont);
    private readonly Panel emptySymbol = new() { Size = new(60, 52), TabStop = false };
    private readonly TableLayoutPanel setup = Stack();
    private readonly FlowLayoutPanel sessionActions = Bar();
    private readonly FlowLayoutPanel browserActions = Bar();
    private readonly Button history = Button("Link history");
    private readonly Button demo = Button("Demo mode");
    private readonly CheckBox sortByPosition = new()
    {
        Text = "Sort by queue position", AutoSize = true, Anchor = AnchorStyles.Left,
        Margin = new(4, 0, 0, 8)
    };
    private bool centeringPlaceholder;

    private void AddSessionContextMenu()
    {
        var menu = new ContextMenuStrip();
        var copyQueueLink = new ToolStripMenuItem("Copy queue link");
        var copyAndClose = new ToolStripMenuItem("Copy and close");
        var closeSession = new ToolStripMenuItem("Close session");
        menu.Items.AddRange([copyQueueLink, copyAndClose, new ToolStripSeparator(), closeSession]);

        BrowserSession? target = null;
        void ShowMenu(BrowserSession session, Point location)
        {
            if (busy || session.Closed) return;
            target = session;
            copyQueueLink.Enabled = copyAndClose.Enabled = session.ShareUrl is not null;
            menu.Show(sessionList, location);
        }

        sessionList.CellMouseClick += (_, args) =>
        {
            if (args.Button == MouseButtons.Right && args.RowIndex >= 0 &&
                sessionList.Rows[args.RowIndex].Tag is BrowserSession session)
                ShowMenu(session, sessionList.PointToClient(Cursor.Position));
        };
        sessionList.KeyDown += (_, args) =>
        {
            if ((args.KeyCode == Keys.Apps || (args.Shift && args.KeyCode == Keys.F10)) &&
                sessionList.CurrentRow is { Tag: BrowserSession session } row)
            {
                args.SuppressKeyPress = true;
                var bounds = sessionList.GetRowDisplayRectangle(row.Index, cutOverflow: true);
                ShowMenu(session, new Point(bounds.Left, bounds.Bottom));
            }
        };
        copyQueueLink.Click += (_, _) =>
        {
            if (target is { } session) CopySession(session);
        };
        copyAndClose.Click += async (_, _) =>
        {
            if (target is { } session) await CopyAndCloseAsync(session);
        };
        closeSession.Click += async (_, _) =>
        {
            if (target is { Closed: false } session && mode.Sessions.Contains(session))
                await RunAsync(() => RemoveAsync([session]));
        };
        sessionList.Disposed += (_, _) => menu.Dispose();
    }

    private void BuildLayout()
    {
        SuspendLayout();
        Text = "Queue Launcher";
        ClientSize = new(1160, 760);
        MinimumSize = new(870, 550);
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Canvas;
        ForeColor = Ink;
        AcceptButton = open;

        ApplyButton(open, prominent: true);
        ApplyButton(mark, prominent: true);
        ApplyButton(retry, prominent: true);
        foreach (var button in new[] { next, back, reload, copy, copyClose, close })
            ApplyButton(button);
        ApplyButton(add, onMasthead: true);
        ApplyButton(logs, onMasthead: true);
        ApplyButton(history, onMasthead: true);
        ApplyButton(demo, onMasthead: true);
        ApplyButton(end, onMasthead: true);
        close.ForeColor = Danger;
        end.ForeColor = Theme(0xFFD4D1, SystemColors.ControlText);

        var root = Stack();
        root.RowStyles.Add(new(SizeType.AutoSize));
        root.RowStyles.Add(new(SizeType.AutoSize));
        root.RowStyles.Add(new(SizeType.Percent, 100));
        root.RowStyles.Add(new(SizeType.AutoSize));
        Controls.Add(root);

        var masthead = Across();
        masthead.BackColor = Masthead;
        masthead.Padding = new(22, 10, 22, 10);
        var title = Heading(Text, TitleFont);
        title.ForeColor = MastheadText;
        masthead.Controls.Add(title, 0, 0);
        masthead.Controls.Add(Bar(demo, add, history, logs, end), 1, 0);
        root.Controls.Add(masthead, 0, 0);

        setup.AutoSize = true;
        setup.ColumnCount = 3;
        setup.Padding = new(22, 11, 22, 14);
        setup.ColumnStyles.Add(new(SizeType.Percent, 100));
        setup.ColumnStyles.Add(new(SizeType.AutoSize));
        setup.ColumnStyles.Add(new(SizeType.AutoSize));
        releaseUrl.Dock = DockStyle.None;
        releaseUrl.Anchor = AnchorStyles.Left | AnchorStyles.Right;
        count.Anchor = AnchorStyles.Left;
        releaseUrl.Margin = new(0, 0, 16, 0);
        count.Margin = new(0, 0, 8, 0);
        count.Width = 86;
        setup.Controls.Add(Heading("Release URL", LabelFont), 0, 0);
        setup.Controls.Add(Heading("Sessions", LabelFont), 1, 0);
        setup.Controls.Add(releaseUrl, 0, 1);
        setup.Controls.Add(count, 1, 1);
        open.Anchor = AnchorStyles.Left;
        setup.Controls.Add(open, 2, 1);
        root.Controls.Add(setup, 0, 1);

        var body = new TableLayoutPanel
        {
            Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Padding = new(22, 0, 22, 16), Margin = Padding.Empty
        };
        body.ColumnStyles.Add(new(SizeType.Absolute, 306));
        body.ColumnStyles.Add(new(SizeType.Percent, 100));
        root.Controls.Add(body, 0, 2);

        var sidebar = Stack();
        sidebar.BackColor = Paper;
        sidebar.Padding = new(12);
        sidebar.Margin = new(0, 0, 12, 0);
        var summary = Across();
        summary.Padding = new(4, 0, 4, 8);
        summary.Controls.Add(Heading("Sessions", SectionFont), 0, 0);
        ready.Dock = DockStyle.None;
        ready.Anchor = AnchorStyles.Right;
        ready.Font = LabelFont;
        ready.ForeColor = Muted;
        ready.Padding = Padding.Empty;
        summary.Controls.Add(ready, 1, 0);
        sidebar.RowStyles.Add(new(SizeType.AutoSize));
        sidebar.RowStyles.Add(new(SizeType.AutoSize));
        sidebar.RowStyles.Add(new(SizeType.Percent, 100));
        sidebar.Controls.Add(summary, 0, 0);
        sidebar.Controls.Add(sortByPosition, 0, 1);

        var listPane = new Panel { Dock = DockStyle.Fill, Margin = Padding.Empty };
        sessionList.BackgroundColor = Paper;
        sessionList.CellBorderStyle = DataGridViewCellBorderStyle.None;
        sessionList.ScrollBars = ScrollBars.Vertical;
        sessionList.Columns.Add(new DataGridViewTextBoxColumn { AutoSizeMode = DataGridViewAutoSizeColumnMode.Fill });
        sessionList.CellPainting += DrawSessionRow;
        sessionList.GotFocus += (_, _) => sessionList.Invalidate();
        sessionList.LostFocus += (_, _) => sessionList.Invalidate();
        listPane.Controls.Add(sessionList);
        emptySessions.ForeColor = Muted;
        emptySessions.Anchor = AnchorStyles.Top | AnchorStyles.Left;
        emptySessions.Location = new(4, 10);
        listPane.Controls.Add(emptySessions);
        sidebar.Controls.Add(listPane, 0, 2);
        body.Controls.Add(sidebar, 0, 0);

        var browser = Stack();
        browser.BackColor = Paper;
        browser.Padding = new(1);
        browser.RowStyles.Add(new(SizeType.AutoSize));
        browser.RowStyles.Add(new(SizeType.AutoSize));
        browser.RowStyles.Add(new(SizeType.Percent, 100));
        browser.RowStyles.Add(new(SizeType.AutoSize));
        var browserBar = Across();
        browserBar.Padding = new(14, 6, 8, 6);
        browserBar.Controls.Add(browserTitle, 0, 0);
        browserActions.Controls.AddRange([back, reload, next, mark]);
        browserBar.Controls.Add(browserActions, 1, 0);
        browser.Controls.Add(browserBar, 0, 0);
        sessionActions.Anchor = AnchorStyles.Left;
        sessionActions.Padding = new(14, 0, 8, 8);
        sessionActions.Controls.AddRange([copy, copyClose, close]);
        browser.Controls.Add(sessionActions, 0, 1);
        browserPane.BackColor = Workspace;
        browserPane.Margin = Padding.Empty;
        browser.Controls.Add(browserPane, 0, 2);
        browser.Controls.Add(shareFeedback, 0, 3);
        body.Controls.Add(browser, 1, 0);

        placeholder.Margin = Padding.Empty;
        browserPane.Controls.Add(placeholder);
        browserMessage.Font = TitleFont;
        browserMessage.Anchor = AnchorStyles.None;
        browserMessage.Margin = new(0, 4, 0, 4);
        browserHint.Anchor = AnchorStyles.None;
        browserHint.ForeColor = Muted;
        browserHint.TextAlign = ContentAlignment.MiddleCenter;
        browserHint.Margin = new(0, 0, 0, 10);
        emptySymbol.Anchor = AnchorStyles.None;
        emptySymbol.Paint += DrawEmptySymbol;
        retry.Anchor = AnchorStyles.None;
        placeholder.Controls.AddRange([emptySymbol, browserMessage, browserHint, retry]);
        browserPane.Resize += (_, _) => CenterPlaceholder();
        placeholder.SizeChanged += (_, _) => CenterPlaceholder();

        notice.ForeColor = Danger;
        notice.BackColor = Paper;
        notice.Margin = new(22, 0, 22, 12);
        root.Controls.Add(notice, 0, 3);
        ResumeLayout();
    }

    private void RefreshLayout(BrowserSession? active)
    {
        Text = mode.IsDemo ? "Queue Launcher · Demo mode" : "Queue Launcher";
        demo.Text = mode.IsDemo ? "Exit demo mode" : "Demo mode";
        var hasSessions = mode.Sessions.Count > 0;
        var readyCount = mode.Sessions.Count(session => session.Saved.Ready);
        ready.Text = hasSessions ? $"{readyCount} of {mode.Sessions.Count} ready" : "";
        sessionList.Visible = hasSessions;
        emptySessions.Visible = !hasSessions;
        setup.Visible = !hasSessions;
        end.Visible = hasSessions;
        sessionActions.Visible = browserActions.Visible = hasSessions;
        add.Visible = hasSessions;
        browserTitle.Text = active is null ? "Browser" : $"Session {active.Saved.Number}";
        shareFeedback.Visible = shareFeedback.Text.Length > 0;
        emptySymbol.Visible = active is null && !busy;
        browserMessage.Text = active?.Error is not null ? "Page couldn’t load"
            : busy || active is not null ? "Opening sessions…" : "No sessions open";
        browserHint.Text = active?.Error ?? (busy || active is not null
            ? "Preparing the browser." : mode.IsDemo
                ? "Open sessions to start another demo batch." : "Enter a release URL above to start a batch.");
        CenterPlaceholder();
    }

    private int SessionRowHeight(DataGridViewRow row, IReadOnlyList<string> lines)
    {
        var width = Math.Max(1, row.Cells[0].Size.Width - ScalePixel(24));
        var height = ScalePixel(13) + TextRenderer.MeasureText(lines[0], LabelFont).Height;
        foreach (var line in lines.Skip(1))
            height += TextRenderer.MeasureText(RowText(line), RowFont(line),
                new(width, int.MaxValue), RowTextFlags).Height + ScalePixel(2);
        return height + ScalePixel(15);
    }

    private const TextFormatFlags RowTextFlags = TextFormatFlags.WordBreak | TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding;
    private static Font RowFont(string line) => line.Contains(" ahead", StringComparison.Ordinal) ? PositionFont : DetailFont;
    private const string BestSpotSuffix = " · Best spot";
    private static string RowText(string line) => line.EndsWith(BestSpotSuffix, StringComparison.Ordinal)
        ? line[..^BestSpotSuffix.Length] : line;

    private void DrawSessionRow(object? sender, DataGridViewCellPaintingEventArgs args)
    {
        if (args.RowIndex < 0 || args.Value is not string { Length: > 0 } value || args.Graphics is not { } graphics) return;
        var selectedRow = (args.State & DataGridViewElementStates.Selected) != 0;
        var keyboardFocus = selectedRow && sessionList.Focused;
        var bounds = args.CellBounds;
        var tile = new Rectangle(bounds.X, bounds.Y + ScalePixel(2), bounds.Width - 1, bounds.Height - ScalePixel(6));
        using var background = new SolidBrush(Paper);
        using var outline = new Pen(selectedRow ? Accent : Rule, keyboardFocus ? ScalePixel(2) : 1);
        using var fill = new SolidBrush(selectedRow ? Selection : Workspace);
        graphics.FillRectangle(background, bounds);
        graphics.FillRectangle(fill, tile);
        graphics.DrawRectangle(outline, tile);

        var lines = value.Split(Environment.NewLine);
        var stateSeparator = lines[0].LastIndexOf(" · ", StringComparison.Ordinal);
        var heading = stateSeparator < 0 ? lines[0] : lines[0][..stateSeparator];
        var state = stateSeparator < 0 ? "" : lines[0][(stateSeparator + 3)..];
        var x = bounds.X + ScalePixel(12);
        var y = bounds.Y + ScalePixel(10);
        var width = Math.Max(1, bounds.Width - ScalePixel(24));
        var headingSize = TextRenderer.MeasureText(graphics, heading, LabelFont, Size.Empty, TextFormatFlags.NoPadding);
        TextRenderer.DrawText(graphics, heading, LabelFont, new Point(x, y), Ink, TextFormatFlags.NoPadding);
        TextRenderer.DrawText(graphics, state, DetailFont, new Rectangle(x, y, width, headingSize.Height),
            state == "Ready" ? Success : Muted, TextFormatFlags.Right | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPadding);
        y += headingSize.Height + ScalePixel(3);

        foreach (var line in lines.Skip(1))
        {
            var font = RowFont(line);
            var mainText = RowText(line);
            var best = mainText != line;
            var size = TextRenderer.MeasureText(graphics, mainText, font, new(width, int.MaxValue), RowTextFlags);
            var color = font == PositionFont ? Ink : line == "Share link available" ? Accent :
                line is "No more attempts" or "Event ended" or "Session expired" ||
                line.StartsWith("Queue-it error ", StringComparison.Ordinal) ? Danger : Muted;
            TextRenderer.DrawText(graphics, mainText, font, new Rectangle(x, y, width, size.Height), color, RowTextFlags);
            if (best)
            {
                var badge = TextRenderer.MeasureText(graphics, "Best spot", DetailFont, Size.Empty, TextFormatFlags.NoPadding);
                var badgeBounds = new Rectangle(x + width - badge.Width - ScalePixel(12), y, badge.Width + ScalePixel(12), size.Height);
                using var tint = new SolidBrush(BestSpotBackground);
                graphics.FillRectangle(tint, badgeBounds);
                TextRenderer.DrawText(graphics, "Best spot", DetailFont, badgeBounds, BestSpot,
                    TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPadding);
            }
            y += size.Height + ScalePixel(2);
        }
        args.Handled = true;
    }

    private static void DrawEmptySymbol(object? sender, PaintEventArgs args)
    {
        if (sender is not Control control) return;
        var graphics = args.Graphics;
        graphics.SmoothingMode = SmoothingMode.AntiAlias;
        graphics.ScaleTransform(control.Width / 60f, control.Height / 52f);
        using var stroke = new Pen(Accent, 1.5f) { LineJoin = LineJoin.Round };
        using var fill = new SolidBrush(Selection);
        graphics.FillRectangle(fill, 5, 5, 42, 30);
        graphics.DrawRectangle(stroke, 5, 5, 42, 30);
        graphics.FillRectangle(fill, 12, 12, 42, 30);
        graphics.DrawRectangle(stroke, 12, 12, 42, 30);
        graphics.DrawLine(stroke, 12, 21, 54, 21);
        graphics.DrawLine(stroke, 20, 30, 35, 30);
    }

    private int ScalePixel(int pixels) => (int)Math.Round(pixels * DeviceDpi / 96d);

    private void CenterPlaceholder()
    {
        if (centeringPlaceholder || !placeholder.Visible) return;
        centeringPlaceholder = true;
        try
        {
            var width = Math.Max(1, browserPane.ClientSize.Width - ScalePixel(36));
            if (browserMessage.MaximumSize.Width != width)
                browserMessage.MaximumSize = browserHint.MaximumSize = new(width, 0);
            placeholder.Location = new(Math.Max(0, (browserPane.ClientSize.Width - placeholder.Width) / 2),
                Math.Max(0, (browserPane.ClientSize.Height - placeholder.Height) / 2));
        }
        finally { centeringPlaceholder = false; }
    }

    private static Color Theme(int rgb, Color highContrast) =>
        SystemInformation.HighContrast ? highContrast : Color.FromArgb(0xFF, rgb >> 16, (rgb >> 8) & 0xFF, rgb & 0xFF);

    private static Button Button(string text) => new() { Text = text, AutoSize = true };

    private static void ApplyButton(Button button, bool prominent = false, bool onMasthead = false)
    {
        button.FlatStyle = FlatStyle.Flat;
        button.Font = LabelFont;
        button.Padding = new(7, 2, 7, 2);
        button.MinimumSize = new(0, 34);
        button.Margin = new(0, 3, 5, 3);
        button.UseVisualStyleBackColor = false;
        button.ForeColor = prominent ? OnAccent : onMasthead ? MastheadText : Ink;
        button.BackColor = prominent ? Accent : onMasthead ? Masthead : Paper;
        button.FlatAppearance.BorderColor = prominent ? Accent : onMasthead
            ? Theme(0x8091B0, SystemColors.ControlText) : Rule;
        button.FlatAppearance.MouseOverBackColor = prominent ? Theme(0x1C38A0, SystemColors.Highlight)
            : onMasthead ? Theme(0x2D4269, SystemColors.Control) : Selection;
        if (prominent)
        {
            button.EnabledChanged += (_, _) =>
            {
                button.BackColor = button.Enabled ? Accent : Canvas;
                button.FlatAppearance.BorderColor = button.Enabled ? Accent : Rule;
            };
        }
    }

    private static Label Heading(string text, Font font) => new()
    {
        Text = text, Font = font, ForeColor = Ink, Anchor = AnchorStyles.Left, AutoSize = true, Margin = Padding.Empty
    };

    private static TableLayoutPanel Stack() => new() { Dock = DockStyle.Fill, ColumnCount = 1, Margin = Padding.Empty };

    private static TableLayoutPanel Across()
    {
        var layout = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, ColumnCount = 2, Margin = Padding.Empty };
        layout.ColumnStyles.Add(new(SizeType.Percent, 100));
        layout.ColumnStyles.Add(new(SizeType.AutoSize));
        return layout;
    }

    private static FlowLayoutPanel Bar(params Control[] controls)
    {
        var bar = new FlowLayoutPanel
        {
            Anchor = AnchorStyles.Right, AutoSize = true, WrapContents = false, Margin = Padding.Empty
        };
        bar.Controls.AddRange(controls);
        return bar;
    }
}
