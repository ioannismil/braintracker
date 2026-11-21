# Presses a key combo system-wide, e.g.  keys.ps1 "ctrl+shift+e"   (like a real keyboard)
param([string]$combo)
Add-Type -Namespace K -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, System.UIntPtr extra);
'@
$map = @{ ctrl = 0x11; shift = 0x10; alt = 0x12; space = 0x20; enter = 0x0D; esc = 0x1B; a = 0x41; c = 0x43; v = 0x56; win = 0x5B; f9 = 0x78 }
$keys = $combo.ToLower().Split('+') | ForEach-Object {
  if ($map.ContainsKey($_)) { $map[$_] } elseif ($_.Length -eq 1) { [byte][char]$_.ToUpper() } else { throw "unknown key $_" }
}
foreach ($k in $keys) { [K.U]::keybd_event([byte]$k, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
[array]::Reverse($keys)
foreach ($k in $keys) { [K.U]::keybd_event([byte]$k, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
