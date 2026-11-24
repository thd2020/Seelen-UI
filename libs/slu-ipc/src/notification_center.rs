//! What Seelen UI and the notification center component of `sluhk.dll` agree on.
//!
//! On Windows 10, when Seelen leaves notifications to Windows (its own popups are off), Seelen
//! loads `sluhk.dll` into the notification host (`ShellExperienceHost.exe`) with a
//! `WH_CALLWNDPROC` hook on the threads of the host's windows, and then sends each window
//! [`ATTACH_MESSAGE`]. The component, running on the window's own UI thread, restyles Windows'
//! notification center as a card under the top bar and its banners to match.

/// Name of the hook procedure exported by `sluhk.dll` for the notification host.
pub const HOOK_PROC: &str = "NotificationCenterCallWndProc";

/// Registered window message (`RegisterWindowMessageW`) that attaches the component to the
/// window it is sent to. `wParam` is the generation of the theme icons in the host's
/// `LocalState\seelen-icons` folder: a new value makes the component load them again.
pub const ATTACH_MESSAGE: &str = "SeelenUI.NotificationCenter.Attach";

/// Registered window message that detaches the component from the window it is sent to.
pub const DETACH_MESSAGE: &str = "SeelenUI.NotificationCenter.Detach";

/// Folder, in the notification host's `LocalState`, with the theme icons for the cards and
/// banners. It is the only place the sandboxed host can read. `map.txt` in it maps app names to
/// icon files, one `name=file` per line, names compared case-insensitively.
pub const ICONS_FOLDER: &str = "seelen-icons";

/// Package family name of the notification host.
pub const HOST_PACKAGE_FAMILY: &str = "Microsoft.Windows.ShellExperienceHost_cw5n1h2txyewy";

/// Size of the notification center's card, in effective (96 DPI) pixels: the width is the one
/// Windows gives the center. The window is scaled by the monitor's DPI.
pub const CARD_WIDTH: i32 = 396;
pub const CARD_HEIGHT: i32 = 560;
/// Space between the card and the edges of the work area (under the top bar, and the right).
pub const CARD_GAP: i32 = 8;
