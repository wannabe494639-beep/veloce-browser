param(
  [string]$MsiPath,
  [string]$OutputPath
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$bundleRoot = Join-Path $projectRoot "src-tauri\target\release\bundle"

if ([string]::IsNullOrWhiteSpace($MsiPath)) {
  $msiCandidate = Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*.msi" -File |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
  if (-not $msiCandidate) { throw "Kein Tauri-MSI gefunden. Erst `npx tauri build --bundles msi` ausführen." }
  $MsiPath = $msiCandidate.FullName
}
$MsiPath = (Resolve-Path -LiteralPath $MsiPath).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $OutputPath = Join-Path $bundleRoot "Veloce-Browser-Setup.exe"
}
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$iexpressPath = Join-Path $env:WINDIR "System32\iexpress.exe"
if (-not (Test-Path -LiteralPath $iexpressPath -PathType Leaf)) {
  throw "IExpress wurde unter Windows nicht gefunden: $iexpressPath"
}

$stagingPath = Join-Path ([System.IO.Path]::GetTempPath()) ("VeloceSetup-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $stagingPath | Out-Null
Copy-Item -LiteralPath $MsiPath -Destination (Join-Path $stagingPath "veloce.msi")
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "iexpress\install.cmd") -Destination $stagingPath

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class VeloceShortPath {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern uint GetShortPathName(string path, StringBuilder shortPath, uint capacity);
}
"@
function Get-IExpressPath([string]$path) {
  $buffer = New-Object System.Text.StringBuilder 2048
  $length = [VeloceShortPath]::GetShortPathName($path, $buffer, [uint32]$buffer.Capacity)
  if ($length -gt 0) { return $buffer.ToString() }
  if ($path -match "\s") { throw "IExpress kann den Pfad ohne 8.3-Kurzpfad nicht sicher verarbeiten: $path" }
  return $path
}

$sedPath = Join-Path $stagingPath "VeloceBrowserSetup.sed"
$sedOutputPath = Get-IExpressPath $OutputPath
$sedStagePath = Get-IExpressPath $stagingPath
$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=1
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=Veloce Browser Setup wurde beendet.
TargetName=$sedOutputPath
FriendlyName=Veloce Browser Setup
AppLaunched=cmd.exe /c install.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[Strings]
InstallPrompt=
DisplayLicense=
FinishMessage=Veloce Browser Setup wurde beendet.
TargetName=$sedOutputPath
FriendlyName=Veloce Browser Setup
AppLaunched=cmd.exe /c install.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
FILE0="install.cmd"
FILE1="veloce.msi"
[SourceFiles]
SourceFiles0=$sedStagePath\
[SourceFiles0]
%FILE0%=
%FILE1%=
"@
[System.IO.File]::WriteAllText($sedPath, $sed, [System.Text.Encoding]::Default)

$sedForProcess = Get-IExpressPath $sedPath
$process = Start-Process -FilePath $iexpressPath -ArgumentList @("/N", "/Q", $sedForProcess) -Wait -PassThru -NoNewWindow
if ($process.ExitCode -ne 0) { throw "IExpress ist mit Exitcode $($process.ExitCode) fehlgeschlagen." }
if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) { throw "IExpress meldete Erfolg, aber die Setup-Datei fehlt: $OutputPath" }
$output = Get-Item -LiteralPath $OutputPath
if ($output.Length -lt 1MB) { throw "Die erzeugte Setup-Datei ist unerwartet klein ($($output.Length) Bytes)." }
Write-Output "Windows-Setup erzeugt: $($output.FullName) ($([math]::Round($output.Length / 1MB, 2)) MiB)"
