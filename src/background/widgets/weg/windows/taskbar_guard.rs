//! Loads the `taskbar_guard` component of `sluhk.dll` into `explorer.exe` for
//! `Shell_TrayWnd` and `Shell_SecondaryTrayWnd`. See `src/hook_dll/src/taskbar_guard.rs` and
//! `slu_ipc::taskbar_guard` for the in-process subclass and protocol.

use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{LazyLock, mpsc},
};

use slu_ipc::taskbar_guard::{ATTACH_MESSAGE, DETACH_MESSAGE, HOOK_PROC};
use windows::{
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::WindowsAndMessaging::{
            FindWindowExW, HHOOK, HOOKPROC, IsWindow, RegisterWindowMessageW, SMTO_ABORTIFHUNG,
            SendMessageTimeoutW, SetWindowsHookExW, WH_CALLWNDPROC,
        },
    },
    core::{HSTRING, PCSTR, w},
};
use windows_core::Owned;

use crate::{error::Result, utils::constants::SEELEN_COMMON, windows_api::WindowsApi};

const DLL_NAME: &str = "sluhk.dll";
const DLL_FOLDER: &str = "taskbar-guard";
const SEND_TIMEOUT_MS: u32 = 500;

static GUARD_TX: LazyLock<mpsc::Sender<GuardCommand>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("Taskbar Guard".to_string())
        .spawn(move || run_worker(rx))
        .expect("Failed to spawn Taskbar Guard worker");
    tx
});

enum GuardCommand {
    Attach,
    Detach(mpsc::Sender<()>),
}

/// The component loaded in Seelen (for hooking) and the hooked taskbar threads.
struct Injection {
    /// Dropped first: unhooking before the module is freed.
    threads: HashMap<u32, HookedThread>,
    hook_proc: HOOKPROC,
    module: Owned<HMODULE>,
}

impl Injection {
    fn load() -> Result<Self> {
        let path = prepare_dll()?;
        let module = unsafe { Owned::new(LoadLibraryW(&HSTRING::from(path.as_os_str()))?) };
        let name = std::ffi::CString::new(HOOK_PROC).map_err(|_| "invalid hook procedure name")?;
        let proc = unsafe { GetProcAddress(*module, PCSTR(name.as_ptr() as _)) }
            .ok_or("the taskbar guard hook procedure is missing from the DLL")?;
        // SAFETY: the export has the HOOKPROC signature.
        let hook_proc: HOOKPROC = Some(unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
            >(proc)
        });
        log::info!("Taskbar guard component loaded from {}", path.display());
        Ok(Self {
            threads: HashMap::new(),
            hook_proc,
            module,
        })
    }

    /// Hooks the taskbar window's thread if needed. Returns whether the window is newly tracked.
    fn track(&mut self, hwnd: isize) -> Result<bool> {
        let (_pid, thread_id) = WindowsApi::window_thread_process_id(HWND(hwnd as _));
        if thread_id == 0 {
            return Ok(false);
        }
        if let Some(thread) = self.threads.get_mut(&thread_id) {
            return Ok(thread.windows.insert(hwnd));
        }
        let hook = unsafe {
            SetWindowsHookExW(
                WH_CALLWNDPROC,
                self.hook_proc,
                Some((*self.module).into()),
                thread_id,
            )?
        };
        self.threads.insert(
            thread_id,
            HookedThread {
                _hook: unsafe { Owned::new(hook) },
                windows: HashSet::from([hwnd]),
            },
        );
        Ok(true)
    }

    fn forget(&mut self, hwnd: isize) {
        self.threads.retain(|_, thread| {
            thread.windows.remove(&hwnd);
            !thread.windows.is_empty()
        });
    }

    fn prune_dead(&mut self) {
        self.threads.retain(|_, thread| {
            thread
                .windows
                .retain(|hwnd| unsafe { IsWindow(Some(HWND(*hwnd as _))) }.as_bool());
            !thread.windows.is_empty()
        });
    }

    fn windows(&self) -> Vec<isize> {
        self.threads
            .values()
            .flat_map(|thread| thread.windows.iter().copied())
            .collect()
    }
}

struct HookedThread {
    _hook: Owned<HHOOK>,
    windows: HashSet<isize>,
}

/// Registered messages shared with `sluhk.dll`.
struct Messages {
    attach: u32,
    detach: u32,
}

