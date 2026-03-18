param(
    [string]$InstallDir = $(if ($env:SWARM_INSTALL_DIR) { $env:SWARM_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'AiOfficeSwarm\bin' }),
    [switch]$KeepPath
)

$ErrorActionPreference = 'Stop'
$binaryPath = Join-Path $InstallDir 'swarm.exe'

function Write-Info {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Cyan
}

function Remove-FromUserPath {
    param([string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $currentUserPath) {
        return
    }

    $entries = $currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries) |
        Where-Object { $_ -ne $PathEntry }

    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
    Write-Info "Removed $PathEntry from the user PATH."
}

if (Test-Path $binaryPath) {
    Remove-Item -Path $binaryPath -Force
    Write-Info "Removed $binaryPath"
}
else {
    Write-Info "Nothing to remove at $binaryPath"
}

if ((Test-Path $InstallDir) -and -not (Get-ChildItem -Path $InstallDir -Force | Select-Object -First 1)) {
    Remove-Item -Path $InstallDir -Force
    Write-Info "Removed empty directory $InstallDir"
}

if (-not $KeepPath) {
    Remove-FromUserPath -PathEntry $InstallDir
}

Write-Info 'Uninstall complete.'
