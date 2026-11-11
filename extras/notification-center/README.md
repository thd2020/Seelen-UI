# Windows' notification center in Seelen's style

With Seelen's notification popups turned off, this fork leaves notifications to Windows, so banners, the notification center and clicks all work natively: a click opens the exact chat or page, for every app. These pieces make Windows 10's notification center and banners look and sit like part of Seelen. The center is a compact frosted card right under the top bar, without the quick-action tiles, since Seelen's own quick settings replace them. Every notification is its own rounded card with its app's icon and name on top, Android-style, and apps your Seelen theme has an icon for show that line icon instead of their logo. Settings and clear-all are round icon buttons (gear and broom). Banners appear under the top bar as the same rounded card, with the theme icon too.

## Pieces

- `styles.txt`: the styles for the Windhawk mod [Windows 11 Notification Center Styler](https://windhawk.net/mods/windows-11-notification-center-styler) (version 1.8), which also loads into Windows 10's center. A line starting at the left margin names the element, the indented lines below it are its styles, and lines starting with `#!` are comments. The element names come from Windows 10 21H2's `ShellExperienceHost` (`ActionCenter.ActionCenterPage`, `ActionCenter.ItemView`, `ActionCenter.ToastView` and so on). The center's content is laid out as a 380 × 560 card in the window's top-left corner.
- `apply-styles.ps1`: writes `styles.txt` into the mod's settings in a portable Windhawk (it expects to sit in a folder inside the Windhawk folder) and restarts Windows' notification host so they apply.
- `seelen-notification-cards.wh.cpp`: a small Windhawk mod. Windows 10's center only names the app on a per-app header row; this mod adds an icon-and-name row to every card (finding each card's app through the list's group headers, which the styles hide) and swaps in theme icons on cards and banners.
- `install-icons.ps1`: renders the theme's SVG tray icons to PNG with headless Chrome and puts them, with a name-to-icon map built from the theme's `apps.json`, into the notification host's own `LocalState\seelen-icons` folder, the only place its sandbox can read.
- `CenterPlacer.cs`: Windows draws these windows opaque, so the windows themselves have to take the card's shape. This small background program moves and shrinks the center's window to 380 × 560 at the top right of the work area, just below Seelen's top bar, and rounds it. It also moves each banner window there and cuts it to its card. Window shapes have no anti-aliasing, so the corners keep only pixels the card fully covers, which leaves no dark fringe. `W` and `H` must match the sizes in `styles.txt`.

## Set up on a PC

1. Extract portable Windhawk without admin rights: `windhawk_setup.exe /S /PORTABLE /D=<folder>`. Then let Windows' shell apps read that folder, since the notification center runs sandboxed: `icacls <folder> /grant *S-1-15-2-1:(OI)(CI)(RX) /T`.
2. Install the styler mod and compile `seelen-notification-cards.wh.cpp` as a second mod (Windhawk's Create a new mod, or its bundled compiler). Put this folder inside the Windhawk folder, then run `install-icons.ps1` and `apply-styles.ps1`.
3. Build the placer with the C# compiler that ships with Windows: `%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe -target:winexe -out:CenterPlacer.exe CenterPlacer.cs`.
4. Start `windhawk.exe -tray-only` and `CenterPlacer.exe` at sign-in, for example with shortcuts in the Startup folder.

The top-bar bell should open Windows' center: an inline toolbar item with the `Notifications` scope and `onClick: open("ms-actioncenter:")`.
