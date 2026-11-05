<#
.SYNOPSIS
  Installs PrintBridge for the current user and starts it at logon (Windows).

.DESCRIPTION
  Copies printbridge.exe to %LOCALAPPDATA%\PrintBridge and registers a per-user
  scheduled task that starts it hidden at logon and restarts it if it exits.
  No administrator rights are needed.

.EXAMPLE
  .\install.ps1 -Binary .\printbridge.exe
  .\install.ps1 -Binary .\printbridge.exe -AllowedOrigins "https://app.example.com"
  .\install.ps1 -Uninstall
#>
param(
  [string]$Binary = ".\printbridge.exe",
  [string]$AllowedOrigins = "*",
  [int]$Port = 9631,
  [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'PrintBridge'
$Target = Join-Path $InstallDir 'printbridge.exe'
$TaskName = 'PrintBridge'

if ($Uninstall) {
  Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
  Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
  Get-Process printbridge -ErrorAction SilentlyContinue | Stop-Process -Force
  Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue
  Write-Host 'PrintBridge removed.'
  return
}

if (-not (Test-Path $Binary)) { throw "Binary not found: $Binary" }

Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Get-Process printbridge -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $Binary $Target -Force
# Files downloaded from the internet carry a Mark-of-the-Web that triggers SmartScreen.
Unblock-File $Target

$taskArgs = "--port $Port --allowed-origins `"$AllowedOrigins`""
$action = New-ScheduledTaskAction -Execute $Target -Argument $taskArgs
$trigger = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
  -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -Hidden
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings `
  -Description 'PrintBridge local print agent' -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName

Write-Host "Installed to $Target and registered to start at logon."
Write-Host "Check: Invoke-RestMethod http://127.0.0.1:$Port/api/health"
