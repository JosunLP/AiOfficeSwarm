# Keep the fallback copies in install.ps1 and uninstall.ps1 in sync with these functions.
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

function Resolve-InstallDir {
    param(
        [string]$RequestedInstallDir,
        [string]$ScriptLabel = 'script'
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedInstallDir)) {
        return $RequestedInstallDir.Trim()
    }

    if ($env:LOCALAPPDATA) {
        return (Join-Path $env:LOCALAPPDATA 'AiOfficeSwarm\bin')
    }

    if ($env:HOME) {
        return (Join-Path $env:HOME 'AppData\Local\AiOfficeSwarm\bin')
    }

    throw "Set SWARM_INSTALL_DIR, LOCALAPPDATA, or HOME before running this $ScriptLabel."
}
