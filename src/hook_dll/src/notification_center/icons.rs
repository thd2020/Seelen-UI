//! Theme icons for the cards and banners.
//!
//! Seelen UI puts the active theme's small app icons in the host's `LocalState\seelen-icons`
//! folder, the only place the sandboxed host can read, with `map.txt` mapping app names to icon
//! files. The map loads on a thread of its own: the host's UI threads run the component inside
//! messages sent from another process, where Windows refuses outgoing COM calls.

use std::{
    collections::HashMap,
    sync::{Mutex, RwLock},
};

use slu_ipc::notification_center::ICONS_FOLDER;
use windows::{
    Foundation::Uri,
    Storage::ApplicationData,
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    core::{HSTRING, Interface},
};

use super::xaml::{BitmapImage, ImageSource, SvgImageSource};

/// Lowercased app name -> icon file name.
static ICONS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);
/// Generation of the icons last requested, see `slu_ipc::notification_center::ATTACH_MESSAGE`.
static REQUESTED: Mutex<Option<usize>> = Mutex::new(None);

/// Loads the icon map again if `generation` differs from the last one requested.
pub fn request(generation: usize) {
    {
        let Ok(mut requested) = REQUESTED.lock() else {
            return;
        };
        if *requested == Some(generation) {
            return;
        }
        *requested = Some(generation);
    }
    let _ = std::thread::Builder::new()
        .name("seelen-icons".into())
        .spawn(|| {
            let map = std::panic::catch_unwind(load).ok().flatten();
            if let Ok(mut icons) = ICONS.write() {
                *icons = map;
            }
        });
}

/// The theme's icon file for an app, by the name the notification shows for it.
pub fn file_for(app_name: &str) -> Option<String> {
    let icons = ICONS.read().ok()?;
    icons
        .as_ref()?
        .get(&app_name.trim().to_lowercase())
        .cloned()
}

/// Whether the icon map is loaded.
pub fn loaded() -> bool {
    ICONS.read().is_ok_and(|icons| icons.is_some())
}

/// An image of one of the theme's icon files.
pub fn image(file: &str) -> Option<ImageSource> {
    let uri = Uri::CreateUri(&HSTRING::from(format!(
        "ms-appdata:///local/{ICONS_FOLDER}/{file}"
    )))
    .ok()?;
    if file.to_lowercase().ends_with(".svg") {
        SvgImageSource::CreateInstanceWithUriSource(&uri)
            .ok()?
            .cast()
            .ok()
    } else {
        BitmapImage::CreateInstanceWithUriSource(&uri)
            .ok()?
            .cast()
            .ok()
    }
}

fn load() -> Option<HashMap<String, String>> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let folder = ApplicationData::Current()
        .and_then(|data| data.LocalFolder())
        .and_then(|folder| folder.Path())
        .map(|path| std::path::PathBuf::from(path.to_string()));
    if initialized {
        unsafe { CoUninitialize() };
    }
    let text = std::fs::read_to_string(folder.ok()?.join(ICONS_FOLDER).join("map.txt")).ok()?;
    Some(parse_map(&text))
}

/// Parses `name=file` lines; names compare case-insensitively.
fn parse_map(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim_end_matches('\r'))
        .filter_map(|line| line.split_once('='))
        .filter(|(name, file)| !name.trim().is_empty() && !file.trim().is_empty())
        .map(|(name, file)| (name.trim().to_lowercase(), file.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_map;

    #[test]
    fn map_lines_are_case_insensitive_names() {
        let map = parse_map("\u{feff}WeChat=weixin.svg\r\nQQ=qq.svg\n=nothing.svg\nbroken\n");
        assert_eq!(map.get("wechat").map(String::as_str), Some("weixin.svg"));
        assert_eq!(map.get("qq").map(String::as_str), Some("qq.svg"));
        assert_eq!(map.len(), 2);
    }
}
