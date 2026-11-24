//! Notification center component, running inside Windows 10's notification host
//! (`ShellExperienceHost.exe`).
//!
//! Seelen UI loads this DLL into the host with a `WH_CALLWNDPROC` hook on the threads of the
//! host's windows, and sends each window the attach message (see `slu_ipc::notification_center`).
//! From then on the component runs on the window's own UI thread:
//!
//! - `window`: sizes the notification center's window to a card under the top bar and moves the
//!   banners there too, by adjusting `WM_WINDOWPOSCHANGING` before the shell's size is applied,
//!   so the window never shows at another size and is never stretched while opening.
//! - `style`: restyles the center's and banners' XAML as frosted cards.
//! - `cards`: one card per notification, with its app's icon and name on top.
//!
//! The DLL pins itself in the host: Windows unloads hook DLLs when the hook goes away (Seelen
//! exits or crashes), which would leave the window subclass pointing to unloaded code. Pinned, it
//! keeps working until the host restarts. A newer copy (another build of Seelen) takes over the
//! window from the older one through the `OWNER_PROP` window property.

mod cards;
mod icons;
mod style;
mod window;

#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    unused_imports,
    clippy::all
)]
mod xaml {
    include!(concat!(env!("OUT_DIR"), "/xaml.rs"));
}

use std::{
    cell::{Cell, RefCell},
    panic::AssertUnwindSafe,
    sync::OnceLock,
    time::{Duration, Instant},
};

use slu_ipc::notification_center::{ATTACH_MESSAGE, DETACH_MESSAGE};
use windows::{
    Foundation::EventHandler,
    Win32::{
        Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CWPSTRUCT, CallNextHookEx, GetPropW, IsWindow, KillTimer, RegisterWindowMessageW,
                RemovePropW, SetPropW, SetTimer, WINDOWPOS, WM_NCDESTROY, WM_TIMER,
                WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
            },
        },
    },
    core::{HSTRING, IInspectable, PCWSTR, Ref, w},
};

use self::xaml::{FrameworkElement, UIElement, Window};

const SUBCLASS_ID: usize = 0x5EE1_0C01;
/// Re-checks the XAML regularly, which also covers list containers recycled for other
/// notifications without a layout pass.
const TICK_TIMER_ID: usize = 0x5EE1_0C01;
const TICK_INTERVAL_MS: u32 = 500;
/// One-shot pass shortly after a layout pass that came too soon after the previous one.
const SOON_TIMER_ID: usize = 0x5EE1_0C02;
const SOON_DELAY_MS: u32 = 40;
/// Layout passes come in bursts while the center animates or scrolls; the XAML is walked at most
/// this often from them.
const MIN_PASS_INTERVAL: Duration = Duration::from_millis(40);
/// Window property holding the detach function of the copy of this DLL attached to the window.
const OWNER_PROP: PCWSTR = w!("SeelenUI.NotificationCenter.Owner");

thread_local! {
    /// The window of this thread that drives the XAML passes (timers and layout handler), the
    /// first one attached. Other windows of the thread only get the window subclass.
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    static LAST_PASS: Cell<Option<Instant>> = const { Cell::new(None) };
}

struct Driver {
    hwnd: HWND,
    /// Root element with the `LayoutUpdated` handler, once the XAML content exists.
    layout_root: Option<(FrameworkElement, i64)>,
}

/// Hook procedure installed by Seelen UI on the notification host's UI threads.
///
/// # Safety
/// Called by Windows as a `WH_CALLWNDPROC` hook procedure.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn NotificationCenterCallWndProc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_CALLWNDPROC, lParam points to a CWPSTRUCT when code >= 0.
        if let Some(msg) = unsafe { (lparam.0 as *const CWPSTRUCT).as_ref() } {
            if msg.message == attach_message() {
                let (hwnd, generation) = (msg.hwnd, msg.wParam.0);
                guard(|| attach(hwnd, generation));
            } else if msg.message == detach_message() {
                let hwnd = msg.hwnd;
                guard(|| detach(hwnd));
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn attach_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(&HSTRING::from(ATTACH_MESSAGE)) })
}

fn detach_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(&HSTRING::from(DETACH_MESSAGE)) })
}

/// Runs `f`, never letting a panic unwind into the host (it would abort the host).
fn guard<F: FnOnce()>(f: F) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(f));
}

/// Attaches the component to `hwnd`. Runs inside a message sent by Seelen from another process,
/// where Windows refuses outgoing COM calls, so the XAML work waits for the timers.
fn attach(hwnd: HWND, icons_generation: usize) {
    if !window::is_core_window(hwnd) {
        return;
    }
    pin_this_module();
    take_over(hwnd);

    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
        let _ = SetPropW(hwnd, OWNER_PROP, Some(HANDLE(detach_entry as *mut _)));
    }
    window::on_attach(hwnd);
    icons::request(icons_generation);

    DRIVER.with_borrow_mut(|driver| {
        let alive = driver
            .as_ref()
            .is_some_and(|d| unsafe { IsWindow(Some(d.hwnd)) }.as_bool());
        if !alive {
            *driver = Some(Driver {
                hwnd,
                layout_root: None,
            });
            unsafe { SetTimer(Some(hwnd), TICK_TIMER_ID, TICK_INTERVAL_MS, None) };
        }
    });
}

