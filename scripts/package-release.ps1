[CmdletBinding()]
param(
    [string[]]$Targets = @('windows-x64'),
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$DistDir = Join-Path $RepoRoot 'dist'
$StageRoot = Join-Path $DistDir 'stage'

$TargetMap = @{
    'windows-x64' = @{
        Triple = 'x86_64-pc-windows-msvc'
        Binary = 'veyra-app.exe'
        Archive = 'veyra-launcher-windows-x64.zip'
        Format = 'zip'
    }
    'windows-arm64' = @{
        Triple = 'aarch64-pc-windows-msvc'
        Binary = 'veyra-app.exe'
        Archive = 'veyra-launcher-windows-arm64.zip'
        Format = 'zip'
    }
    'linux-x64' = @{
        Triple = 'x86_64-unknown-linux-gnu'
        Binary = 'veyra-app'
        Archive = 'veyra-launcher-linux-x64.tar.gz'
        Format = 'tar.gz'
    }
    'linux-arm64' = @{
        Triple = 'aarch64-unknown-linux-gnu'
        Binary = 'veyra-app'
        Archive = 'veyra-launcher-linux-arm64.tar.gz'
        Format = 'tar.gz'
    }
}

function Copy-PackagePayload {
    param(
        [Parameter(Mandatory = $true)][string]$BinaryPath,
        [Parameter(Mandatory = $true)][string]$StageDir
    )

    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    Copy-Item -LiteralPath $BinaryPath -Destination $StageDir -Force
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'README.md') -Destination $StageDir -Force
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'LICENSE') -Destination $StageDir -Force
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'docs') -Destination $StageDir -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'scripts') -Destination $StageDir -Recurse -Force
}

function Write-Checksum {
    param([Parameter(Mandatory = $true)][string]$ArchivePath)

    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath
    $line = "{0}  {1}" -f $hash.Hash.ToLowerInvariant(), (Split-Path -Leaf $ArchivePath)
    Set-Content -LiteralPath "$ArchivePath.sha256" -Value $line -NoNewline
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
New-Item -ItemType Directory -Force -Path $StageRoot | Out-Null

foreach ($target in $Targets) {
    if (-not $TargetMap.ContainsKey($target)) {
        throw "Unknown target alias '$target'. Expected one of: $($TargetMap.Keys -join ', ')"
    }

    $entry = $TargetMap[$target]
    $triple = $entry.Triple
    $binaryName = $entry.Binary
    $archiveName = $entry.Archive
    $packageName = "veyra-launcher-$target"
    $stageDir = Join-Path $StageRoot $packageName
    $archivePath = Join-Path $DistDir $archiveName
    $binaryPath = Join-Path $RepoRoot "target\$triple\release\$binaryName"

    if (-not $SkipBuild) {
        cargo build --release --target $triple -p veyra-app
    }

    if (-not (Test-Path -LiteralPath $binaryPath)) {
        throw "Missing binary: $binaryPath"
    }

    Remove-Item -LiteralPath $stageDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $archivePath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath "$archivePath.sha256" -Force -ErrorAction SilentlyContinue

    Copy-PackagePayload -BinaryPath $binaryPath -StageDir $stageDir

    if ($entry.Format -eq 'zip') {
        Compress-Archive -Path (Join-Path $stageDir '*') -DestinationPath $archivePath -Force
    } else {
        tar -czf $archivePath -C $StageRoot $packageName
    }

    Write-Checksum -ArchivePath $archivePath
    Write-Host "Packaged $archivePath"
}
