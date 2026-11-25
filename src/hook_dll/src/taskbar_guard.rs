//! In-process guard for Windows' native taskbar (`Shell_TrayWnd` and `Shell_SecondaryTrayWnd`),
//! running inside `explorer.exe`.
//!
//! When Seelen UI hides the native taskbar, Windows keeps it in auto-hide mode so the work area is
//! freed, which still allows Explorer to show the taskbar on bottom-edge hover or window alerts.
//! Subclassing the taskbar windows lets us clear `SWP_SHOWWINDOW` (and enforce `SWP_HIDEWINDOW`)
//! inside `WM_WINDOWPOSCHANGING` before the taskbar ever becomes visible.
//!
//! Safety: the guard tracks Seelen UI's process handle (`PROCESS_SYNCHRONIZE`) and automatically
//! detaches itself on the very next `WM_WINDOWPOSCHANGING` if Seelen exits or crashes, so the
//! native taskbar can never remain stuck hidden.

use std::{cell::RefCell, collections::HashMap, panic::AssertUnwindSafe, sync::OnceLock};

use slu_ipc::taskbar_guard::{ATTACH_MESSAGE, DETACH_MESSAGE};
use windows::{
    Win32::{
        Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, WAIT_TIMEOUT, WPARAM},
        System::{
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
                GetModuleHandleExW,
            },
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CWPSTRUCT, CallNextHookEx, GetClassNameW, GetPropW, IsWindowVisible,
                RegisterWindowMessageW, RemovePropW, SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW,
                SWP_SHOWWINDOW, SetPropW, WINDOWPOS, WM_NCDESTROY, WM_WINDOWPOSCHANGING,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};
use windows_core::Owned;

const SUBCLASS_ID: usize = 0x5EE1_0C02;
const TASKBAR_CLASSES: [&str; 2] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"];
/// Window property holding the detach function of the copy of this DLL attached to the window.
const OWNER_PROP: PCWSTR = w!("SeelenUI.TaskbarGuard.Owner");

thread_local! {
    /// Handle (`PROCESS_SYNCHRONIZE`) to the Seelen UI process that attached each taskbar window.
    static OWNERS: RefCell<HashMap<isize, Owned<HANDLE>>> = RefCell::new(HashMap::new());
}

/// Hook procedure installed by Seelen UI on the native taskbar threads.
///
/// # Safety
/// Called by Windows as a `WH_CALLWNDPROC` hook procedure.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn TaskbarGuardCallWndProc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_CALLWNDPROC, lParam points to a CWPSTRUCT when code >= 0.
        if let Some(msg) = unsafe { (lparam.0 as *const CWPSTRUCT).as_ref() } {
            if msg.message == attach_message() {
                let (hwnd, owner_pid) = (msg.hwnd, msg.wParam.0);
                guard(|| attach(hwnd, owner_pid));
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

/// Runs `f`, never letting a panic unwind into `explorer.exe`.
fn guard<F: FnOnce()>(f: F) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(f));
}

fn is_taskbar_window(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class) };
    usize::try_from(len).is_ok_and(|len| {
        let name = String::from_utf16_lossy(&class[..len]);
        TASKBAR_CLASSES.contains(&name.as_str())
    })
}

fn attach(hwnd: HWND, owner_pid: usize) {
    if !is_taskbar_window(hwnd) {
        return;
    }
    let Ok(pid) = u32::try_from(owner_pid) else {
        return;
    };
    if pid == 0 {
        return;
    }

    pin_this_module();
    take_over(hwnd);

    let Ok(raw_owner) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return;
    };
    let owner = unsafe { Owned::new(raw_owner) };
    if unsafe { WaitForSingleObject(*owner, 0) } != WAIT_TIMEOUT {
        return;
    }

    OWNERS.with_borrow_mut(|owners| {
        owners.insert(hwnd.0 as isize, owner);
    });

    if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
        OWNERS.with_borrow_mut(|owners| {
            owners.remove(&(hwnd.0 as isize));
        });
        return;
    }
    unsafe {
        let _ = SetPropW(hwnd, OWNER_PROP, Some(HANDLE(detach_entry as *mut _)));
    }
}

fn detach(hwnd: HWND) {
    OWNERS.with_borrow_mut(|owners| {
        owners.remove(&(hwnd.0 as isize));
    });
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

/// Keeps this DLL loaded in `explorer.exe` for the rest of its life so the subclass proc never
/// points to unloaded code if Seelen UI exits.
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

fn is_owner_alive(hwnd: HWND) -> bool {
    OWNERS.with_borrow(|owners| {
        let Some(owner) = owners.get(&(hwnd.0 as isize)) else {
            return false;
        };
        unsafe { WaitForSingleObject(**owner, 0) == WAIT_TIMEOUT }
    })
}

fn suppress_show_flags(flags: SET_WINDOW_POS_FLAGS, is_visible: bool) -> SET_WINDOW_POS_FLAGS {
    if flags.contains(SWP_SHOWWINDOW) || is_visible {
        (flags & !SWP_SHOWWINDOW) | SWP_HIDEWINDOW
    } else {
        flags
    }
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
            if !is_owner_alive(hwnd) {
                guard(|| detach(hwnd));
                return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            }
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            // SAFETY: lParam of WM_WINDOWPOSCHANGING points to the WINDOWPOS being applied.
            if let Some(pos) = unsafe { (lparam.0 as *mut WINDOWPOS).as_mut() } {
                let is_visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
                pos.flags = suppress_show_flags(pos.flags, is_visible);
            }
            return result;
        }
        WM_NCDESTROY => {
            guard(|| detach(hwnd));
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

    #[test]
    fn show_flag_is_replaced_with_hide() {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
        let out = suppress_show_flags(flags, false);
        assert!(!out.contains(SWP_SHOWWINDOW));
        assert!(out.contains(SWP_HIDEWINDOW));
        assert!(out.contains(SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE));
    }

    #[test]
    fn already_visible_window_gets_hide_flag() {
        let flags = SWP_NOMOVE | SWP_NOSIZE;
        let out = suppress_show_flags(flags, true);
        assert!(!out.contains(SWP_SHOWWINDOW));
        assert!(out.contains(SWP_HIDEWINDOW));
    }

    #[test]
    fn hidden_window_move_or_resize_is_untouched() {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        let out = suppress_show_flags(flags, false);
        assert_eq!(out, flags);
    }

    #[test]
    fn existing_hide_flag_is_preserved() {
        let flags = SWP_NOMOVE | SWP_HIDEWINDOW;
        let out = suppress_show_flags(flags, true);
        assert!(!out.contains(SWP_SHOWWINDOW));
        assert!(out.contains(SWP_HIDEWINDOW));
        assert!(out.contains(SWP_NOMOVE));
    }
}