/// Detaches the component from `hwnd`. The XAML keeps its current look until the host restarts.
fn detach(hwnd: HWND) {
    let mut driver_to_release = None;
    DRIVER.with_borrow_mut(|driver| {
        if driver.as_ref().is_some_and(|d| d.hwnd == hwnd) {
            driver_to_release = driver.take();
        }
    });
    if let Some(driver) = driver_to_release {
        unsafe {
            let _ = KillTimer(Some(hwnd), TICK_TIMER_ID);
            let _ = KillTimer(Some(hwnd), SOON_TIMER_ID);
        }
        if let Some((root, token)) = driver.layout_root {
            let _ = root.RemoveLayoutUpdated(token);
        }
    }
    window::on_detach(hwnd);
    unsafe {
        let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
        let _ = RemovePropW(hwnd, OWNER_PROP);
    }
}

/// Detach entry point stored in `OWNER_PROP`, called by newer copies of this DLL.
extern "system" fn detach_entry(hwnd: HWND) {
    guard(|| detach(hwnd));
}

/// Detaches an older copy of this DLL (from another Seelen build) attached to `hwnd`.
fn take_over(hwnd: HWND) {
    let owner = unsafe { GetPropW(hwnd, OWNER_PROP) };
    let ours = detach_entry as *mut core::ffi::c_void;
    if !owner.is_invalid() && owner.0 != ours {
        // SAFETY: the property is only ever set by a copy of this DLL to its `detach_entry`, and
        // copies pin themselves, so the function is still loaded.
        let theirs: extern "system" fn(HWND) = unsafe { std::mem::transmute(owner.0) };
        theirs(hwnd);
    }
}

/// Keeps this DLL loaded in the host for the rest of the host's life (see the module docs).
fn pin_this_module() {
    static PINNED: OnceLock<()> = OnceLock::new();
    PINNED.get_or_init(|| unsafe {
        let mut module = HMODULE::default();
        let _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            PCWSTR(pin_this_module as *const u16),
            &mut module,
        );
    });
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_WINDOWPOSCHANGING => {
            // The shell sets the window's size and place in its own handler: adjust after it,
            // right before Windows applies the change.
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            // SAFETY: lParam of WM_WINDOWPOSCHANGING points to the WINDOWPOS being applied.
            if let Some(pos) = unsafe { (lparam.0 as *mut WINDOWPOS).as_mut() } {
                guard(|| window::on_pos_changing(hwnd, pos));
            }
            return result;
        }
        WM_WINDOWPOSCHANGED => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            guard(|| window::on_pos_changed(hwnd));
            return result;
        }
        WM_TIMER if wparam.0 == TICK_TIMER_ID => {
            guard(|| pass(hwnd));
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == SOON_TIMER_ID => {
            unsafe {
                let _ = KillTimer(Some(hwnd), SOON_TIMER_ID);
            }
            guard(|| pass(hwnd));
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            guard(|| detach(hwnd));
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// One pass over this thread's XAML: restyles it and updates the notification cards.
fn pass(driver_hwnd: HWND) {
    LAST_PASS.set(Some(Instant::now()));
    let Ok(content) = Window::Current().and_then(|w| w.Content()) else {
        return;
    };
    watch_layout(driver_hwnd, &content);
    let mut walker = cards::CardsPass::default();
    style::walk(&content, &mut |element, chain| {
        style::apply(element, chain);
        walker.visit(element, chain);
    });
}

/// Walks the XAML again after each layout pass, so new cards and banners are styled before the
/// frame that shows them is drawn, instead of up to a tick later.
fn watch_layout(driver_hwnd: HWND, content: &UIElement) {
    let needs_handler = DRIVER.with_borrow(|driver| {
        driver
            .as_ref()
            .is_some_and(|d| d.hwnd == driver_hwnd && d.layout_root.is_none())
    });
    if !needs_handler {
        return;
    }
    let Ok(root) = windows::core::Interface::cast::<FrameworkElement>(content) else {
        return;
    };
    // Handlers must be `Send`: carry the window handle as a plain number.
    let driver_address = driver_hwnd.0 as isize;
    let handler =
        EventHandler::<IInspectable>::new(move |_: Ref<IInspectable>, _: Ref<IInspectable>| {
            guard(|| on_layout_updated(HWND(driver_address as *mut _)));
            Ok(())
        });
    if let Ok(token) = root.LayoutUpdated(&handler) {
        DRIVER.with_borrow_mut(|driver| {
            if let Some(d) = driver.as_mut().filter(|d| d.hwnd == driver_hwnd) {
                d.layout_root = Some((root, token));
            }
        });
    }
}

fn on_layout_updated(driver_hwnd: HWND) {
    let recent = LAST_PASS
        .get()
        .is_some_and(|last| last.elapsed() < MIN_PASS_INTERVAL);
    if recent {
        // A pass changes the layout itself: don't loop on it, look again shortly instead.
        unsafe { SetTimer(Some(driver_hwnd), SOON_TIMER_ID, SOON_DELAY_MS, None) };
        return;
    }
    pass(driver_hwnd);
}
