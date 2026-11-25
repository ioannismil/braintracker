# A plain full-screen backdrop on the primary screen, so README recordings show the app on a
# clean background instead of your own desktop. Started and closed by demo.mjs.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$f = New-Object System.Windows.Forms.Form
$f.FormBorderStyle = 'None'
$f.StartPosition = 'Manual'
$f.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$f.ShowInTaskbar = $false
$f.Text = 'BrainTracker demo backdrop'
$f.Add_Paint({
    param($s, $e)
    $r = $s.ClientRectangle
    $top = [System.Drawing.Color]::FromArgb(0x5a, 0x46, 0x3a)
    $bottom = [System.Drawing.Color]::FromArgb(0x2b, 0x24, 0x20)
    $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush($r, $top, $bottom, 60)
    $e.Graphics.FillRectangle($br, $r)
    $br.Dispose()
  })
[System.Windows.Forms.Application]::Run($f)
