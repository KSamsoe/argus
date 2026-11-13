# Install Argus for this user (distribution item 6), from a release on
# GitHub, or from an archive already here:
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Version 0.0.1] [-Archive <zip>] [-Repo owner/name]
#
#   irm https://argusengine.org/install.ps1 | iex
#
# A public release needs nothing more. A GitHub token is used when there is
# one, for a private repository or a fork: $env:GITHUB_TOKEN, else
# `gh auth token` (the GitHub CLI, signed in). The archive's checksum is
# checked against the release's SHA256SUMS before anything is put in place.
#
# Argus goes in %LOCALAPPDATA%\Argus\app (bin\, runtime\, templates\), and
# app\bin goes on this user's PATH. The version before stays as app.old
# until the new one has started (`argus version`), then is removed; if the
# new one does not start, the old one is put back. Nothing needs
# administrator rights. `argus uninstall` (or -Uninstall here) takes it all out
# again, except the projects and Argus's own settings (%APPDATA%\Argus).

param(
    [string]$Version = "latest",
    [string]$Archive = "",
    [string]$Repo = "KSamsoe/argus",
    [string]$Into = (Join-Path $env:LOCALAPPDATA "Argus"),
    [switch]$Uninstall,
    [switch]$NoPath,
    # Wait for this process to end first (argus self-update passes its own).
    [int]$WaitFor = 0
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$App = Join-Path $Into "app"
$Bin = Join-Path $App "bin"

function Say($m) { Write-Host "argus install: $m" }

function Set-UserPath([string]$add, [string]$remove) {
    $key = "HKCU:\Environment"
    $path = (Get-ItemProperty -Path $key -Name Path -ErrorAction SilentlyContinue).Path
    $now = @()
    if ($path) { $now = $path -split ";" }
    # Write only to add what is missing or take out what is there.
    if ($add -and ($now -contains $add)) { return }
    if ($remove -and -not ($now -contains $remove)) { return }
    $parts = @($now | Where-Object { $_ -and ($_ -ne $remove) -and ($_ -ne $add) })
    if ($add) { $parts = @($add) + $parts }
    $new = ($parts -join ";")
    if ($new -ne $path) {
        Set-ItemProperty -Path $key -Name Path -Value $new -Type ExpandString
        # Tell running programs (Explorer, new terminals) the PATH changed.
        $sig = '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'
        $t = Add-Type -MemberDefinition $sig -Name Env -Namespace ArgusInstall -PassThru
        $r = [UIntPtr]::Zero
        [void]$t::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$r)
    }
}

if ($WaitFor -gt 0) {
    try { Wait-Process -Id $WaitFor -Timeout 60 -ErrorAction Stop } catch { }
}

if ($Uninstall) {
    if (Test-Path $Bin) {
        # Daemons run from the install; stop them so its files can go.
        Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($App, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $Into "app"), (Join-Path $Into "app.old"), (Join-Path $Into "app.new"), (Join-Path $Into "downloads")
    if (-not $NoPath) { Set-UserPath "" $Bin }
    Say "removed $App (your projects and $env:APPDATA\Argus are kept)"
    exit 0
}

New-Item -ItemType Directory -Force -Path $Into | Out-Null

if (-not $Archive) {
    $token = $env:GITHUB_TOKEN
    if (-not $token) {
        try { $token = (& gh auth token 2>$null).Trim() } catch { }
    }
    $h = @{ "X-GitHub-Api-Version" = "2022-11-28"; "User-Agent" = "argus-install" }
    if ($token) { $h.Authorization = "Bearer $token" }
    $api = "https://api.github.com/repos/$Repo/releases/" + $(if ($Version -eq "latest") { "latest" } else { "tags/v$Version" })
    try { $rel = Invoke-RestMethod -Headers $h -Uri $api }
    catch { throw "no release at $api ($($_.Exception.Message)); a private repository needs GITHUB_TOKEN, or ``gh auth login``" }
    $asset = $rel.assets | Where-Object { $_.name -like "argus-*-windows-x86_64.zip" } | Select-Object -First 1
    $sums = $rel.assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
    if (-not $asset) { throw "release $($rel.tag_name) has no Windows archive" }
    if (-not $sums) { throw "release $($rel.tag_name) has no SHA256SUMS" }
    $dl = Join-Path $Into "downloads"
    New-Item -ItemType Directory -Force -Path $dl | Out-Null
    $Archive = Join-Path $dl $asset.name
    $get = $h.Clone(); $get.Accept = "application/octet-stream"
    Say "downloading $($asset.name) ($([math]::Round($asset.size / 1MB)) MB) from $($rel.tag_name)"
    Invoke-WebRequest -Headers $get -Uri $asset.url -OutFile $Archive
    $sumsFile = Join-Path $dl "SHA256SUMS"
    Invoke-WebRequest -Headers $get -Uri $sums.url -OutFile $sumsFile
    $want = (Get-Content $sumsFile | Where-Object { $_ -match [regex]::Escape($asset.name) + "$" } | Select-Object -First 1) -split "\s+" | Select-Object -First 1
    $have = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLower()
    if (-not $want -or $have -ne $want.ToLower()) {
        Remove-Item -Force $Archive
        throw "$($asset.name) does not match its checksum (expected $want, got $have); nothing was installed"
    }
}

# Unpack beside the install, then swap: app -> app.old, app.new -> app.
$New = Join-Path $Into "app.new"
$Old = Join-Path $Into "app.old"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $New, $Old
$unpack = Join-Path $Into "unpack"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $unpack
Expand-Archive -Path $Archive -DestinationPath $unpack
# The archive holds one folder, argus-<version>-windows.
$top = Get-ChildItem -Directory $unpack | Select-Object -First 1
if (-not $top -or -not (Test-Path (Join-Path $top.FullName "bin\argus.exe"))) {
    throw "$Archive is not an Argus archive (no argus-*\bin\argus.exe in it)"
}
Move-Item $top.FullName $New
Remove-Item -Recurse -Force $unpack
$ver = & (Join-Path $New "bin\argus.exe") version 2>&1
if ($LASTEXITCODE -ne 0) { throw "the new argus does not start: $ver" }

if (Test-Path $App) {
    # Daemons from the old install hold its files.
    Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($App + "\", [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
    Move-Item $App $Old
}
try {
    Move-Item $New $App
    $ok = & (Join-Path $Bin "argus.exe") version 2>&1
    if ($LASTEXITCODE -ne 0) { throw "argus does not start from $App : $ok" }
} catch {
    if (Test-Path $Old) {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $App
        Move-Item $Old $App
        Say "the new version did not start; the old one is back"
    }
    throw
}
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Old
if (-not $NoPath) { Set-UserPath $Bin "" }
Say "installed $ok in $App"
if (-not $NoPath -and -not (($env:Path -split ";") -contains $Bin)) {
    Say "open a new terminal for argus to be on the PATH, then: argus new my-game"
}
