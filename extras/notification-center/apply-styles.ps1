# Writes the Notification Center Styler settings from styles.txt and reloads the notification host.
# styles.txt: a "target" line starts a control, the indented lines under it are its styles.
param([string]$StylesFile = "$PSScriptRoot\styles.txt")
$w = Split-Path $PSScriptRoot -Parent
$f = "$w\AppData\Engine\Mods\windows-11-notification-center-styler.ini"
$cur = [IO.File]::ReadAllText($f, [Text.Encoding]::Unicode)
$dll = [regex]::Match($cur, 'LibraryFileName=([^\r\n]+)').Groups[1].Value
$lines = @('[Settings]', 'theme=')
$i = -1; $j = 0
foreach ($raw in Get-Content $StylesFile -Encoding UTF8) {
    if ($raw.Trim() -eq '' -or $raw.TrimStart().StartsWith('#!')) { continue }
    if ($raw -match '^\S') { $i++; $j = 0; $lines += "controlStyles[$i].target=$($raw.Trim())" }
    else { $lines += "controlStyles[$i].styles[$j]=$($raw.Trim())"; $j++ }
}
$t = [int]([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() -band 0x7fffffff)
$ini = "[Mod]`r`nLibraryFileName=$dll`r`nDisabled=0`r`nLoggingEnabled=0`r`nInclude=ShellExperienceHost.exe|ShellHost.exe`r`nArchitecture=x86-64`r`nVersion=1.8`r`nSettingsChangeTime=$t`r`n" + ($lines -join "`r`n") + "`r`n"
[IO.File]::WriteAllText($f, $ini, [Text.Encoding]::Unicode)
Stop-Process -Name ShellExperienceHost -Force -ErrorAction SilentlyContinue
"applied $($i + 1) targets"
