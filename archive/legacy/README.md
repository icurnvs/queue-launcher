# Legacy Swift and WinForms apps

These implementations are archived for reference. The maintained Rust/Tauri
app is documented in the [repository README](../../README.md).

The legacy source, tests, and packaging scripts retain their relative layout
inside this directory. Shared fixture and Windows launcher files are copied
into `Scripts` so the archived projects remain self-contained.

Run these commands from `archive/legacy`, rather than the repository root.

## Swift on macOS

With Xcode or its command-line tools installed:

```sh
sh Scripts/package-app.sh
```

This builds and locally signs `build/Queue Launcher.app` inside this archive.

## WinForms on Windows

With the .NET 8 SDK and WebView2 Runtime installed:

```powershell
dotnet run --project Windows/QueueLauncher.csproj
dotnet run --project Tests/Windows
```

To build the archived self-contained portable Windows ZIP:

```powershell
.\Scripts\package-windows.ps1 -RuntimeCab 'C:\path\to\Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab'
```

These commands are retained as historical build instructions. Current
release verification covers Rust/Tauri.
