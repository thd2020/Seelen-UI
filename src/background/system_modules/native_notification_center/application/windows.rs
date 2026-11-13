//! Windows 10: the notification center component of `sluhk.dll`, loaded into the notification
//! host (`ShellExperienceHost.exe`). See `src/hook_dll/src/notification_center` for what it does
//! there and `slu_ipc::notification_center` for the protocol.
//!
//! - The host runs sandboxed and only reads its own package folders, so the DLL is copied into
//!   the host's `LocalState`, named by its content: the host keeps a loaded copy for the rest of
//!   its life (the component pins itself), and a new Seelen build brings a new file instead of
//!   failing to replace a loaded one.
//! - A `WH_CALLWNDPROC` hook on each thread of the host's windows loads the DLL there, then the
//!   attach message starts the component on each window. New windows are attached as they show.
//! - The center opens and closes through the shell's own Win + A hotkey handler, the way the
//!   keyboard shortcut does, without the launch zoom of the `ms-actioncenter:` link.

use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use slu_ipc::notification_center::{
    ATTACH_MESSAGE, CARD_HEIGHT, DETACH_MESSAGE, HOOK_PROC, HOST_PACKAGE_FAMILY, ICONS_FOLDER,
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EnumThreadWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindowRect, HHOOK,
                HOOKPROC, PostMessageW, RegisterWindowMessageW, SMTO_ABORTIFHUNG,
                SendMessageTimeoutW, SetWindowsHookExW, WH_CALLWNDPROC, WM_HOTKEY,
            },
        },
    },
    core::{BOOL, HSTRING, PCSTR, w},
};
use windows_core::Owned;

use crate::{
    error::{Result, ResultLogExt},
    platform::{
        hook::{HookManager, WindowEvent},
        info::is_windows_10,
        window::{Window, WindowsWindowExt},
    },
    system_modules::native_notification_center::domain::{
        NativeNotificationCenterSystem, ThemeIcons,
    },
    utils::lock_free::TracedMutex,
    windows_api::WindowsApi,
};

const HOST_EXE: &str = "ShellExperienceHost.exe";
const CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";
/// Folder in the host's `LocalState` with the copies of the DLL.
const DLL_FOLDER: &str = "seelen-ui";
const DLL_NAME: &str = "sluhk.dll";
/// How long to wait for the host to take each message.
const SEND_TIMEOUT_MS: u32 = 2000;
/// The shell's id for its Win + A (notification center) hotkey.
const NOTIFICATION_CENTER_HOTKEY: usize = 500;
/// A click outside the open center closes it right before the click lands: a toggle within this
/// time of the center closing is that same click, and leaves it closed.
const JUST_CLOSED: Duration = Duration::from_millis(600);

pub struct WindowsNativeNotificationCenter {
    shared: Arc<Shared>,
}

impl NativeNotificationCenterSystem for WindowsNativeNotificationCenter {
    fn create() -> Self {
        Self {
            shared: Arc::new(Shared {
                enabled: AtomicBool::new(false),
                failed: AtomicBool::new(false),
                injection: TracedMutex::new(None),
                closed_at: TracedMutex::new(None),
                icons_generation: AtomicUsize::new(0),
            }),
        }
    }

    fn init(&mut self) -> Result<()> {
        let shared = self.shared.clone();
        HookManager::subscribe(move |(event, window)| shared.on_window_event(event, window));
        Ok(())
    }

    fn set_enabled(&self, enabled: bool) -> Result<()> {
        let enabled = enabled && is_windows_10();
        if self.shared.enabled.swap(enabled, Ordering::SeqCst) == enabled {
            return Ok(());
        }
        self.shared.failed.store(false, Ordering::SeqCst);
        if enabled {
            log::info!("Restyling Windows' notification center");
            for window in host_windows() {
                self.shared.attach(window);
            }
        } else {
            log::info!("Giving Windows' notification center its own look back");
            self.shared.detach_all();
        }
        Ok(())
    }

    fn set_icons(&self, icons: &ThemeIcons) -> Result<()> {
        if !is_windows_10() {
            return Ok(());
        }
        let map = icons.map_text();
        let mut hasher = DefaultHasher::new();
        map.hash(&mut hasher);
        for file in &icons.files {
            std::fs::metadata(&file.source)
                .and_then(|meta| meta.modified())
                .ok()
                .hash(&mut hasher);
        }
        // zero means "no icons sent yet" to the host
        let generation = (hasher.finish() as usize).max(1);
        if self.shared.icons_generation.load(Ordering::SeqCst) == generation {
            return Ok(());
        }

        let dir = host_local_state()?.join(ICONS_FOLDER);
        std::fs::create_dir_all(&dir)?;
        // files of the previous icons, or of the old script-based setup
        for entry in std::fs::read_dir(&dir)?.flatten() {
            if entry.path().is_file() {
                std::fs::remove_file(entry.path()).log_error();
            }
        }
        for file in &icons.files {
            // read & write instead of copy, as for the hook DLL (see the tray hook loader)
            match std::fs::read(&file.source) {
                Ok(content) => std::fs::write(dir.join(&file.file_name), content).log_error(),
                Err(err) => log::warn!("Missing theme icon {}: {err}", file.source.display()),
            }
        }
        std::fs::write(dir.join("map.txt"), map)?;

        self.shared
            .icons_generation
            .store(generation, Ordering::SeqCst);
        // the attach message carries the generation: the host loads the new icons
        self.shared.reattach_all();
        Ok(())
    }

