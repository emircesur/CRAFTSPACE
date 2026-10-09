# End-to-end checks against real ArtCraft releases (Windows): portable installs with Start menu
# shortcuts and Settings › Apps entries, the MSI and NSIS paths, fonts, autostart, self-install.
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$cli = Join-Path $PWD "target\debug\craftspace-cli.exe"
$env:CRAFTSPACE_HOME = Join-Path $env:RUNNER_TEMP "cs"
function Installed($id) { (Get-Content (Join-Path $env:CRAFTSPACE_HOME "installed.json") | ConvertFrom-Json).apps.$id }
function Check($cond, $what) { if (-not $cond) { throw "check failed: $what" } else { Write-Host "ok: $what" } }
$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\ArtCraft"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"

& $cli list
& $cli install photocraft --version 0.3.0
$pc = Installed photocraft
Check ($pc.current.version -eq "0.3.0") "photocraft 0.3.0 installed"
Check (Test-Path $pc.current.executable) "program exists"
Check (Test-Path (Join-Path $startMenu "PhotoCraft.lnk")) "Start menu shortcut"
Check (Test-Path "$uninstallKey\CraftSpace.photocraft") "Settings > Apps entry"
$psdProgids = Get-ItemProperty "HKCU:\Software\Classes\.psd\OpenWithProgids"
Check ($null -ne $psdProgids."CraftSpace.photocraft.Document") "Open with registration for .psd"
Check (-not (Test-Path (Join-Path $pc.current.dir "portable.txt"))) "portable marker removed"
& (Join-Path $pc.current.dir "photocraft-cli.exe") --version
& $cli verify photocraft
& $cli update photocraft
Check ((Installed photocraft).current.version -ne "0.3.0") "updated"
& $cli rollback photocraft
Check ((Installed photocraft).current.version -eq "0.3.0") "rolled back"
# Uninstall the way Settings > Apps does.
$entry = Get-ItemProperty "$uninstallKey\CraftSpace.photocraft"
Write-Host "UninstallString: $($entry.UninstallString)"
cmd /c "$($entry.QuietUninstallString)"
Check ($null -eq (Installed photocraft)) "uninstalled from the registry entry"
Check (-not (Test-Path (Join-Path $startMenu "PhotoCraft.lnk"))) "shortcut removed"
Check (-not (Test-Path "$uninstallKey\CraftSpace.photocraft")) "registry entry removed"

# MSI (Windows Installer) path.
& $cli config prefer_system_installer true
& $cli -q install pdfcraft
$pdf = Installed pdfcraft
Check ($pdf.current.kind -eq "msi") "installed with the MSI"
Write-Host "PdfCraft program: $($pdf.current.executable)"
Check ($pdf.current.executable -and (Test-Path $pdf.current.executable)) "MSI program found through its Settings > Apps entry"
& $cli -q uninstall pdfcraft --yes
Check ($null -eq (Installed pdfcraft)) "MSI uninstalled"
& $cli config prefer_system_installer false

# ArtCraft ships an NSIS setup program (per user, silent with /S).
& $cli -q install artcraft
$ac = Installed artcraft
Check ($ac.current.kind -eq "exe") "ArtCraft installed with its setup program"
Write-Host "ArtCraft program: $($ac.current.executable)"
Check ($ac.current.executable -and (Test-Path $ac.current.executable)) "ArtCraft program found"
& $cli -q uninstall artcraft --yes
Start-Sleep -Seconds 5
Check (-not (Test-Path $ac.current.executable)) "ArtCraft uninstalled"

# Fonts.
& $cli fonts install "Noto Sans Arabic"
$fontFile = Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Fonts\NotoSansArabic.ttf"
Check (Test-Path $fontFile) "font file installed"
$fonts = Get-ItemProperty "HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts"
Check ($null -ne $fonts."Noto Sans Arabic Regular (TrueType)") "font registered"
& $cli fonts uninstall
Check (-not (Test-Path $fontFile)) "font removed"

# Start at login.
& $cli autostart on
Check ($null -ne (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run")."CraftSpace") "Run entry"
& $cli autostart off

# Self-install, then uninstall it the way Settings > Apps would.
& $cli self-install
Check (Test-Path (Join-Path $startMenu "CraftSpace.lnk")) "CraftSpace in the Start menu"
Check (Test-Path "$uninstallKey\CraftSpace.craftspace") "CraftSpace in Settings > Apps"
Write-Host "Windows end-to-end checks passed"
