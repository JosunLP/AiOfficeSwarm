param(
    [string]$Version = "latest",
    [string]$InstallDir = $(if ($env:SWARM_INSTALL_DIR) { $env:SWARM_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'AiOfficeSwarm\bin' }),
    [switch]$SkipPathUpdate
)

$ErrorActionPreference = 'Stop'

function Write-Info {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Cyan
}

function Get-TargetTriple {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch.ToString()) {
        'X64' { return 'x86_64-pc-windows-msvc' }
        'Arm64' { throw 'Windows Arm64 binaries are not published yet.' }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Get-DownloadUrl {
    param(
        [string]$AssetName,
        [string]$RequestedVersion
    )

    if ($RequestedVersion -eq 'latest') {
        return "https://github.com/JosunLP/AiOfficeSwarm/releases/latest/download/$AssetName"
    }

    if (-not $RequestedVersion.StartsWith('v')) {
        $RequestedVersion = "v$RequestedVersion"
    }

    return "https://github.com/JosunLP/AiOfficeSwarm/releases/download/$RequestedVersion/$AssetName"
}

function Get-ExpectedChecksum {
    param(
        [string]$ChecksumFile,
        [string]$AssetName
    )

    foreach ($line in Get-Content -Path $ChecksumFile) {
        $trimmed = $line.Trim()
        if (-not $trimmed -or $trimmed.StartsWith('#')) {
            continue
        }

        $parts = $trimmed -split '\s+', 3
        if ($parts.Count -lt 2) {
            continue
        }

        $name = $parts[1].TrimStart('*')
        if ($name -eq $AssetName) {
            return $parts[0].ToLowerInvariant()
        }
    }

    throw "Checksum for $AssetName not found in $ChecksumFile"
}

function Add-ToUserPath {
    param([string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @()
    if ($currentUserPath) {
        $entries = $currentUserPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if ($entries -contains $PathEntry) {
        return
    }

    $updatedPath = if ($currentUserPath) {
        "$currentUserPath;$PathEntry"
    }
    else {
        $PathEntry
    }

    [Environment]::SetEnvironmentVariable('Path', $updatedPath, 'User')
    Write-Info "Added $PathEntry to the user PATH."
}

$target = Get-TargetTriple
$assetName = "swarm-$target.zip"
$checksumsName = 'SHA256SUMS'
$downloadUrl = Get-DownloadUrl -AssetName $assetName -RequestedVersion $Version
$checksumsUrl = Get-DownloadUrl -AssetName $checksumsName -RequestedVersion $Version
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
$archivePath = Join-Path $tempRoot $assetName
$checksumsPath = Join-Path $tempRoot $checksumsName
$extractDir = Join-Path $tempRoot 'extract'
$binaryPath = Join-Path $InstallDir 'swarm.exe'

New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

try {
    Write-Info "Downloading $assetName ..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath
    Invoke-WebRequest -Uri $checksumsUrl -OutFile $checksumsPath

    Write-Info 'Verifying checksum ...'
    $expectedChecksum = Get-ExpectedChecksum -ChecksumFile $checksumsPath -AssetName $assetName
    $actualChecksum = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualChecksum -ne $expectedChecksum) {
        throw "Checksum verification failed for $assetName"
    }

    Write-Info "Installing to $InstallDir ..."
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force
    Copy-Item -Path (Join-Path $extractDir 'swarm.exe') -Destination $binaryPath -Force

    if (-not $SkipPathUpdate) {
        Add-ToUserPath -PathEntry $InstallDir
    }

    & $binaryPath --version
    Write-Info 'Installation complete.'
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -Path $tempRoot -Recurse -Force
    }
}
