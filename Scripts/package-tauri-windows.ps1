[CmdletBinding()]
param(
    [string] $RuntimeCab,
    [ValidateSet('x64', 'arm64')]
    [string] $Architecture = 'x64',
    [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
if (!$OutputDirectory) {
    $suffix = if ($RuntimeCab) { 'portable' } else { 'portable-evergreen' }
    $OutputDirectory = Join-Path $PSScriptRoot "../build/tauri-windows-$Architecture-$suffix"
}
if ($RuntimeCab) {
    $cab = Get-Item -LiteralPath $RuntimeCab
    $pattern = "^Microsoft\.WebView2\.FixedVersionRuntime\.\d+(\.\d+){3}\.$Architecture\.cab$"
    if ($cab.Name -notmatch $pattern) {
        throw "Supply the Microsoft WebView2 Fixed Version CAB for $Architecture."
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $cab.FullName
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|, )O=Microsoft Corporation(,|$)') {
        throw 'The WebView2 CAB must have a valid Microsoft signature.'
    }
}

$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if ((Test-Path -LiteralPath $output) -or (Test-Path -LiteralPath "$output.zip")) {
    throw "Choose a fresh output directory. The folder or its ZIP already exists: $output"
}

$target = switch ($Architecture) {
    'x64' { 'x86_64-pc-windows-msvc' }
    'arm64' { 'aarch64-pc-windows-msvc' }
}
$project = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../Tauri')).Path
$manifest = Join-Path $project 'Cargo.toml'
$targetDirectory = Join-Path $project 'target'
cargo build --locked --release --manifest-path $manifest --bin queue-launcher --target $target --target-dir $targetDirectory
if ($LASTEXITCODE -ne 0) { throw 'Building Queue Launcher failed.' }

$app = Join-Path $targetDirectory "$target/release/queue-launcher.exe"
New-Item -ItemType Directory -Path $output | Out-Null
Copy-Item -LiteralPath $app -Destination (Join-Path $output 'QueueLauncher.exe')
if ($RuntimeCab) {
    $runtime = Join-Path $output 'WebView2Runtime'
    New-Item -ItemType Directory -Path $runtime | Out-Null
    expand.exe -F:* $cab.FullName $runtime | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Extracting the WebView2 runtime failed.' }

    # Microsoft's CAB can put the runtime under a versioned directory.
    $browser = @(Get-ChildItem -LiteralPath $runtime -Filter 'msedgewebview2.exe' -File -Recurse)
    if ($browser.Count -ne 1) { throw 'Expected one WebView2 browser executable in the CAB.' }
    if ($browser[0].DirectoryName -ne $runtime) {
        Get-ChildItem -LiteralPath $browser[0].DirectoryName -Force | Move-Item -Destination $runtime
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Open on Windows 10.cmd') -Destination $output
} else {
    @'
Queue Launcher - Windows portable (Evergreen WebView2)

Extract this ZIP and run QueueLauncher.exe. No app installation is required.
The app uses Microsoft Edge WebView2 Evergreen Runtime, normally already
installed on Windows 10 and Windows 11. If it is missing, install it from:
https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download

If you cannot install WebView2, use the self-contained portable download
that includes a WebView2Runtime folder instead.

Application data is stored under %LOCALAPPDATA%\local.queue-launcher.tauri,
not beside this executable. Installed and portable copies share that data.
'@ | Set-Content -LiteralPath (Join-Path $output 'README.txt') -Encoding utf8
}

tar.exe -a -cf "$output.zip" -C (Split-Path -Parent $output) (Split-Path -Leaf $output)
if ($LASTEXITCODE -ne 0) { throw 'Creating the Windows ZIP failed.' }
Write-Output "$output.zip"
