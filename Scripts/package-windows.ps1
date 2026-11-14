[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $RuntimeCab,
    [ValidateSet('x64', 'arm64')]
    [string] $Architecture = 'x64',
    [string] $OutputDirectory = (Join-Path $PSScriptRoot '../build/windows-x64-portable')
)

$ErrorActionPreference = 'Stop'
$cab = Get-Item -LiteralPath $RuntimeCab
$pattern = "^Microsoft\.WebView2\.FixedVersionRuntime\.\d+(\.\d+){3}\.$Architecture\.cab$"
if ($cab.Name -notmatch $pattern) {
    throw "Supply the Microsoft WebView2 Fixed Version CAB for $Architecture."
}
$signature = Get-AuthenticodeSignature -LiteralPath $cab.FullName
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|, )O=Microsoft Corporation(,|$)') {
    throw 'The WebView2 CAB must have a valid Microsoft signature.'
}

$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if ((Test-Path -LiteralPath $output) -or (Test-Path -LiteralPath "$output.zip")) {
    throw "Choose a fresh output directory. The folder or its ZIP already exists: $output"
}
$project = Join-Path $PSScriptRoot '../Windows/QueueLauncher.csproj'
dotnet publish $project -c Release -r "win-$Architecture" --self-contained true -o $output
if ($LASTEXITCODE -ne 0) { throw 'Publishing Queue Launcher failed.' }

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

tar.exe -a -cf "$output.zip" -C (Split-Path -Parent $output) (Split-Path -Leaf $output)
if ($LASTEXITCODE -ne 0) { throw 'Creating the Windows ZIP failed.' }
Write-Output "$output.zip"