    fn toggle(&self) -> Result<()> {
        if open_center().is_none() {
            let closed_at = *self.shared.closed_at.lock();
            if closed_at.is_some_and(|at| at.elapsed() < JUST_CLOSED) {
                return Ok(());
            }
        }
        // The hotkey handler toggles: it closes an open center.
        if invoke_shell_hotkey(NOTIFICATION_CENTER_HOTKEY).is_err() {
            std::process::Command::new("explorer.exe")
                .arg("ms-actioncenter:")
                .spawn()?;
        }
        Ok(())
    }
}

/// State shared with the window events subscription.
struct Shared {
    enabled: AtomicBool,
    /// Loading or hooking failed: don't retry (and log) on every window event until the next
    /// time the module is enabled.
    failed: AtomicBool,
    injection: TracedMutex<Option<Injection>>,
    /// When the center last closed.
    closed_at: TracedMutex<Option<Instant>>,
    icons_generation: AtomicUsize,
}

impl Shared {
    fn on_window_event(&self, event: WindowEvent, window: Window) {
        match event {
            WindowEvent::Created | WindowEvent::Shown | WindowEvent::Uncloaked => {
                if self.enabled.load(Ordering::SeqCst) && is_host_window(&window) {
                    self.attach(window);
                }
            }
            WindowEvent::Cloaked | WindowEvent::Hidden => {
                if is_host_window(&window) && is_center_sized(window.hwnd()) {
                    *self.closed_at.lock() = Some(Instant::now());
                }
            }
            WindowEvent::Destroyed => {
                if let Some(injection) = self.injection.lock().as_mut() {
                    injection.forget(window.address());
                }
            }
            _ => {}
        }
    }

