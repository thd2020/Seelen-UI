use std::{
    collections::{HashMap, VecDeque},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetForegroundWindow,
        GetMessageW, IsWindowVisible, MSG, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNORMAL,
        TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_ENDSESSION,
        WM_QUERYENDSESSION, WNDCLASSW,
    },
};

use crate::{
    error::Result,
    string_utils::WindowsString,
    windows_api::{
        WindowsApi,
        app_bar::{AppBarData, AppBarDataState},
        iterator::WindowEnumerator,
    },
};

/// A taskbar that had to be hidden this many times within [`STUCK_WINDOW`] is considered stuck.
/// Explorer re-shows a stuck taskbar about every 400 ms, so a stuck one reaches this in ~2 s.
const STUCK_HIDES: usize = 5;
const STUCK_WINDOW: Duration = Duration::from_secs(3);
/// Hides closer together than this belong to the same show (the hide is async).
const MIN_HIDE_GAP: Duration = Duration::from_millis(150);
/// The recovery briefly takes the foreground, so never run it more often than this per taskbar.
const RECOVERY_COOLDOWN: Duration = Duration::from_secs(10);
/// How long the taskbar keeps the foreground during the recovery.
const RECOVERY_ACTIVE_TIME: Duration = Duration::from_millis(300);

/// Tracks whether we actually hid the native taskbar, so we only restore it
/// when it was hidden by us (avoids unnecessary restores on settings changes/shutdown).
static NATIVE_TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Per taskbar handle, how often it had to be hidden again.
static RESHOW_TRACKERS: LazyLock<Mutex<HashMap<isize, ReshowTracker>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Detects an auto-hide taskbar that explorer keeps re-showing.
///
/// When explorer reveals an auto-hide taskbar without activating it, it only hides it again
/// by sliding it away once the reveal is over. We hide the window with `SW_HIDE` before that
/// happens, so explorer can get stuck in its "revealed" state and show the taskbar again on
/// every auto-hide timer tick, which flickers behind Seelen UI forever.
#[derive(Debug, Default)]
struct ReshowTracker {
    hides: VecDeque<Instant>,
    last_recovery: Option<Instant>,
}

impl ReshowTracker {
    /// Records that the taskbar had to be hidden again. Returns true when it is stuck and a
    /// recovery is due.
    fn record_hide(&mut self, now: Instant) -> bool {
        if self
            .hides
            .back()
            .is_some_and(|last| now.duration_since(*last) < MIN_HIDE_GAP)
        {
            return false;
        }
        self.hides.push_back(now);
        while self
            .hides
            .front()
            .is_some_and(|first| now.duration_since(*first) > STUCK_WINDOW)
        {
            self.hides.pop_front();
        }

        if self.hides.len() < STUCK_HIDES
            || self
                .last_recovery
                .is_some_and(|last| now.duration_since(last) < RECOVERY_COOLDOWN)
        {
            return false;
        }
        self.hides.clear();
        self.last_recovery = Some(now);
        true
    }
}

pub fn get_taskbars_handles() -> Result<Vec<HWND>> {
    let mut founds = Vec::new();
    WindowEnumerator::new().for_each(|hwnd| {
        let class = WindowsApi::get_class(hwnd);
        if (class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd")
            && WindowsApi::get_title(hwnd).is_empty()
        {
            founds.push(hwnd);
        }
    })?;
    Ok(founds)
}

/// Only one hide pass runs at a time; requests that arrive during a pass are covered by it.
static HIDE_PASS_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn hide_native_taskbar() {
    NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Release);
    if HIDE_PASS_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| {
        let mut stuck = Vec::new();
        match get_taskbars_handles() {
            Ok(handles) => {
                for attempt in 0..10 {
                    if attempt > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    if !NATIVE_TASKBAR_HIDDEN.load(Ordering::Acquire) {
                        continue;
                    }
                    for hwnd in &handles {
                        let app_bar = AppBarData::from_handle(*hwnd);
                        // ABM_SETSTATE makes explorer re-run its auto-hide logic and show the
                        // taskbar again, which fires a new show event and so a new hide request.
                        // On Windows 10 21H2 that code path can also crash explorer, so only
                        // send it when the state really has to change.
                        if !app_bar.is_auto_hide() {
                            app_bar.set_state(AppBarDataState::AutoHide);
                        }
                        if unsafe { IsWindowVisible(*hwnd) }.as_bool() {
                            let _ = WindowsApi::show_window_async(hwnd.0 as isize, SW_HIDE.0);
                            if record_hide(*hwnd) && !stuck.contains(hwnd) {
                                stuck.push(*hwnd);
                            }
                        }
                    }
                }
            }
            Err(err) => log::error!("Failed to get taskbars handles: {err:?}"),
        }
        HIDE_PASS_RUNNING.store(false, Ordering::Release);

        // after releasing the pass, so the taskbar keeps being hidden while it is activated
        for hwnd in stuck {
            if NATIVE_TASKBAR_HIDDEN.load(Ordering::Acquire) {
                finish_auto_hide(hwnd);
            }
        }
    });
}

/// Returns true when `hwnd` is stuck being re-shown by explorer and should be recovered.
fn record_hide(hwnd: HWND) -> bool {
    let mut trackers = RESHOW_TRACKERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    trackers
        .entry(hwnd.0 as isize)
        .or_default()
        .record_hide(Instant::now())
}

