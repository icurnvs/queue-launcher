[CmdletBinding()]
param(
    [string] $Tag,
    [switch] $Publish
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
if ($Publish -and !$Tag) { throw 'Publishing requires an existing release tag.' }
if ($Tag) {
    if ($Tag -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$') { throw 'Unsupported release tag.' }
    $tagCommit = git rev-parse "refs/tags/$Tag^{commit}"
    if ($LASTEXITCODE -ne 0) { throw 'The release tag does not exist.' }
    $headCommit = git rev-parse HEAD
    if ($tagCommit -ne $headCommit) { throw 'The checkout must match the release tag.' }
}
$config = Get-Content -Raw Tauri/tauri.conf.json | ConvertFrom-Json
if ($Tag -like 'v*' -and $Tag -ne "v$($config.version)") {
    throw "Tag $Tag does not match application version $($config.version)."
}
$label = if ($Tag) { $Tag } else { 'build-' + (git rev-parse --short HEAD) }
$runtimeConfig = Get-Content -Raw .github/windows-runtime.json | ConvertFrom-Json
if (!$env:RUNNER_TOOL_CACHE) { throw 'RUNNER_TOOL_CACHE must point to the provisioned runner tool cache.' }
$runtime = Join-Path $env:RUNNER_TOOL_CACHE "WebView2/$($runtimeConfig.filename)"
if (!(Test-Path -LiteralPath $runtime)) { throw "Provision the signed WebView2 CAB using setup-windows-runner.ps1: $runtime" }
if ((Get-FileHash -LiteralPath $runtime -Algorithm SHA256).Hash -ne $runtimeConfig.sha256) {
    throw 'The cached WebView2 CAB does not match .github/windows-runtime.json.'
}

$target = 'x86_64-pc-windows-msvc'
cargo test --locked --release --manifest-path Tauri/Cargo.toml --bin queue-launcher --target $target
if ($LASTEXITCODE -ne 0) { throw 'Windows unit tests failed.' }
Push-Location Tauri
try {
    npx.cmd --yes --package @tauri-apps/cli@2.12.4 tauri build --bundles nsis --target $target --ci -- --locked
    if ($LASTEXITCODE -ne 0) { throw 'Windows installer build failed.' }
} finally { Pop-Location }

$output = Join-Path $root 'build/release'
if (Test-Path -LiteralPath $output) { throw 'Release output already exists; use a clean checkout.' }
New-Item -ItemType Directory -Path $output | Out-Null
$prefix = "QueueLauncher-$label-windows-x64"
$installers = @(Get-ChildItem "Tauri/target/$target/release/bundle/nsis/*-setup.exe" -File)
if ($installers.Count -ne 1) { throw 'Expected exactly one Windows installer.' }
Copy-Item -LiteralPath $installers[0].FullName -Destination (Join-Path $output "$prefix-setup.exe")
& "$PSScriptRoot/package-tauri-windows.ps1" -OutputDirectory (Join-Path $root 'build/portable-evergreen')
Copy-Item -LiteralPath 'build/portable-evergreen.zip' -Destination (Join-Path $output "$prefix-portable-evergreen.zip")
& "$PSScriptRoot/package-tauri-windows.ps1" -RuntimeCab $runtime -OutputDirectory (Join-Path $root 'build/portable-self-contained')
Copy-Item -LiteralPath 'build/portable-self-contained.zip' -Destination (Join-Path $output "$prefix-portable.zip")
foreach ($file in Get-ChildItem -LiteralPath $output -File) {
    $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $($file.Name)" | Set-Content -LiteralPath "$($file.FullName).sha256" -Encoding ascii
}
Get-ChildItem -LiteralPath $output -File | Select-Object Name, Length | Format-Table
if ($env:GITHUB_OUTPUT) { "tag=$Tag" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8 }
