# Windows PowerShell Package Build Script for beaM.
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Get-Item "$ScriptDir\..\..").FullName

Set-Location $RepoRoot

if (-not $Version) {
    $Version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
}

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Building Windows packages for beaM. v$Version" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$DistDir = "$RepoRoot\dist"
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Path $DistDir | Out-Null
}

# 1. Compile release binary
Write-Host "[1/3] Compiling release binary for Windows..." -ForegroundColor Yellow
cargo build --release -p beam-app

# 2. Package portable zip
Write-Host "[2/3] Creating portable ZIP bundle..." -ForegroundColor Yellow
$PortableDir = "$DistDir\beaM-v$Version-windows-x86_64"
if (Test-Path $PortableDir) {
    Remove-Item -Recurse -Force $PortableDir
}
New-Item -ItemType Directory -Path "$PortableDir\assets" | Out-Null

Copy-Item "target\release\beam.exe" -Destination "$PortableDir\"
Copy-Item "README.md" -Destination "$PortableDir\"
Copy-Item "LICENSE" -Destination "$PortableDir\"
Copy-Item "assets\*" -Destination "$PortableDir\assets\" -Recurse

$ZipPath = "$DistDir\beaM-v$Version-windows-x86_64.zip"
if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}
Compress-Archive -Path "$PortableDir\*" -DestinationPath $ZipPath
Write-Host "Created portable ZIP: $ZipPath" -ForegroundColor Green

# 3. Build Inno Setup Installer if ISCC is present
Write-Host "[3/3] Compiling Inno Setup installer..." -ForegroundColor Yellow
$IsccPaths = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "ISCC.exe"
)

$Iscc = $null
foreach ($p in $IsccPaths) {
    if (Get-Command $p -ErrorAction SilentlyContinue) {
        $Iscc = $p
        break
    }
}

if ($Iscc) {
    & $Iscc "$ScriptDir\beam-installer.iss"
    Write-Host "Created Inno Setup installer in $DistDir" -ForegroundColor Green
} else {
    Write-Host "ISCC (Inno Setup) not found. Skipping installer generation." -ForegroundColor Gray
    Write-Host "Install Inno Setup 6 from https://jrsoftware.org/isinfo.php to generate .exe installers." -ForegroundColor Gray
}

Write-Host "Windows build complete!" -ForegroundColor Cyan
Get-ChildItem $DistDir | Format-Table Name, Length, LastWriteTime