/// Makes explorer leave the "revealed" state of a stuck auto-hide taskbar.
///
/// Explorer always finishes the auto-hide cycle when the taskbar loses activation, so this
/// activates the taskbar for a moment and then gives the foreground back to the window that
/// had it (or the desktop).
fn finish_auto_hide(taskbar: HWND) {
    log::warn!(
        "Native taskbar {taskbar:?} keeps being re-shown by explorer, resetting its auto-hide"
    );
    let previous = unsafe { GetForegroundWindow() };
    if let Err(err) = WindowsApi::set_foreground(taskbar.0 as isize) {
        log::error!("Failed to activate the native taskbar: {err:?}");
        return;
    }
    std::thread::sleep(RECOVERY_ACTIVE_TIME);

    let back = if previous.is_invalid() || previous == taskbar {
        let progman = WindowsString::from_str("Progman");
        unsafe { FindWindowW(progman.as_pcwstr(), None) }.unwrap_or_default()
    } else {
        previous
    };
    if back.is_invalid() {
        return;
    }
    if let Err(err) = WindowsApi::set_foreground(back.0 as isize) {
        log::error!("Failed to give the foreground back after resetting the taskbar: {err:?}");
    }
}

pub fn restore_native_taskbar() -> Result<()> {
    if NATIVE_TASKBAR_HIDDEN
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    for hwnd in get_taskbars_handles()? {
        AppBarData::from_handle(hwnd).set_state(AppBarDataState::AlwaysOnTop);
        WindowsApi::show_window_async(hwnd.0 as isize, SW_SHOWNORMAL.0)?;
    }
    Ok(())
}

unsafe extern "system" fn shutdown_window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            // sent by the OS when it wants to shut down/log off/quiesce the process
            // (system shutdown, user logoff, or a package update needing us to exit).
            // Reacting here lets us call `exit()` promptly instead of idling in the
            // exit channel wait until the OS times out and force-kills us.
            WM_QUERYENDSESSION | WM_ENDSESSION => {
                log::info!("Received shutdown/quiesce request (msg={msg}), exiting service");
                crate::exit(0);
            }
            WM_DESTROY => PostQuitMessage(0),
            _ => {}
        }
        DefWindowProcW(hwnd, msg, w_param, l_param)
    }
}

/// will lock until the window is closed
unsafe fn create_shutdown_window(done: &crossbeam_channel::Sender<()>) -> Result<()> {
    unsafe {
        let title = WindowsString::from_str("Seelen UI Service Shutdown Window");
        let class = WindowsString::from_str("SeelenServiceShutdownWindow");

        let h_module = GetModuleHandleW(None)?;

        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(shutdown_window_proc),
            hInstance: h_module.into(),
            lpszClassName: class.as_pcwstr(),
            ..Default::default()
        };

        RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.as_pcwstr(),
            title.as_pcwstr(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(wnd_class.hInstance),
            None,
        )?;

        done.send(())?;
        let mut msg = MSG::default();

        // GetMessageW will run until PostQuitMessage(0) is called
        while GetMessageW(&mut msg, Some(hwnd), 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

/// Spawns a hidden top-level window on its own thread whose only purpose is receiving
/// `WM_QUERYENDSESSION`/`WM_ENDSESSION`, so the service can gracefully exit when the OS
/// tries to quiesce it (shutdown, logoff, package update) instead of being force-killed
/// while idling and reported as a hang.
pub fn start_shutdown_listener() -> Result<()> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("Shutdown Window".to_string())
        .spawn(move || {
            if let Err(err) = unsafe { create_shutdown_window(&tx) } {
                log::error!("Shutdown window thread failed: {err:?}");
            }
        })?;
    rx.recv()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESHOW_PERIOD: Duration = Duration::from_millis(400);

    #[test]
    fn occasional_shows_are_not_stuck() {
        let mut tracker = ReshowTracker::default();
        let start = Instant::now();
        for i in 0..20 {
            assert!(!tracker.record_hide(start + Duration::from_secs(i)));
        }
    }

    #[test]
    fn repeated_hides_of_the_same_show_count_once() {
        // the async hide can take a couple of 50 ms attempts to land for one show
        let mut tracker = ReshowTracker::default();
        let start = Instant::now();
        for show in 0..10u64 {
            for attempt in 0..3u64 {
                let at = start + Duration::from_millis(show * 1000 + attempt * 50);
                assert!(!tracker.record_hide(at));
            }
        }
    }

    #[test]
    fn explorer_reshow_loop_is_stuck_and_recovery_has_cooldown() {
        let mut tracker = ReshowTracker::default();
        let start = Instant::now();
        let at = |n: u32| start + RESHOW_PERIOD * n;

        for n in 0..(STUCK_HIDES as u32 - 1) {
            assert!(!tracker.record_hide(at(n)));
        }
        assert!(tracker.record_hide(at(STUCK_HIDES as u32 - 1)));

        // still looping right after the recovery: wait for the cooldown
        let cooldown_hides = (RECOVERY_COOLDOWN.as_millis() / RESHOW_PERIOD.as_millis()) as u32;
        let mut recovered_again = None;
        for n in STUCK_HIDES as u32..STUCK_HIDES as u32 + cooldown_hides + STUCK_HIDES as u32 {
            if tracker.record_hide(at(n)) {
                recovered_again = Some(n);
                break;
            }
        }
        let n = recovered_again.expect("a still stuck taskbar must be recovered again");
        assert!(RESHOW_PERIOD * (n - (STUCK_HIDES as u32 - 1)) >= RECOVERY_COOLDOWN);
    }
}
