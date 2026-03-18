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
