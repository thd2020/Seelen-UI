> Updated 2026-11-13: Seelen UI does all of this itself now. Windhawk, its two mods, `CenterPlacer.exe` and the scripts
> are no longer used; see "Moving from the Windhawk setup" below.

# Windows' notification center in Seelen's style

With Seelen's notification popups turned off, this fork leaves notifications to Windows, so banners, the notification
center and clicks all work natively: a click opens the exact chat or page, for every app. On Windows 10, Seelen then
makes Windows' notification center and banners look and sit like part of Seelen:

- The center is a compact frosted card (396 × 560 at 100% scale) right under the top bar, without the quick-action
  tiles, since Seelen's own quick settings replace them. It opens and closes without a zoom.
- Notifications are rounded cards with their app's icon and name on top, Android-style. Apps the active theme has a
  small icon for show that icon instead of their logo, and Windows' default app icon fills in when neither is available.
  Conversation headers are hidden, and matching cards in the same app group show once at a time (dismissing one may
  reveal the next copy, since Windows' notifications stay as they are).
- Settings and clear-all are round icon buttons (gear and broom).
- Banners appear under the top bar as the same rounded card, with the theme icon too.

## How it works

The code lives in the app itself:

- `src/hook_dll/src/notification_center/` is a component of `sluhk.dll` that runs inside the notification host
  (`ShellExperienceHost.exe`), on the UI thread of each of its windows. It sizes the center's window to the card and
  moves the banners under the top bar from the window's own `WM_WINDOWPOSCHANGING`, before Windows applies the shell's
  size, so the window is never resized while it opens. It turns off the window's DWM transitions, and restyles the
  host's XAML (`style.rs` has the rules that used to be `styles.txt`; `cards.rs` the per-card rows).
- `src/background/system_modules/native_notification_center/` loads the component: it copies `sluhk.dll` into the host's
  own `LocalState` folder (the only place the sandboxed host can read), hooks the threads of the host's windows so
  Windows loads it there, and attaches it to each window as it shows. It also copies the active theme's small icons
  there (`tray-icons/apps.json` and the files it lists; SVG files are drawn as they are, no rendering step).
- The component pins itself in the host, so it keeps working if Seelen closes, until the host restarts. Turning Seelen's
  notification popups back on detaches it and restarts the host, which gives Windows' center its own look back.

It is on whenever Seelen's notification popups are off, on Windows 10 only (Windows 11's center is a different one).

## The top-bar bell

The bell opens and closes Windows' center through the shell's own Win + A handler, like the keyboard shortcut. Use an
inline toolbar item with the `Notifications` scope:

```yaml
badge: "return count > 0 ? count : null"
onClick: "invoke(SeelenCommand.ToggleNativeNotificationCenter)"
```

A click elsewhere closes the open center just before the click lands; a bell click right after that leaves it closed.

## Moving from the Windhawk setup

1. In Windhawk, remove the "Windows 11 Notification Center Styler" and "Seelen notification cards" mods (or quit
   Windhawk), and remove the `windhawk.exe -tray-only` and `CenterPlacer.exe` shortcuts from the Startup folder.
2. Change the bell's `onClick` from `open("seelen-center:toggle")` to the one above, then remove the link:
   `Remove-Item HKCU:\Software\Classes\seelen-center -Recurse`.
3. Sign out and in (or end `ShellExperienceHost.exe` in Task Manager), so the host starts without the old mods.

The old `LocalState\seelen-icons` folder is reused: Seelen replaces its PNG files with the theme's icons.

Known limit: Windows 10 draws a faint one-pixel line just outside the left edge of the center's window (the shell's own
decoration for edge panels, outside the window and its XAML). It runs straight along the card's left side for the card's
full height, so it doesn't follow the rounded corners.
