param(
  [string]$Version = $(if ($env:VERSION) { $env:VERSION } else { "latest" }),
  [string]$Repo = $(if ($env:REPO) { $env:REPO } else { "biglone/codex-history-viewer" }),
  [switch]$Silent,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
  @"
Install Codex History Viewer desktop app from GitHub Releases.

Usage:
  .\install-gui.ps1 [-Version TAG|latest] [-Repo OWNER/REPO] [-Silent]

Environment:
  VERSION       Release tag to install, defaults to latest
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases

Examples:
  iwr https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.ps1 -OutFile install-gui.ps1
  .\install-gui.ps1 -Version v1.3.8
  .\install-gui.ps1 -Version v1.3.8 -Silent
"@
}

if ($Help) {
  Show-Usage
  exit 0
}

function Get-AssetNames {
  $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
  switch ($arch) {
    "X64" {
      return @{
        Stable = "codex-history-viewer-Windows-x64-setup.exe"
        Pattern = "Codex.History.Viewer_*_x64-setup.exe"
      }
    }
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
  param([string]$Pattern, [string]$Destination, [string]$TmpDir)

  $gh = Get-Command gh -ErrorAction SilentlyContinue
  if (-not $gh) {
    throw "GitHub CLI is not installed"
  }

  if ($Version -eq "latest") {
    gh release download --repo $Repo --pattern $Pattern --dir $TmpDir --clobber
  } else {
    gh release download $Version --repo $Repo --pattern $Pattern --dir $TmpDir --clobber
  }

  $downloaded = Get-ChildItem -Path $TmpDir -Filter $Pattern -File | Select-Object -First 1
  if (-not $downloaded) {
    throw "No release asset matched $Pattern"
  }
  Move-Item $downloaded.FullName $Destination -Force
}

$asset = Get-AssetNames
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("codex-history-gui-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir | Out-Null
$installer = Join-Path $tmpDir $asset.Stable

if ($Version -eq "latest") {
  $url = "https://github.com/$Repo/releases/latest/download/$($asset.Stable)"
} else {
  $url = "https://github.com/$Repo/releases/download/$Version/$($asset.Stable)"
}

try {
  Write-Host "Installing $($asset.Stable) from $Repo@$Version"
  try {
    Download-Direct -Url $url -Destination $installer
  } catch {
    Write-Warning "Direct download failed; trying GitHub CLI with pattern $($asset.Pattern)..."
    Download-WithGh -Pattern $asset.Pattern -Destination $installer -TmpDir $tmpDir
  }

  if ($Silent) {
    Start-Process -FilePath $installer -ArgumentList "/S" -Wait
  } else {
    Start-Process -FilePath $installer -Wait
  }
} finally {
  Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
