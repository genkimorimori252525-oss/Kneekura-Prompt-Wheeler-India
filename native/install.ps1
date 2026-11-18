# Copies the release build to a stable place and puts a shortcut on the desktop.
# Run after `cargo build --release`:  powershell -ExecutionPolicy Bypass -File install.ps1
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$built = Join-Path $here 'target\release\love-ai-donut.exe'
if (-not (Test-Path $built)) { throw 'Build first: cargo build --release' }

# The display name is assembled from code points so this file stays plain ASCII
# (Windows PowerShell 5.1 misreads UTF-8 scripts without a byte-order mark).
$name = 'Love' + [char]0x611B + [char]0x30C9 + [char]0x30FC + [char]0x30CA + [char]0x30C4
$dir = Join-Path $env:LOCALAPPDATA 'Programs\LoveAiDonut'
$exe = Join-Path $dir 'love-ai-donut.exe'

Get-Process love-ai-donut, kneekura-prompt-wheeler -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $built $exe -Force

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) ($name + '.lnk')))
$link.TargetPath = $exe
$link.WorkingDirectory = $dir
$link.IconLocation = "$exe,0"
$link.Description = $name
$link.Save()

Start-Process $exe
Write-Output ('Installed to ' + $exe)
