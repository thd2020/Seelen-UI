# glass-bars theme

The Seelen theme this fork's owner runs: a translucent top bar and dock, and a monochrome line-icon set for small icons.
Large icons (the dock, notification pictures) stay each app's own; the line set is for small ones, such as the top-bar
tray and the 16-pixel app icons on notifications.

- `styles/`: the theme's styles. `fancy-toolbar.scss` embeds each tray icon as a data URL.
- `tray-icons/build.py`: draws the line icons (24-unit grid, 2.4 stroke, round caps) and writes the SVG files and
  `apps.json`, the map from each program to its icon and tray tooltips.
- With notifications left to Windows, Seelen copies these icons (as listed in `apps.json`) to Windows' notification
  center while the theme is active.

Install by copying this folder to `%APPDATA%\com.seelen.seelen-ui\themes\glass-bars` and enabling it in Seelen's
settings.
