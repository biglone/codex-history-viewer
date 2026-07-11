param(
  [string]$Version = $(if ($env:VERSION) { $env:VERSION } else { "latest" }),
  [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\codex-history-viewer\bin" }),
  [string]$Repo = $(if ($env:REPO) { $env:REPO } else { "biglone/codex-history-viewer" }),
  [switch]$AddToPath,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
  @"
Install codex-history-cli from GitHub Releases.

Usage:
  .\install-cli.ps1 [-Version TAG|latest] [-InstallDir DIR] [-Repo OWNER/REPO] [-AddToPath]

Environment:
  VERSION       Release tag to install, defaults to latest
  INSTALL_DIR   Install directory, defaults to %LOCALAPPDATA%\Programs\codex-history-viewer\bin
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases

Examples:
  iwr https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-cli.ps1 -OutFile install-cli.ps1
  .\install-cli.ps1 -Version v1.3.7 -AddToPath
"@
}

if ($Help) {
  Show-Usage
  exit 0
}

function Get-AssetName {
  $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
  switch ($arch) {
    "X64" { return "codex-history-cli-Windows-x64.exe" }
    default { throw "Unsupported Windows architecture: $arch" }
  }
}

function Download-Direct {
  param([string]$Url, [string]$Destination)

  $headers = @{}
  if ($env:GH_TOKEN) {
    $headers["Authorization"] = "Bearer $($env:GH_TOKEN)"
    $headers["X-GitHub-Api-Version"] = "2022-11-28"
  }

  Invoke-WebRequest -Uri $Url -OutFile $Destination -Headers $headers
}

function Download-WithGh {
  param([string]$Asset, [string]$Destination)

  $gh = Get-Command gh -ErrorAction SilentlyContinue
  if (-not $gh) {
    throw "GitHub CLI is not installed"
  }

  if ($Version -eq "latest") {
    gh release download --repo $Repo --pattern $Asset --output $Destination --clobber
  } else {
    gh release download $Version --repo $Repo --pattern $Asset --output $Destination --clobber
  }
}

$asset = Get-AssetName
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("codex-history-cli-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir | Out-Null
$tmpExe = Join-Path $tmpDir $asset

if ($Version -eq "latest") {
  $url = "https://github.com/$Repo/releases/latest/download/$asset"
} else {
  $url = "https://github.com/$Repo/releases/download/$Version/$asset"
}

try {
  Write-Host "Installing $asset from $Repo@$Version"
  try {
    Download-Direct -Url $url -Destination $tmpExe
  } catch {
    Write-Warning "Direct download failed; trying GitHub CLI..."
    Download-WithGh -Asset $asset -Destination $tmpExe
  }

  New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
  $target = Join-Path $InstallDir "codex-history-cli.exe"
  Copy-Item $tmpExe $target -Force

  if ($AddToPath) {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $paths = @($currentPath -split ";") | Where-Object { $_ }
    if ($paths -notcontains $InstallDir) {
      [Environment]::SetEnvironmentVariable("Path", ($paths + $InstallDir -join ";"), "User")
      Write-Host "Added to user PATH. Open a new PowerShell session to use codex-history-cli.exe directly."
    }
  }

  Write-Host "Installed: $target"
  & $target --help
} finally {
  Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
