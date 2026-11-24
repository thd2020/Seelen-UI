use std::path::{Path, PathBuf};

use crate::error::Result;

/// OS contract of the native notification center module: the OS's own notification center and
/// banners, restyled to match Seelen UI, for when Seelen leaves notifications to the OS.
pub trait NativeNotificationCenterSystem: Sized + Send + Sync {
    /// Builds the native struct. Never fails: no OS calls here, resources are acquired in `init`.
    fn create() -> Self;

    /// Starts following the OS notification host's windows. Called once.
    fn init(&mut self) -> Result<()>;

    /// Restyles the OS notification center and banners (enabled), or gives them back their own
    /// look (disabled). OSes without a supported notification center ignore it.
    fn set_enabled(&self, enabled: bool) -> Result<()>;

    /// Gives the restyled notifications the theme's small app icons.
    fn set_icons(&self, icons: &ThemeIcons) -> Result<()>;

    /// Opens the OS notification center, or closes it when it is open.
    fn toggle(&self) -> Result<()>;
}

/// Small app icons of a theme, for the notifications of the apps they stand for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeIcons {
    /// Icon files, each listed once.
    pub files: Vec<ThemeIconFile>,
    /// App name -> icon file name, in the order the theme lists them (later names win).
    pub names: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeIconFile {
    pub source: PathBuf,
    pub file_name: String,
}

impl ThemeIcons {
    /// Reads a theme's `apps.json`, as written by the glass-bars theme's `tray-icons/build.py`:
    /// `{ "<program>.exe": { "icon": "<file>.svg", "tooltips": ["<app name>", ...] } }`. Each
    /// app is known by its program's file name without extension and by its tray tooltips, which
    /// are the names its notifications show.
    pub fn from_apps_json(icons_dir: &Path, json: &str) -> Result<Self> {
        let apps: serde_json::Value = serde_json::from_str(json)?;
        let apps = apps.as_object().ok_or("apps.json is not an object")?;

        let mut icons = Self::default();
        for (program, app) in apps {
            let Some(file_name) = app.get("icon").and_then(|icon| icon.as_str()) else {
                continue;
            };
            if !is_plain_file_name(file_name) {
                continue;
            }
            if !icons.files.iter().any(|f| f.file_name == file_name) {
                icons.files.push(ThemeIconFile {
                    source: icons_dir.join(file_name),
                    file_name: file_name.to_string(),
                });
            }

            let stem = Path::new(program)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
            let tooltips = app
                .get("tooltips")
                .and_then(|tooltips| tooltips.as_array())
                .into_iter()
                .flatten()
                .filter_map(|tooltip| tooltip.as_str());
            for name in std::iter::once(stem.as_str()).chain(tooltips) {
                let name = name.trim();
                if !name.is_empty() && !name.contains(['=', '\r', '\n']) {
                    icons.names.push((name.to_string(), file_name.to_string()));
                }
            }
        }
        Ok(icons)
    }

    /// The `name=file` lines read by the notification host's side.
    pub fn map_text(&self) -> String {
        self.names
            .iter()
            .map(|(name, file)| format!("{name}={file}\n"))
            .collect()
    }
}

/// File names are used in `ms-appdata:` URIs as they are: only plain names, nothing to escape.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_json_lists_program_and_tooltip_names() -> Result<()> {
        let json = r#"{
            "weixin.exe": { "icon": "weixin.svg", "tooltips": ["Weixin", "微信"], "item": "tray-weixin" },
            "QQ.exe": { "icon": "qq.svg", "tooltips": ["QQ"] },
            "bad.exe": { "icon": "../evil.svg", "tooltips": ["Bad"] },
            "noicon.exe": { "tooltips": ["No icon"] }
        }"#;
        let icons = ThemeIcons::from_apps_json(Path::new("icons"), json)?;

        let files: Vec<_> = icons.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"weixin.svg") && files.contains(&"qq.svg"));
        assert!(
            icons
                .files
                .iter()
                .all(|f| f.source == Path::new("icons").join(&f.file_name))
        );

        let map = icons.map_text();
        for line in [
            "weixin=weixin.svg",
            "Weixin=weixin.svg",
            "微信=weixin.svg",
            "QQ=qq.svg",
        ] {
            assert!(map.lines().any(|l| l == line), "missing {line} in {map}");
        }
        assert!(!map.contains("evil") && !map.contains("No icon"));
        Ok(())
    }

    #[test]
    fn only_plain_file_names_are_accepted() {
        assert!(is_plain_file_name("qq.svg"));
        assert!(is_plain_file_name("glass_bars-1.png"));
        assert!(!is_plain_file_name("../qq.svg"));
        assert!(!is_plain_file_name("a b.svg"));
        assert!(!is_plain_file_name(".hidden"));
        assert!(!is_plain_file_name(""));
    }
}