    /// Loads the component on the window's thread and attaches it to the window, once.
    fn attach(&self, window: Window) {
        if self.failed.load(Ordering::SeqCst) {
            return;
        }
        let generation = self.icons_generation.load(Ordering::SeqCst);
        let newly = {
            let mut injection = self.injection.lock();
            if injection.is_none() {
                match Injection::load() {
                    Ok(loaded) => *injection = Some(loaded),
                    Err(err) => {
                        log::error!("Can't load the notification center component: {err}");
                        self.failed.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
            let Some(injection) = injection.as_mut() else {
                return;
            };
            match injection.track(window.address()) {
                Ok(newly) => newly,
                Err(err) => {
                    log::error!("Can't hook the notification host: {err}");
                    self.failed.store(true, Ordering::SeqCst);
                    return;
                }
            }
        };
        // never wait on the host while holding the lock
        if newly && !send(window.address(), &Messages::get().attach, generation) {
            log::warn!("The notification host didn't take the attach message");
            if let Some(injection) = self.injection.lock().as_mut() {
                injection.forget(window.address());
            }
        }
    }

    /// Sends the attach message again to the attached windows, with the current icons.
    fn reattach_all(&self) {
        let generation = self.icons_generation.load(Ordering::SeqCst);
        let windows = match self.injection.lock().as_ref() {
            Some(injection) => injection.windows(),
            None => return,
        };
        for hwnd in windows {
            send(hwnd, &Messages::get().attach, generation);
        }
    }

    /// Detaches the component from every window, unhooks the host and restarts it, so it gets
    /// its own look back (Windows starts the host again on demand).
    fn detach_all(&self) {
        let Some(injection) = self.injection.lock().take() else {
            return;
        };
        let windows = injection.windows();
        for hwnd in &windows {
            send(*hwnd, &Messages::get().detach, 0);
        }
        let processes: HashSet<u32> = windows
            .iter()
            .map(|hwnd| Window::from(*hwnd).process_id())
            .filter(|pid| *pid != 0)
            .collect();
        drop(injection);
        for pid in processes {
            terminate(pid).log_error();
        }
    }
}

/// The component loaded in Seelen (to hook with it) and the host threads hooked.
struct Injection {
    /// Dropped first: unhooking before the module is freed.
    threads: HashMap<u32, HookedThread>,
    hook_proc: HOOKPROC,
    module: Owned<HMODULE>,
}

// SAFETY: the handles are plain identifiers, usable from any thread.
unsafe impl Send for Injection {}

impl Injection {
    fn load() -> Result<Self> {
        let path = prepare_dll()?;
        let module = unsafe { Owned::new(LoadLibraryW(&HSTRING::from(path.as_os_str()))?) };
        let name = std::ffi::CString::new(HOOK_PROC).map_err(|_| "invalid hook procedure name")?;
        let proc = unsafe { GetProcAddress(*module, PCSTR(name.as_ptr() as _)) }
            .ok_or("the hook procedure is missing from the DLL")?;
        // SAFETY: the export has the HOOKPROC signature.
        let hook_proc: HOOKPROC = Some(unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
            >(proc)
        });
        log::info!(
            "Notification center component loaded from {}",
            path.display()
        );
        Ok(Self {
            threads: HashMap::new(),
            hook_proc,
            module,
        })
    }

    /// Hooks the window's thread if needed. Returns whether the window is new.
    fn track(&mut self, hwnd: isize) -> Result<bool> {
        let (_pid, thread_id) = WindowsApi::window_thread_process_id(HWND(hwnd as _));
        if thread_id == 0 {
            // the window is already gone
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

    /// Stops tracking a window, unhooking its thread when it was the last one there.
    fn forget(&mut self, hwnd: isize) {
        self.threads.retain(|_, thread| {
            thread.windows.remove(&hwnd);
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

/// The registered messages of the protocol.
struct Messages {
    attach: u32,
    detach: u32,
}

impl Messages {
    fn get() -> &'static Self {
        static MESSAGES: std::sync::LazyLock<Messages> = std::sync::LazyLock::new(|| unsafe {
            Messages {
                attach: RegisterWindowMessageW(&HSTRING::from(ATTACH_MESSAGE)),
                detach: RegisterWindowMessageW(&HSTRING::from(DETACH_MESSAGE)),
            }
        });
        &MESSAGES
    }
}

/// Sends a message to a host window, waiting a bounded time. Returns whether it was taken.
fn send(hwnd: isize, message: &u32, wparam: usize) -> bool {
    let taken = unsafe {
        SendMessageTimeoutW(
            HWND(hwnd as _),
            *message,
            WPARAM(wparam),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            None,
        )
    };
    taken.0 != 0
}

fn is_host_window(window: &Window) -> bool {
    window.class() == CORE_WINDOW_CLASS
        && window.exe_path().is_ok_and(|path| path.ends_with(HOST_EXE))
}

/// The host's windows: the notification center, the banners and the shell's flyouts.
fn host_windows() -> Vec<Window> {
    let mut found = Vec::new();
    let mut after: Option<HWND> = None;
    while let Ok(hwnd) =
        unsafe { FindWindowExW(None, after, w!("Windows.UI.Core.CoreWindow"), None) }
    {
        let window = Window::from(hwnd);
        if is_host_window(&window) {
            found.push(window);
        }
        after = Some(hwnd);
    }
    found
}

/// The notification center's window when it is open.
fn open_center() -> Option<Window> {
    host_windows().into_iter().find(|window| {
        window.is_visible() && !window.is_cloaked() && is_center_sized(window.hwnd())
    })
}

/// The center's window is the card, or still at the shell's full height; banners are far lower.
fn is_center_sized(hwnd: HWND) -> bool {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return false;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let card_height = CARD_HEIGHT * dpi / 96;
    rect.bottom - rect.top >= card_height - 2
}

/// Runs a hotkey of the shell (the `WM_HOTKEY` ids explorer registers for its Win + key
/// shortcuts), the same way the keyboard shortcut does.
fn invoke_shell_hotkey(id: usize) -> Result<()> {
    let shell = unsafe { FindWindowW(w!("ApplicationManager_ImmersiveShellWindow"), None)? };
    let (_pid, thread_id) = WindowsApi::window_thread_process_id(shell);

    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 16];
        let len = unsafe { GetClassNameW(hwnd, &mut class) };
        if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "WorkerW" {
            // SAFETY: `found` is the Vec passed below, alive for the whole enumeration.
            unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
        }
        true.into()
    }
    let mut workers: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumThreadWindows(
            thread_id,
            Some(collect),
            LPARAM(&mut workers as *mut Vec<HWND> as isize),
        );
    }
    if workers.is_empty() {
        return Err("the shell's hotkey window was not found".into());
    }
    for worker in workers {
        unsafe { PostMessageW(Some(worker), WM_HOTKEY, WPARAM(id), LPARAM(0))? };
    }
    Ok(())
}

/// The host's own `LocalState` folder, the only place outside its install it can read.
fn host_local_state() -> Result<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(local_app_data)
        .join("Packages")
        .join(HOST_PACKAGE_FAMILY)
        .join("LocalState");
    if !dir.is_dir() {
        return Err(format!("{} doesn't exist", dir.display()).into());
    }
    Ok(dir)
}

/// Copies the DLL into the host's `LocalState`, named by its content, and removes the copies of
/// other builds the host no longer has loaded.
fn prepare_dll() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let source = exe
        .parent()
        .ok_or("Failed to get executable directory")?
        .join(DLL_NAME);
    let content = std::fs::read(&source)?;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let file_name = format!("sluhk-{:016x}.dll", hasher.finish());

    let dir = host_local_state()?.join(DLL_FOLDER);
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
            // fails while a running host has it loaded, which is fine
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn terminate(pid: u32) -> Result<()> {
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, false, pid)?;
        let result = TerminateProcess(process, 0);
        let _ = CloseHandle(process);
        result?;
    }
    Ok(())
}
