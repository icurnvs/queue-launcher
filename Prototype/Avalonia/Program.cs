using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Themes.Fluent;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;

namespace QueueLauncher.Prototype;

internal sealed class App : Application
{
    internal static string Root = "";
    internal static string Mode = "";
    internal static Uri Fixture = null!;
    internal static int ExitCode;

    public override void Initialize() => Styles.Add(new FluentTheme());

    public override void OnFrameworkInitializationCompleted()
    {
        ((IClassicDesktopStyleApplicationLifetime)ApplicationLifetime!).MainWindow = new Launcher();
        base.OnFrameworkInitializationCompleted();
    }

    [STAThread]
    public static int Main(string[] args)
    {
        if (!OperatingSystem.IsWindows() && !OperatingSystem.IsMacOSVersionAtLeast(14))
        {
            Console.Error.WriteLine("Requires Windows or macOS 14+.");
            return 1;
        }

        Mode = args.FirstOrDefault() ?? "interactive";
        if (args.Length > 2 || Mode is not ("interactive" or "seed" or "restore" or "settle" or "reopen" or "12" or "50"))
        {
            Console.Error.WriteLine("Usage: [interactive|seed|restore|settle|reopen|12|50] [scratch directory]");
            return 1;
        }
        Root = Path.GetFullPath(args.ElementAtOrDefault(1) ?? Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Queue Launcher Avalonia Prototype"));
        Directory.CreateDirectory(Root);
        using var instance = new FileStream(Path.Combine(Root, "launcher.lock"), FileMode.OpenOrCreate,
            FileAccess.ReadWrite, FileShare.None);

        if (Mode is ("settle" or "reopen") && OperatingSystem.IsWindows())
            WindowsStore.WaitForDeletion();

        var originPath = Path.Combine(Root, "port.txt");
        var port = File.Exists(originPath) ? int.Parse(File.ReadAllText(originPath)) : 0;
        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.ClearProviders();
        builder.WebHost.UseUrls($"http://127.0.0.1:{port}");
        var server = builder.Build();
        var html = File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "fixture.html"));
        server.MapGet("/", () => Results.Content(html, "text/html; charset=utf-8"));
        server.StartAsync().GetAwaiter().GetResult();
        try
        {
            Fixture = new Uri(server.Urls.Single() + "/");
            File.WriteAllText(originPath, Fixture.Port.ToString());
            AppBuilder.Configure<App>().UsePlatformDetect().StartWithClassicDesktopLifetime([]);
            return ExitCode;
        }
        finally
        {
            // After Avalonia exits, its UI synchronization context can no longer run continuations.
            Task.Run(async () =>
            {
                await server.StopAsync();
                await server.DisposeAsync();
            }).GetAwaiter().GetResult();
        }
    }
}
