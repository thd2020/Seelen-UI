//! The OS's own notification center and banners, restyled to match Seelen UI, for when Seelen
//! leaves notifications to the OS (its own notification popups are off). Windows 10 only.

mod application;
mod domain;

use crate::{error::Result, state::AppSettings, utils::spawn_named_thread};

use self::application::NativeNotificationCenterManager;

/// Follows the settings in the background: attaching to the notification host waits on it.
pub fn apply_settings(state: &AppSettings) {
    let state = state.clone();
    spawn_named_thread("Native Notification Center", move || {
        if let Err(err) = NativeNotificationCenterManager::instance().apply_settings(&state) {
            log::error!("Failed to set up the native notification center: {err}");
        }
    });
}

impl crate::tauri_handlers::Handlers {
    pub fn toggle_native_notification_center() -> Result<()> {
        NativeNotificationCenterManager::instance().toggle()
    }
}
