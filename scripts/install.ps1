param(
  [string]$Target = $(if ($env:TARGET) { $env:TARGET } else { "" }),
  [string]$Version = $(if ($env:VERSION) { $env:VERSION } else { "latest" }),
  [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "" }),
  [string]$Repo = $(if ($env:REPO) { $env:REPO } else { "biglone/codex-history-viewer" }),
  [switch]$AddToPath,
  [switch]$Silent,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
  @"
Install Codex History Viewer GUI or CLI from the command line.

Usage:
  .\install.ps1 -Target cli|gui [-Version TAG|latest] [-InstallDir DIR] [-Repo OWNER/REPO] [-AddToPath] [-Silent]

Environment:
  TARGET        cli or gui
  VERSION       Release tag to install, defaults to latest
  INSTALL_DIR   Install directory passed to the underlying installer
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases

Examples:
  iwr https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install.ps1 -OutFile install.ps1
  .\install.ps1 -Target cli -Version v1.3.8 -AddToPath
  .\install.ps1 -Target gui -Version v1.3.8
"@
}

if ($Help) {
  Show-Usage
  exit 0
}

if (-not $Target) {
  throw "Target is required. Use -Target cli or -Target gui."
}

if ($Target -notin @("cli", "gui")) {
  throw "Unsupported target: $Target. Use -Target cli or -Target gui."
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$helperName = "install-$Target.ps1"
$helperPath = Join-Path $scriptDir $helperName

function Invoke-Helper {
  param([string]$Helper)

  $params = @{
    Version = $Version
    Repo = $Repo
  }

  if ($InstallDir) {
    $params["InstallDir"] = $InstallDir
  }
  if ($Target -eq "cli" -and $AddToPath) {
    $params["AddToPath"] = $true
  }
  if ($Target -eq "gui" -and $Silent) {
    $params["Silent"] = $true
  }

  & $Helper @params
}

if (Test-Path $helperPath) {
  Invoke-Helper -Helper $helperPath
  exit 0
}

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("codex-history-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir | Out-Null
$tmpHelper = Join-Path $tmpDir $helperName
$rawUrl = "https://raw.githubusercontent.com/$Repo/main/scripts/$helperName"

try {
  Invoke-WebRequest -Uri $rawUrl -OutFile $tmpHelper
  Invoke-Helper -Helper $tmpHelper
} finally {
  Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
