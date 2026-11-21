# A small window with some text selected, standing in for "another app" (with a menu bar,
# like Notepad, so a lone Alt release would open its menu).
param([string]$text, [int]$seconds = 20)
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form
$f.Text = "BT selection test"; $f.Width = 600; $f.Height = 200; $f.StartPosition = "CenterScreen"; $f.TopMost = $true
$m = New-Object System.Windows.Forms.MenuStrip
[void]$m.Items.Add("&File"); [void]$m.Items.Add("&Edit"); $f.MainMenuStrip = $m
$t = New-Object System.Windows.Forms.TextBox
$t.Multiline = $true; $t.Dock = "Fill"; $t.Text = $text; $t.Font = New-Object System.Drawing.Font("Segoe UI", 14)
$f.Controls.Add($t); $f.Controls.Add($m)
$f.Add_Shown({ $f.Activate(); $t.Focus(); $t.SelectAll() })
$timer = New-Object System.Windows.Forms.Timer; $timer.Interval = $seconds * 1000; $timer.Add_Tick({ $f.Close() }); $timer.Start()
[void]$f.ShowDialog()
