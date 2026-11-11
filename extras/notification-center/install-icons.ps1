# Renders the Seelen theme's tray icons (SVG) to PNG and installs them, with a name map, where the
# seelen-notification-cards mod reads them: the notification host's own LocalState\seelen-icons
# folder, the only place its sandbox can read. Needs Google Chrome for the SVG rendering.
param(
    [string]$ThemeIcons = "$env:APPDATA\com.seelen.seelen-ui\themes\glass-bars\tray-icons",
    [string]$Chrome = "$env:ProgramFiles\Google\Chrome\Application\chrome.exe"
)
$ErrorActionPreference = 'Stop'
$dest = "$env:LOCALAPPDATA\Packages\Microsoft.Windows.ShellExperienceHost_cw5n1h2txyewy\LocalState\seelen-icons"
$work = Join-Path $env:TEMP 'seelen-icons-render'
New-Item -ItemType Directory -Force $dest, $work | Out-Null

foreach ($svg in Get-ChildItem $ThemeIcons -Filter *.svg) {
    $page = Join-Path $work ($svg.BaseName + '.html')
    $src = ([Uri]$svg.FullName).AbsoluteUri
    Set-Content $page "<html><body style='margin:0;background:transparent'><img src='$src' style='width:96px;height:96px;display:block'></body></html>"
    $png = Join-Path $dest ($svg.BaseName + '.png')
    # Chrome reports progress on stderr, which Windows PowerShell would treat as an error.
    Start-Process $Chrome -Wait -WindowStyle Hidden -ArgumentList @(
        '--headless=old', '--disable-gpu', '--hide-scrollbars', '--default-background-color=00000000',
        '--window-size=96,96', "--screenshot=`"$png`"", ([Uri]$page).AbsoluteUri)
}
Remove-Item $work -Recurse -Force

# Every name an app goes by (window titles from apps.json and the program's file name) -> its icon.
$apps = Get-Content (Join-Path $ThemeIcons 'apps.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$map = [ordered]@{}
foreach ($p in $apps.PSObject.Properties) {
    $png = $p.Value.icon -replace '\.svg$', '.png'
    $map[[IO.Path]::GetFileNameWithoutExtension($p.Name)] = $png
    foreach ($t in $p.Value.tooltips) { $map[$t] = $png }
}
$lines = $map.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }
[IO.File]::WriteAllText((Join-Path $dest 'map.txt'), ($lines -join "`r`n") + "`r`n", (New-Object Text.UTF8Encoding $false))
"installed $((Get-ChildItem $dest -Filter *.png).Count) icons and $($map.Count) names in $dest"
