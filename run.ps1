param([switch]$SkipBuild)
taskkill /IM craboard.exe /F 2>$null | Out-Null
Start-Sleep -Seconds 1
if (-not $SkipBuild) {
  cargo build --release 2>&1 | Select-String -Pattern "^error" -Context 0,6
  if ($LASTEXITCODE -ne 0) { Write-Output "BUILD FAILED"; exit 1 }
}
Start-Process -FilePath "D:\Code\craboard\target\release\craboard.exe"
Start-Sleep -Seconds 5
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class Shot {
  [DllImport("user32.dll")] public static extern int SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
"@
[void][Shot]::SetProcessDpiAwarenessContext([IntPtr](-4))
$w=[Shot]::GetSystemMetrics(0); $h=[Shot]::GetSystemMetrics(1)
$bmp=New-Object System.Drawing.Bitmap($w,$h)
$g=[System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0,0,0,0,(New-Object System.Drawing.Size($w,$h)))
$bmp.Save("D:\Code\craboard\shot.png",[System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose();$bmp.Dispose()
Write-Output "captured ${w}x${h}"
