# Capture the VIctory window's own pixels via PrintWindow (works even when it is
# occluded by other windows), and print its rect so we know where it is.
param([string]$Out = (Join-Path $env:TEMP 'victory-shot.png'))
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public struct RC2 { public int L, T, R, B; }
public class PW {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RC2 r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
"@
[PW]::SetProcessDPIAware() | Out-Null
$p = Get-Process victory -ErrorAction SilentlyContinue
if (-not $p) { Write-Host 'no process'; exit 1 }
$p.Refresh()
$h = $p.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Host 'no window'; exit 1 }
if ([PW]::IsIconic($h)) { [PW]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Milliseconds 300 }

$r = New-Object RC2
[PW]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
Write-Host ("rect: {0},{1} {2}x{3}" -f $r.L, $r.T, $w, $ht)

$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [PW]::PrintWindow($h, $hdc, 2)   # PW_RENDERFULLCONTENT (DWM-composed frames)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "PrintWindow=$ok saved $Out"