impl Messages {
    fn get() -> &'static Self {
        static MESSAGES: LazyLock<Messages> = LazyLock::new(|| unsafe {
            Messages {
                attach: RegisterWindowMessageW(&HSTRING::from(ATTACH_MESSAGE)),
                detach: RegisterWindowMessageW(&HSTRING::from(DETACH_MESSAGE)),
            }
        });
        &MESSAGES
    }
}

/// Attaches the in-process taskbar guard to all native taskbar windows asynchronously.
pub fn attach_taskbar_guard() {
    let _ = GUARD_TX.send(GuardCommand::Attach);
}

/// Detaches the in-process taskbar guard from all native taskbar windows synchronously.
pub fn detach_taskbar_guard() {
    let (tx, rx) = mpsc::channel();
    if GUARD_TX.send(GuardCommand::Detach(tx)).is_ok() {
        let _ = rx.recv();
    }
}

fn run_worker(rx: mpsc::Receiver<GuardCommand>) {
    let mut injection: Option<Injection> = None;
    let mut load_logged_err = false;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            GuardCommand::Attach => {
                // Coalesce back-to-back attach requests; stop if a detach is queued next.
                attach_all(&mut injection, &mut load_logged_err);
            }
            GuardCommand::Detach(done) => {
                detach_all(&mut injection);
                load_logged_err = false;
                let _ = done.send(());
            }
        }
    }
}

fn attach_all(injection: &mut Option<Injection>, load_logged_err: &mut bool) {
    let handles = taskbar_windows();
    if handles.is_empty() {
        return;
    }

    if injection.is_none() {
        match Injection::load() {
            Ok(loaded) => *injection = Some(loaded),
            Err(err) => {
                if !*load_logged_err {
                    *load_logged_err = true;
                    log::warn!("Taskbar guard unavailable: {err}");
                }
                return;
            }
        }
    }

    let Some(inj) = injection.as_mut() else {
        return;
    };
    inj.prune_dead();

    let pid = std::process::id() as usize;
    let msg = Messages::get().attach;
    for hwnd in handles {
        match inj.track(hwnd) {
            Ok(true) => {
                if !send(hwnd, msg, pid) {
                    log::warn!(
                        "Taskbar window 0x{hwnd:x} did not acknowledge taskbar guard attach"
                    );
                    inj.forget(hwnd);
                }
            }
            Ok(false) => {}
            Err(err) => {
                log::warn!("Failed to hook taskbar window 0x{hwnd:x}: {err}");
            }
        }
    }
}

fn detach_all(injection: &mut Option<Injection>) {
    let mut targets: HashSet<isize> = taskbar_windows().into_iter().collect();
    if let Some(inj) = injection.as_ref() {
        targets.extend(inj.windows());
    }
    let msg = Messages::get().detach;
    for hwnd in targets {
        send(hwnd, msg, 0);
    }
    // Unhook and free after sending detach so the hook handles the detach message first.
    *injection = None;
}

fn taskbar_windows() -> Vec<isize> {
    let mut found = Vec::new();
    for class in [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
        let mut after: Option<HWND> = None;
        while let Ok(hwnd) = unsafe { FindWindowExW(None, after, class, None) } {
            if hwnd.is_invalid() {
                break;
            }
            found.push(hwnd.0 as isize);
            after = Some(hwnd);
        }
    }
    found
}

fn send(hwnd: isize, message: u32, wparam: usize) -> bool {
    let taken = unsafe {
        SendMessageTimeoutW(
            HWND(hwnd as _),
            message,
            WPARAM(wparam),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            None,
        )
    };
    taken.0 != 0
}

fn prepare_dll() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let source = exe
        .parent()
        .ok_or("Failed to get executable directory")?
        .join(DLL_NAME);
    if !source.exists() {
        return Err(format!("DLL not found at {}", source.display()).into());
    }
    let content = std::fs::read(&source)?;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let file_name = format!("sluhk-{:016x}.dll", hasher.finish());

    let dir = SEELEN_COMMON.app_temp_dir().join(DLL_FOLDER);
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(&file_name);
    if !target.exists() {
        std::fs::write(&target, content)?;
    }
    remove_old_copies(&dir, &file_name);
    Ok(target)
}

fn remove_old_copies(dir: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name != keep && name.starts_with("sluhk-") && name.ends_with(".dll") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
