//! Size, place and shape of the notification host's windows.
//!
//! - The notification center: the shell opens it at the monitor's full height on the right edge.
//!   Its window becomes the card instead, under the top bar, before the size is applied: the shell
//!   draws the window's frosted backdrop and edge line to the window's size, so they then match
//!   the card too, and the window is never resized while it opens (a resize during the shell's
//!   open animation draws the content stretched, then snaps it).
//! - Banners: Windows puts them in the bottom-right corner. They move under the top bar, cut to
//!   their card's rounded shape.
//!
//! Window shapes (regions) have no anti-aliasing, so each rounded corner keeps only the pixels the
//! card's corner fully covers: the visible edge is then the card's own solid pixels.

use std::{cell::RefCell, collections::HashMap};

use slu_ipc::notification_center::{CARD_GAP, CARD_HEIGHT, CARD_WIDTH};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::{
        Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute},
        Gdi::{
            CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, GetWindowRgnBox, HMONITOR,
            HRGN, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, MonitorFromWindow,
            RGN_DIFF, RGN_ERROR, SetWindowRgn,
        },
    },
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GetClassNameW, GetWindowRect, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SetWindowPos, WINDOWPOS,
        },
    },
};
use windows::core::BOOL;

const CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// Corner radius of the center's card. Values in this file are at 96 DPI.
const CARD_RADIUS: i32 = 12;

/// A banner's card sits this far in from its window's sides, top and bottom: the rest of the
/// window is the shadow Windows draws around it.
const BANNER_SIDE: i32 = 16;
const BANNER_TOP: i32 = 71;
const BANNER_BOTTOM: i32 = 11;
const BANNER_RADIUS: i32 = 14;
/// Windows places banners with their bottom within this distance of the work area's bottom.
const BANNER_BOTTOM_ZONE: i32 = 80;

thread_local! {
    /// What each window of this thread was recognised as.
    static KINDS: RefCell<HashMap<isize, Kind>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Center,
    /// A banner, with the window size its shape was cut for.
    Banner {
        shaped: (i32, i32),
    },
}

pub fn is_core_window(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class) };
    usize::try_from(len)
        .is_ok_and(|len| String::from_utf16_lossy(&class[..len]) == CORE_WINDOW_CLASS)
}

pub fn on_attach(hwnd: HWND) {
    ensure_card(hwnd);
}

pub fn on_detach(hwnd: HWND) {
    KINDS.with_borrow_mut(|kinds| kinds.remove(&(hwnd.0 as isize)));
}

/// Adjusts the size and place the window is about to get.
pub fn on_pos_changing(hwnd: HWND, pos: &mut WINDOWPOS) {
    let current = window_rect(hwnd);
    let size_given = !pos.flags.contains(SWP_NOSIZE);
    let (x, y) = if pos.flags.contains(SWP_NOMOVE) {
        (current.left, current.top)
    } else {
        (pos.x, pos.y)
    };
    let (w, h) = if size_given {
        (pos.cx, pos.cy)
    } else {
        (current.right - current.left, current.bottom - current.top)
    };
    let proposed = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let Some(screen) = Screen::for_rect(&proposed) else {
        return;
    };

    // The center opening at full height: make the window the card, under the top bar.
    if size_given && pos.cy > 0 && pos.cy >= screen.monitor_height() * 6 / 10 {
        disable_transitions(hwnd);
        let card = screen.card_rect();
        pos.x = card.left;
        pos.y = card.top;
        // A zero width is the shell collapsing the window: keep that.
        if pos.cx != 0 {
            pos.cx = card.right - card.left;
        }
        pos.cy = card.bottom - card.top;
        pos.flags &= !SWP_NOMOVE;
        set_kind(hwnd, Kind::Center);
        return;
    }

    if kind(hwnd) == Some(Kind::Center) || h >= screen.monitor_height() / 2 {
        return;
    }
    // A banner in the bottom-right corner: same right edge, its card's top under the top bar.
    let at_right = proposed.right >= screen.work.right - 2;
    let at_bottom = proposed.bottom >= screen.work.bottom - screen.scale(BANNER_BOTTOM_ZONE);
    if at_right && at_bottom {
        pos.x = proposed.left;
        pos.y = screen.banner_top();
        pos.flags &= !SWP_NOMOVE;
        if kind(hwnd).is_none() {
            set_kind(hwnd, Kind::Banner { shaped: (0, 0) });
        }
    }
}

/// Cuts the window to its card once its new size is applied.
pub fn on_pos_changed(hwnd: HWND) {
    match kind(hwnd) {
        Some(Kind::Banner { shaped }) => {
            let r = window_rect(hwnd);
            let size = (r.right - r.left, r.bottom - r.top);
            if size == shaped || size.0 <= 0 || size.1 <= 0 {
                return;
            }
            let Some(screen) = Screen::for_window(hwnd) else {
                return;
            };
            let (side, top, bottom) = (
                screen.scale(BANNER_SIDE),
                screen.scale(BANNER_TOP),
                screen.scale(BANNER_BOTTOM),
            );
            let region = card_region(
                side,
                top,
                size.0 - 2 * side,
                size.1 - top - bottom,
                screen.scale(BANNER_RADIUS),
            );
            unsafe { SetWindowRgn(hwnd, Some(region), true) };
            set_kind(hwnd, Kind::Banner { shaped: size });
        }
        _ => ensure_card(hwnd),
    }
}

