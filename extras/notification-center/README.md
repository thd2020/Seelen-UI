# Windows' notification center in Seelen's style

With Seelen's notification popups turned off, this fork leaves notifications to Windows, so banners, the notification center and clicks all work natively: a click opens the exact chat or page, for every app. These pieces make Windows 10's notification center and banners look and sit like part of Seelen. The center is a compact frosted card right under the top bar, without the quick-action tiles, since Seelen's own quick settings replace them. Every notification is its own rounded card with its app's icon and name on top, Android-style, and apps your Seelen theme has an icon for show that line icon instead of their logo. Settings and clear-all are round icon buttons (gear and broom). Banners appear under the top bar as the same rounded card, with the theme icon too.

## Pieces

- `styles.txt`: the styles for the Windhawk mod [Windows 11 Notification Center Styler](https://windhawk.net/mods/windows-11-notification-center-styler) (version 1.8), which also loads into Windows 10's center. A line starting at the left margin names the element, the indented lines below it are its styles, and lines starting with `#!` are comments. The element names come from Windows 10 21H2's `ShellExperienceHost` (`ActionCenter.ActionCenterPage`, `ActionCenter.ItemView`, `ActionCenter.ToastView` and so on). The center's content is laid out as a 380 × 560 card in the window's top-left corner.
- `apply-styles.ps1`: writes `styles.txt` into the mod's settings in a portable Windhawk (it expects to sit in a folder inside the Windhawk folder) and restarts Windows' notification host so they apply.
- `seelen-notification-cards.wh.cpp`: a small Windhawk mod running inside the notification host. Windows 10's center only names the app on a per-app header row; the mod adds an icon-and-name row to every card (finding each card's app through the list's group headers, which the styles hide), swaps in theme icons on cards and banners, and hides conversation headers (apps such as Claude group notifications under a title row that would read as a duplicate card). It also shapes the center: the window keeps the full-height size the shell gives it (the shell's open animation is built for that size; resizing the window made it draw the content zoomed, then snap), the styles lay the content out as a card 8 px in from the window's corner, and the mod cuts the window to that card's rounded shape. And it notes when the center loses focus, for the bell.
- `install-icons.ps1`: renders the theme's SVG tray icons to PNG with headless Chrome (apps listed in `-KeepOwnLogo`, phevere by default, keep their own logo) and puts them, with a name-to-icon map built from the theme's `apps.json`, into the notification host's own `LocalState\seelen-icons` folder, the only place its sandbox can read.
- `CenterPlacer.cs`: a small background program. Windows draws banners opaque in the bottom-right corner, so it moves each banner window under the top bar and cuts it to its rounded card (window shapes have no anti-aliasing, so the corners keep only pixels the card fully covers). Run with a `seelen-center:` link as its argument, it toggles the center for the top-bar bell: Windows' own command only ever opens it, so an open center is closed with Escape; a closed one is opened, unless a click elsewhere closed it a moment ago (the mod notes when the center loses focus). The card size in the mod and in `styles.txt` must match (380 × 560 at 8, 8).

## Set up on a PC

1. Extract portable Windhawk without admin rights: `windhawk_setup.exe /S /PORTABLE /D=<folder>`. Then let Windows' shell apps read that folder, since the notification center runs sandboxed: `icacls <folder> /grant *S-1-15-2-1:(OI)(CI)(RX) /T`.
2. Install the styler mod and compile `seelen-notification-cards.wh.cpp` as a second mod (Windhawk's Create a new mod, or its bundled compiler). Put this folder inside the Windhawk folder, then run `install-icons.ps1` and `apply-styles.ps1`.
3. Build the placer with the C# compiler that ships with Windows: `%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe -target:winexe -out:CenterPlacer.exe CenterPlacer.cs`.
4. Start `windhawk.exe -tray-only` and `CenterPlacer.exe` at sign-in, for example with shortcuts in the Startup folder.

The top-bar bell opens and closes Windows' center: an inline toolbar item with the `Notifications` scope, `badge: return count > 0 ? count : null` and `onClick: open("seelen-center:toggle")`. Register the link for your user once:

```powershell
$exe = "<folder>\CenterPlacer.exe"; $k = 'HKCU:\Software\Classes\seelen-center'
New-Item "$k\shell\open\command" -Force | Out-Null
Set-ItemProperty $k '(default)' 'URL:Seelen notification center'; Set-ItemProperty $k 'URL Protocol' ''
Set-ItemProperty "$k\shell\open\command" '(default)' "`"$exe`" `"%1`""
```

Known limit: Windows 10 draws a faint one-pixel line just outside the left edge of the center's window (the shell's own decoration for edge panels, outside the window and its XAML). With the window kept at the shell's size, that line runs the full height a few pixels left of the card.
