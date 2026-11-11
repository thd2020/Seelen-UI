# Windows' notification center in Seelen's style

With Seelen's notification popups turned off, this fork leaves notifications to Windows, so banners, the notification center and clicks all work natively: a click opens the exact chat or page, for every app. These two pieces make Windows 10's notification center look and sit like part of Seelen: a compact frosted card right under the top bar, with rounded cards and without the quick-action tiles (Seelen's own quick settings replace them).

## Pieces

- `styler-settings.ini`: the `[Settings]` section for the Windhawk mod [Windows 11 Notification Center Styler](https://windhawk.net/mods/windows-11-notification-center-styler) (version 1.8), which also loads into Windows 10's center. It lays the content out as a 380 × 560 card in the window's top-left corner and restyles it. The element names come from Windows 10 21H2's `ShellExperienceHost` (`ActionCenter.ActionCenterPage`, `ActionCenter.ItemView` and so on).
- `CenterPlacer.cs`: Windows draws the center's window opaque, so the window itself has to become the card. This small background program watches for the center to open, moves and shrinks its window to 380 × 560 at the top right of the work area, just below Seelen's top bar, and rounds its corners. `W` and `H` must match the sizes in `styler-settings.ini`.

## Set up on a PC

1. Extract portable Windhawk without admin rights: `windhawk_setup.exe /S /PORTABLE /D=<folder>`. Then let Windows' shell apps read that folder, since the notification center runs sandboxed: `icacls <folder> /grant *S-1-15-2-1:(OI)(CI)(RX) /T`.
2. Install the styler mod, then paste `styler-settings.ini` into its settings.
3. Build the placer with the C# compiler that ships with Windows: `%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe -target:winexe -out:CenterPlacer.exe CenterPlacer.cs`.
4. Start `windhawk.exe -tray-only` and `CenterPlacer.exe` at sign-in, for example with shortcuts in the Startup folder.

The top-bar bell should open Windows' center: an inline toolbar item with the `Notifications` scope and `onClick: open("ms-actioncenter:")`.
