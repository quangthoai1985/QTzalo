param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Theme = "light",
    [int]$WaitSec = 9
)
# Launch QTzalo in --demo mode (offline, writes no data) and capture its window with PrintWindow.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
}
"@
[Win]::SetProcessDPIAware() | Out-Null
$env:QTZALO_DEMO_THREAD = "0"
$env:QTZALO_DEMO_REACT_BAR = "1"
$env:QTZALO_DEMO_SIZE = "1280x800"
$env:QTZALO_DEMO_THEME = $Theme
$p = Start-Process -FilePath $Exe -ArgumentList "--gui", "--demo" -PassThru
try {
    Start-Sleep -Seconds $WaitSec
    $p.Refresh()
    $h = $p.MainWindowHandle
    if ($h -eq [IntPtr]::Zero) { throw "no main window" }
    $r = New-Object Win+RECT
    [Win]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Win]::PrintWindow($h, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    "saved $Out ${w}x${hh}"
} finally {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
