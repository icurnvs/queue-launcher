param(
    [switch]$SkipBuild,
    [switch]$NamedProfiles,
    [string]$FixedRuntimePath
)

$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class ProcessTree {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct Entry {
        public uint Size, Usage, Id;
        public IntPtr HeapId;
        public uint ModuleId, Threads, ParentId;
        public int Priority;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string Name;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern SafeFileHandle CreateToolhelp32Snapshot(uint flags, uint id);
    [DllImport("kernel32.dll", EntryPoint = "Process32FirstW")]
    private static extern bool First(SafeFileHandle snapshot, ref Entry entry);
    [DllImport("kernel32.dll", EntryPoint = "Process32NextW")]
    private static extern bool Next(SafeFileHandle snapshot, ref Entry entry);

    public static List<Entry> WebView2() {
        var entries = new List<Entry>();
        using (var snapshot = CreateToolhelp32Snapshot(2, 0)) {
            var entry = new Entry { Size = (uint)Marshal.SizeOf(typeof(Entry)) };
            if (!First(snapshot, ref entry)) throw new InvalidOperationException("Process snapshot failed");
            do {
                if (String.Equals(entry.Name, "msedgewebview2.exe", StringComparison.OrdinalIgnoreCase)) {
                    entries.Add(entry);
                }
            } while (Next(snapshot, ref entry));
        }
        return entries;
    }
}
"@

$fixedBrowser = if ($FixedRuntimePath) {
    $runtime = (Resolve-Path -LiteralPath $FixedRuntimePath).ProviderPath
    Get-Item -LiteralPath (Join-Path $runtime 'msedgewebview2.exe')
}

Push-Location $PSScriptRoot
$previousData = $env:QUEUE_TAURI_PROTOTYPE_DATA
$previousRuntime = $env:WEBVIEW2_BROWSER_EXECUTABLE_FOLDER
try {
    if ($fixedBrowser) {
        $env:WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = $fixedBrowser.DirectoryName
        "Fixed WebView2 $($fixedBrowser.VersionInfo.ProductVersion) at $($fixedBrowser.FullName)"
    }
    if (!$SkipBuild) {
        cargo build --locked
        if ($LASTEXITCODE) { throw 'Prototype build failed' }
    }
    $exe = (Resolve-Path './target/debug/queue-launcher-tauri-prototype.exe').Path
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('queue-tauri-' + [guid]::NewGuid())
    $env:QUEUE_TAURI_PROTOTYPE_DATA = Join-Path $scratch 'baseline'
    $flags = @()
    $modes = @('initial', 'reopen')
    if ($NamedProfiles) {
        $flags += '--named-profiles'
        $modes += 'named-deletion'
    }
    foreach ($mode in $modes) {
        & $exe @flags "--verify-$mode"
        if ($LASTEXITCODE) { throw "Verification failed at $mode" }
    }

    foreach ($count in 12, 50) {
        $env:QUEUE_TAURI_PROTOTYPE_DATA = Join-Path $scratch "scale-$count"
        $null = New-Item -ItemType Directory -Path $env:QUEUE_TAURI_PROTOTYPE_DATA -Force
        $output = Join-Path $scratch "scale-$count.out"
        $errors = Join-Path $scratch "scale-$count.err"
        $launcher = Start-Process $exe ($flags + "--verify-$count") -PassThru -WindowStyle Normal -RedirectStandardOutput $output -RedirectStandardError $errors
        $null = $launcher.Handle # Keep the handle so ExitCode survives process exit.
        $measured = $false
        try {
            $deadline = [DateTime]::UtcNow.AddMinutes(5)
            while (!$launcher.HasExited -and [DateTime]::UtcNow -lt $deadline) {
                if ((Get-Content -Raw -LiteralPath $output) -match "SNAPSHOT $count") {
                    $processes = @([ProcessTree]::WebView2())
                    $browser = @($processes | Where-Object ParentId -eq $launcher.Id)
                    $tracked = [Collections.Generic.HashSet[uint32]]::new()
                    $null = $tracked.Add([uint32]$launcher.Id)
                    do {
                        $previousCount = $tracked.Count
                        foreach ($process in $processes) {
                            if ($tracked.Contains($process.ParentId)) {
                                $null = $tracked.Add($process.Id)
                            }
                        }
                    } while ($tracked.Count -gt $previousCount)
                    $children = @($processes | Where-Object { $tracked.Contains($_.Id) })
                    if ($browser.Count -eq 0) { throw 'Could not sample the WebView2 process tree' }
                    if ($fixedBrowser) {
                        $wrongRuntime = Get-Process -Id $browser.Id | Where-Object Path -ne $fixedBrowser.FullName
                        if ($wrongRuntime) { throw 'A WebView2 browser process did not use the requested Fixed Version executable' }
                        "Verified Fixed Version executable for all $($browser.Count) WebView2 browser processes at $count views"
                    }
                    $memory = (Get-Process -Id $children.Id -ErrorAction SilentlyContinue | Measure-Object WorkingSet64 -Sum).Sum
                    $profilePath = if ($NamedProfiles) { 'named-profiles/webview2' } else { 'profiles' }
                    $files = Get-ChildItem -LiteralPath (Join-Path $env:QUEUE_TAURI_PROTOTYPE_DATA $profilePath) -File -Recurse
                    $profileBytes = ($files | Measure-Object Length -Sum).Sum
                    "WebView2 for ${count} views - $($browser.Count) browser processes, $($children.Count) total, $([Math]::Round($memory/1MB)) MiB summed working set, $([Math]::Round($profileBytes/1MB)) MiB profile files"
                    $measured = $true
                    break
                }
                Start-Sleep -Milliseconds 200
            }
            if (!$launcher.WaitForExit(20000)) { throw "Verification timed out at $count views" }
            Get-Content -LiteralPath $output, $errors
            if ($launcher.ExitCode -ne 0 -or !$measured) { throw "Verification failed at $count views (exit code $($launcher.ExitCode), measured $measured)" }
        } finally {
            if (!$launcher.HasExited) { Stop-Process -Id $launcher.Id }
        }
    }
    "Scratch data: $scratch"
} finally {
    $env:QUEUE_TAURI_PROTOTYPE_DATA = $previousData
    $env:WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = $previousRuntime
    Pop-Location
}
