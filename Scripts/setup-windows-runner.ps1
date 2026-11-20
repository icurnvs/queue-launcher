[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $RuntimeCab,
    [string] $InstallDirectory = (Join-Path $env:LOCALAPPDATA 'QueueLauncherRunner')
)

$ErrorActionPreference = 'Stop'
$InstallDirectory = [IO.Path]::GetFullPath($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InstallDirectory))
$repo = 'icurnvs/queue-launcher'
$runtimeConfig = Get-Content -Raw (Join-Path $PSScriptRoot '../.github/windows-runtime.json') | ConvertFrom-Json
$cab = Get-Item -LiteralPath $RuntimeCab
if ($cab.Name -ne $runtimeConfig.filename -or (Get-FileHash -LiteralPath $cab.FullName -Algorithm SHA256).Hash -ne $runtimeConfig.sha256) {
    throw 'The CAB must match .github/windows-runtime.json.'
}
$signature = Get-AuthenticodeSignature -LiteralPath $cab.FullName
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|, )O=Microsoft Corporation(,|$)') {
    throw 'The CAB must have a valid Microsoft signature.'
}

if (!(Test-Path -LiteralPath (Join-Path $InstallDirectory 'config.cmd'))) {
    $release = gh api repos/actions/runner/releases/latest | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot look up the GitHub runner release.' }
    $asset = @($release.assets | Where-Object name -Match '^actions-runner-win-x64-[\d.]+\.zip$')
    if ($asset.Count -ne 1 -or $asset[0].digest -notmatch '^sha256:[a-f0-9]{64}$') {
        throw 'Expected one Windows x64 runner archive with a published SHA-256 digest.'
    }
    New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
    $archive = Join-Path $InstallDirectory $asset[0].name
    Invoke-WebRequest -Uri $asset[0].browser_download_url -OutFile $archive
    if ("sha256:$((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant())" -ne $asset[0].digest) {
        throw 'Runner archive checksum verification failed.'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $InstallDirectory
}

Push-Location $InstallDirectory
try {
    if (!(Test-Path -LiteralPath '.runner')) {
        $registration = gh api --method POST "repos/$repo/actions/runners/registration-token" | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or !$registration.token) { throw 'Runner registration requires repository admin access.' }
        & .\config.cmd --unattended --url "https://github.com/$repo" --token $registration.token --name "$env:COMPUTERNAME-queue-launcher" --labels queue-launcher-release --work _work
        if ($LASTEXITCODE -ne 0) { throw 'Runner registration failed.' }
        $registration = $null
    } else {
        $existing = Get-Content -Raw .runner | ConvertFrom-Json
        if ($existing.gitHubUrl.TrimEnd('/') -ne "https://github.com/$repo") { throw 'Runner belongs to another repository.' }
    }
    $cache = Join-Path $InstallDirectory '_work/_tool/WebView2'
    New-Item -ItemType Directory -Path $cache -Force | Out-Null
    Copy-Item -LiteralPath $cab.FullName -Destination (Join-Path $cache $cab.Name) -Force
    @'
$ErrorActionPreference = 'Stop'
$mutex = New-Object System.Threading.Mutex($false, 'Local\QueueLauncherGitHubRunner')
if (!$mutex.WaitOne(0)) { $mutex.Dispose(); exit }
try {
    Set-Location $PSScriptRoot
    & "$PSScriptRoot\run.cmd" *> "$PSScriptRoot\runner-console.log"
} finally {
    $mutex.ReleaseMutex()
    $mutex.Dispose()
}
'@ | Set-Content -LiteralPath 'start-runner.ps1' -Encoding utf8
    $powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $startup = [Environment]::GetFolderPath('Startup')
    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $startup 'Queue Launcher GitHub Runner.lnk'))
    $shortcut.TargetPath = $powershell
    $shortcut.Arguments = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$(Join-Path $InstallDirectory 'start-runner.ps1')`""
    $shortcut.WorkingDirectory = $InstallDirectory
    $shortcut.WindowStyle = 7
    $shortcut.Save()
    Start-Process -FilePath $powershell -ArgumentList $shortcut.Arguments -WindowStyle Hidden
    Write-Output "Runner installed at $InstallDirectory and starts at Windows sign-in."
} finally { Pop-Location }
