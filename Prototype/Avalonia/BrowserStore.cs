using System.Runtime.InteropServices;
using Avalonia.Controls;
using Avalonia.Platform;

#if WINDOWS_BUILD
using System.Reflection;
using Microsoft.Web.WebView2.Core;
#endif

namespace QueueLauncher.Prototype;

internal interface IBrowserStore
{
    void Configure(Guid id, WebViewEnvironmentRequestedEventArgs args);
    Task DeleteAsync(Guid id, NativeWebView view, Action detach);
}

internal sealed class WindowsStore : IBrowserStore
{
    private readonly string? fixedRuntime = GetRuntime();

    public void Configure(Guid id, WebViewEnvironmentRequestedEventArgs args)
    {
        var options = (WindowsWebView2EnvironmentRequestedEventArgs)args;
        options.ProfileName = id.ToString("N");
        options.UserDataFolder = Path.Combine(App.Root, "WebView2");
        options.BrowserExecutableFolder = fixedRuntime;
    }

    private static string? GetRuntime()
    {
        var folder = Environment.GetEnvironmentVariable("QUEUE_PROTOTYPE_WEBVIEW2")
            ?? Path.Combine(AppContext.BaseDirectory, "WebView2Runtime");
        if (File.Exists(Path.Combine(folder, "msedgewebview2.exe")))
            return Path.GetFullPath(folder);
        if (Directory.Exists(folder) || Environment.GetEnvironmentVariable("QUEUE_PROTOTYPE_WEBVIEW2") is not null)
            throw new InvalidOperationException("Fixed WebView2 folder does not contain msedgewebview2.exe.");
        return null;
    }

#if WINDOWS_BUILD
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    internal static CoreWebView2 Core(NativeWebView view)
    {
        var handle = (IWindowsWebView2PlatformHandle)view.TryGetPlatformHandle()!;
        // Avalonia exposes a borrowed ICoreWebView2 pointer. The SDK wrapper's
        // nonpublic constructor is a prototype dependency, pinned in the project.
        var native = Marshal.GetObjectForIUnknown(handle.CoreWebView2);
        return (CoreWebView2)Activator.CreateInstance(typeof(CoreWebView2),
            BindingFlags.Instance | BindingFlags.NonPublic, null, [native], null)!;
    }
#endif

    internal static void WaitForDeletion()
    {
        var marker = Path.Combine(App.Root, "deleted-profile-path.txt");
        if (!File.Exists(marker))
            return;
        var entries = File.ReadAllLines(marker);
        var pid = int.Parse(entries[0]);
        try
        {
            using var browser = System.Diagnostics.Process.GetProcessById(pid);
            if (browser.ProcessName == "msedgewebview2" && !browser.WaitForExit(30000))
                throw new InvalidOperationException("Previous WebView2 browser process is still running.");
        }
        catch (ArgumentException)
        {
            // The previous browser process already exited.
        }
        if (!SpinWait.SpinUntil(() => !Directory.Exists(entries[1]), TimeSpan.FromSeconds(30)))
            throw new InvalidOperationException("WebView2 still holds the deleted profile directory.");
    }

#if WINDOWS_BUILD
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    internal void CheckRuntime(NativeWebView view, Guid id)
    {
        var core = Core(view);
        if (core.Profile.ProfileName != id.ToString("N"))
            throw new InvalidOperationException("Unexpected WebView2 profile.");
        core.Settings.IsWebMessageEnabled = false;
        if (fixedRuntime is null)
            return;
        using var browser = System.Diagnostics.Process.GetProcessById((int)core.BrowserProcessId);
        var expected = Path.Combine(fixedRuntime, "msedgewebview2.exe");
        if (!string.Equals(browser.MainModule?.FileName, expected, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("WebView2 did not launch the selected Fixed Version executable.");
    }
#endif

    public async Task DeleteAsync(Guid id, NativeWebView view, Action detach)
    {
#if WINDOWS_BUILD
        if (OperatingSystem.IsWindows())
        {
            var core = Core(view);
            var processId = core.BrowserProcessId;
            var profile = core.Profile;
            if (profile.ProfileName != id.ToString("N"))
                throw new InvalidOperationException("Unexpected WebView2 profile.");
            await profile.ClearBrowsingDataAsync();
            var path = profile.ProfilePath;
            profile.Delete();
            detach();
            if (App.Mode == "restore")
                File.WriteAllLines(Path.Combine(App.Root, "deleted-profile-path.txt"), [processId.ToString(), path]);
            return;
        }
#endif
        await Task.FromException(new PlatformNotSupportedException());
    }
}

internal sealed class MacStore : IBrowserStore
{
    public void Configure(Guid id, WebViewEnvironmentRequestedEventArgs args)
    {
        ((AppleWKWebViewEnvironmentRequestedEventArgs)args).DataStoreIdentifier = id;
    }

    public async Task DeleteAsync(Guid id, NativeWebView view, Action detach)
    {
        detach();
        // The native host can release its view on a subsequent dispatcher pass.
        for (int attempt = 0; attempt < 20; attempt++)
        {
            await Task.Delay(100);
            var completion = new TaskCompletionSource<int>(TaskCreationOptions.RunContinuationsAsynchronously);
            CleanupCallback callback = error => completion.TrySetResult(error);
            DeleteMacStore(id.ToString("D"), callback);
            int error = await completion.Task;
            GC.KeepAlive(callback);
            if (error == 0)
                return;
        }
        throw new InvalidOperationException("WebKit could not release and remove the store.");
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void CleanupCallback(int error);

    [DllImport("QueuePrototypeCleanup", EntryPoint = "queue_delete_store")]
    private static extern void DeleteMacStore([MarshalAs(UnmanagedType.LPUTF8Str)] string id, CleanupCallback callback);
}
