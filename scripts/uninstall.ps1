# Remove files installed by scripts/install.ps1.
# Usage:
#   .\scripts\uninstall.ps1
#   .\scripts\uninstall.ps1 -Prefix "$env:LOCALAPPDATA\mako"
#   .\scripts\uninstall.ps1 -DryRun
param(
    [string]$Prefix = $(Join-Path $env:USERPROFILE ".local"),
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$BinDir = Join-Path $Prefix "bin"
$ShareDir = Join-Path $Prefix "share\mako"
$Bin = Join-Path $BinDir "mako.exe"

function Remove-MakoPath {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        Write-Host "skip missing $Path"
        return
    }
    if ($DryRun) {
        Write-Host "would remove $Path"
    } else {
        Remove-Item $Path -Recurse -Force
        Write-Host "removed $Path"
    }
}

Write-Host "mako uninstall"
Write-Host "  prefix: $Prefix"
Write-Host "  bin:    $Bin"
Write-Host "  share:  $ShareDir"

Remove-MakoPath $Bin
Remove-MakoPath $ShareDir

if ($DryRun) {
    Write-Host "Dry run only. Re-run without -DryRun to remove files."
} else {
    Write-Host "Done. Remove $BinDir from PATH if it was added only for Mako."
}
