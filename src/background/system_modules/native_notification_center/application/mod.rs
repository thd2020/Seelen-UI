#[cfg(windows)]
mod windows;

use std::sync::LazyLock;

use crate::{
    error::{Result, ResultLogExt},
    resources::RESOURCES,
    state::AppSettings,
};

use super::domain::{NativeNotificationCenterSystem, ThemeIcons};

#[cfg(windows)]
type Platform = windows::WindowsNativeNotificationCenter;

/// Folder of a theme with small app icons, and the file listing them (see
/// [`ThemeIcons::from_apps_json`]).
const THEME_ICONS_DIR: &str = "tray-icons";
const THEME_ICONS_LIST: &str = "apps.json";

pub struct NativeNotificationCenterManager {
    platform: Platform,
}

impl NativeNotificationCenterManager {
    pub fn instance() -> &'static Self {
        static MANAGER: LazyLock<NativeNotificationCenterManager> = LazyLock::new(|| {
            let mut m = NativeNotificationCenterManager::create();
            m.init().log_error();
            m
        });
        &MANAGER
    }

    fn create() -> Self {
        Self {
            platform: Platform::create(),
        }
    }

    fn init(&mut self) -> Result<()> {
        self.platform.init()
    }

    /// Follows the settings: the OS notification center is restyled while Seelen leaves
    /// notifications to the OS (its own notification popups are off).
    pub fn apply_settings(&self, state: &AppSettings) -> Result<()> {
        let enabled = !state.settings.by_widget.seelen_shows_notifications();
        if enabled {
            // before attaching, so the host loads the current icons right away
            self.platform
                .set_icons(&active_theme_icons(state))
                .log_error();
        }
        self.platform.set_enabled(enabled)
    }

    pub fn toggle(&self) -> Result<()> {
        self.platform.toggle()
    }
}

/// The small app icons of the active themes; a later theme replaces an earlier one's.
fn active_theme_icons(state: &AppSettings) -> ThemeIcons {
    let themes = RESOURCES.themes();
    let mut icons = ThemeIcons::default();
    for id in &state.settings.active_themes {
        let Some(theme) = themes.iter().find(|theme| &theme.id == id) else {
            continue;
        };
        let Ok(dir) = theme.metadata.directory() else {
            continue;
        };
        let dir = dir.join(THEME_ICONS_DIR);
        let Ok(json) = std::fs::read_to_string(dir.join(THEME_ICONS_LIST)) else {
            continue;
        };
        match ThemeIcons::from_apps_json(&dir, &json) {
            Ok(theme_icons) => icons = theme_icons,
            Err(err) => log::warn!("Invalid {THEME_ICONS_LIST} in theme {id}: {err}"),
        }
    }
    icons
}
