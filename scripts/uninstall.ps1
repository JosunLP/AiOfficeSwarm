param(
    [string]$InstallDir = $env:SWARM_INSTALL_DIR,
    [switch]$KeepPath
)

$ErrorActionPreference = 'Stop'
$pathUtilsPath = Join-Path $PSScriptRoot 'path-utils.ps1'
if (Test-Path $pathUtilsPath) {
    . $pathUtilsPath
}

# Keep this fallback in sync with scripts/path-utils.ps1 for one-file downloads.
if (-not (Get-Command Normalize-PathEntry -CommandType Function -ErrorAction SilentlyContinue)) {
    function Normalize-PathEntry {
        param([string]$PathEntry)

        if ([string]::IsNullOrWhiteSpace($PathEntry)) {
            return ''
        }

        $trimmedPath = $PathEntry.Trim()
        if ($trimmedPath.Length -ge 2 -and $trimmedPath.StartsWith('"') -and $trimmedPath.EndsWith('"')) {
            $trimmedPath = $trimmedPath.Substring(1, $trimmedPath.Length - 2)
        }

        $expandedPath = [Environment]::ExpandEnvironmentVariables($trimmedPath)

        try {
            $normalizedPath = [System.IO.Path]::GetFullPath($expandedPath)
        }
        catch {
            $normalizedPath = $expandedPath
        }

        $pathRoot = [System.IO.Path]::GetPathRoot($normalizedPath)
        if ($pathRoot -and -not $normalizedPath.Equals($pathRoot, [System.StringComparison]::Ordinal)) {
            $normalizedPath = $normalizedPath.TrimEnd('\', '/')
        }

        return $normalizedPath
    }
}

function Write-Info {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Cyan
}

function Resolve-InstallDir {
    param([string]$RequestedInstallDir)

    if ($RequestedInstallDir) {
        return $RequestedInstallDir
    }

    if ($env:LOCALAPPDATA) {
        return (Join-Path $env:LOCALAPPDATA 'AiOfficeSwarm\bin')
    }

    if ($env:HOME) {
        return (Join-Path $env:HOME 'AppData\Local\AiOfficeSwarm\bin')
    }

    throw 'Set SWARM_INSTALL_DIR, LOCALAPPDATA, or HOME before running this uninstaller.'
}

function Remove-FromUserPath {
    param([string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $currentUserPath) {
        return
    }

    $normalizedPathEntry = Normalize-PathEntry -PathEntry $PathEntry
    $existingEntries = @($currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries))
    $entries = @($existingEntries | Where-Object {
        (Normalize-PathEntry -PathEntry $_) -ne $normalizedPathEntry
    })

    if ($entries.Count -eq $existingEntries.Count) {
        return
    }

    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
    Write-Info "Removed $PathEntry from the user PATH."
}

$InstallDir = Resolve-InstallDir -RequestedInstallDir $InstallDir
$binaryPath = Join-Path $InstallDir 'swarm.exe'

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