/// Makes the center's window the card if it is at the shell's full height, and gives a card-sized
/// window the card's shape if it has none. The shell keeps the window's size from one opening to
/// the next, so this also runs when the component attaches, not only on resizes.
fn ensure_card(hwnd: HWND) {
    let Some(screen) = Screen::for_window(hwnd) else {
        return;
    };
    let r = window_rect(hwnd);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let card = screen.card_rect();
    let (card_w, card_h) = (card.right - card.left, card.bottom - card.top);

    if h >= screen.monitor_height() * 6 / 10 {
        disable_transitions(hwnd);
        set_kind(hwnd, Kind::Center);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                card.left,
                card.top,
                card_w,
                card_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        // the resize above already shaped it, through `on_pos_changed`
        return;
    }

    if (w, h) == (card_w, card_h) {
        disable_transitions(hwnd);
        set_kind(hwnd, Kind::Center);
        let mut bounds = RECT::default();
        let shaped = unsafe { GetWindowRgnBox(hwnd, &mut bounds) } != RGN_ERROR;
        let expected = RECT {
            left: 0,
            top: 0,
            right: card_w,
            bottom: card_h,
        };
        if !shaped || bounds != expected {
            let region = card_region(0, 0, card_w, card_h, screen.scale(CARD_RADIUS));
            unsafe { SetWindowRgn(hwnd, Some(region), true) };
        }
    }
}

/// Turns off DWM's transitions (the open/close zoom and fade) for the window.
fn disable_transitions(hwnd: HWND) {
    let disabled = BOOL::from(true);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disabled as *const BOOL as *const _,
            size_of::<BOOL>() as u32,
        );
    }
}

fn kind(hwnd: HWND) -> Option<Kind> {
    KINDS.with_borrow(|kinds| kinds.get(&(hwnd.0 as isize)).copied())
}

fn set_kind(hwnd: HWND, kind: Kind) {
    KINDS.with_borrow_mut(|kinds| kinds.insert(hwnd.0 as isize, kind));
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    rect
}

/// The window shape of a card at (x, y) of size w × h with corners of radius r, built row by row
/// so all four corners are exact mirrors (`CreateRoundRectRgn` is off by one on two sides).
fn card_region(x: i32, y: i32, w: i32, h: i32, r: i32) -> HRGN {
    unsafe {
        let region = CreateRectRgn(x, y, x + w, y + h);
        for row in 0..r {
            // distance from the corner's centre to the row's outer edge
            let dy = (r - row) as f64;
            let inside = ((r * r) as f64 - dy * dy).max(0.0).sqrt();
            let cut = (r as f64 - inside).ceil() as i32;
            if cut <= 0 {
                continue;
            }
            for yy in [y + row, y + h - 1 - row] {
                let left = CreateRectRgn(x, yy, x + cut, yy + 1);
                let right = CreateRectRgn(x + w - cut, yy, x + w, yy + 1);
                CombineRgn(Some(region), Some(region), Some(left), RGN_DIFF);
                CombineRgn(Some(region), Some(region), Some(right), RGN_DIFF);
                let _ = DeleteObject(left.into());
                let _ = DeleteObject(right.into());
            }
        }
        region
    }
}

/// A monitor's areas and scale.
struct Screen {
    monitor: RECT,
    work: RECT,
    dpi: u32,
}

impl Screen {
    fn for_rect(rect: &RECT) -> Option<Self> {
        Self::new(unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST) })
    }

    fn for_window(hwnd: HWND) -> Option<Self> {
        Self::new(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })
    }

    fn new(monitor: HMONITOR) -> Option<Self> {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }
        let (mut dpi, mut _dpi_y) = (96, 96);
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y) }.is_err() {
            dpi = 96;
        }
        Some(Self {
            monitor: info.rcMonitor,
            work: info.rcWork,
            dpi,
        })
    }

    fn scale(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }

    fn monitor_height(&self) -> i32 {
        self.monitor.bottom - self.monitor.top
    }

    /// The center's card: at the right of the work area, under the top bar.
    fn card_rect(&self) -> RECT {
        let (w, h, gap) = (
            self.scale(CARD_WIDTH),
            self.scale(CARD_HEIGHT),
            self.scale(CARD_GAP),
        );
        let left = self.work.right - gap - w;
        let top = self.work.top + gap;
        RECT {
            left,
            top,
            right: left + w,
            bottom: top + h,
        }
    }

    /// Top of a banner's window that puts its card's top level with the center's.
    fn banner_top(&self) -> i32 {
        self.work.top + self.scale(CARD_GAP) - self.scale(BANNER_TOP)
    }
}
