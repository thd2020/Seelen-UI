//! One card per notification.
//!
//! Windows 10 groups the notification center by app: a header row with the app's logo and name,
//! then that app's notifications. The header rows are flattened away (see `style`), and every
//! notification gets its own row with the app's icon and name on top instead, Android-style. A
//! notification doesn't carry its app's name, but its group header does, and
//! `ListViewBase.GroupHeaderContainerFromItemContainer` finds it.
//!
//! - Apps the theme has an icon for show that icon, the others their logo, and Windows' default
//!   app glyph when there is neither.
//! - Conversation headers (a title with no message and no time) are hidden, so the cards match
//!   the notification count.
//! - Repeated cards in the same app group, with the same visible title, message and time, show
//!   once at a time. Windows' notifications stay as they are, so dismissing the shown card may
//!   reveal the next copy.
//! - Banners already show the app's logo and name; the theme's icon replaces the logo.

use std::collections::{HashMap, HashSet};

use windows::{
    Foundation::{IPropertyValue, PropertyValue},
    core::{HSTRING, Interface, Result},
};

use super::{
    icons,
    style::{Ident, SECONDARY_TEXT, semibold, solid},
    xaml::{
        DependencyObject, FontFamily, FontIcon, FrameworkElement, Image, ImageSource, ListViewBase,
        Orientation, StackPanel, TextBlock, TextTrimming, Thickness, UIElement, VerticalAlignment,
        Visibility, VisualTreeHelper,
    },
};

const CARD_CLASS: &str = "ActionCenter.NotificationListViewItem";
const BANNER_CLASS: &str = "ActionCenter.ToastView";
/// Name of the icon-and-name row added on top of each card.
const ROW_NAME: &str = "SeelenCardHeader";
/// Segoe MDL2 Assets' default app icon.
const DEFAULT_APP_GLYPH: &str = "\u{ECAA}";
/// Elements are only searched this deep under a card.
const MAX_SEARCH_DEPTH: usize = 30;

/// State of one pass over the XAML.
#[derive(Default)]
pub struct CardsPass {
    /// Cards shown so far in this pass, to hide their repeats.
    shown: HashSet<String>,
}

impl CardsPass {
    pub fn visit(&mut self, element: &DependencyObject, chain: &[Ident]) {
        match chain.last().map(|ident| ident.class.as_str()) {
            Some(CARD_CLASS) => {
                let _ = self.patch_card(element);
            }
            Some(BANNER_CLASS) => {
                let _ = patch_banner(element);
            }
            _ => {}
        }
    }

    fn patch_card(&mut self, item: &DependencyObject) -> Result<()> {
        let named = find_named(
            item,
            &["Title", "Content", "Attribution", "TextContentPanel"],
        );
        let header = is_conversation_header(&named);
        let app = if header { None } else { AppGroup::of(item) };
        let repeated = app
            .as_ref()
            .and_then(|app| repeat_key(&named, app))
            .is_some_and(|key| !self.shown.insert(key));

        // Containers are recycled for other notifications: set the visibility both ways.
        let hidden = header || repeated;
        let container: UIElement = item.cast()?;
        let wanted = if hidden {
            Visibility::Collapsed
        } else {
            Visibility::Visible
        };
        if container.Visibility()? != wanted {
            container.SetVisibility(wanted)?;
        }
        if hidden {
            return Ok(());
        }

        let (Some(app), Some(panel)) = (app, named.get("TextContentPanel")) else {
            return Ok(());
        };
        update_row(&panel.cast()?, &app)
    }
}

/// The app group a card is listed under.
struct AppGroup {
    name: String,
    /// Identity of the group's header, so equal apps in separate groups stay apart.
    id: usize,
    logo: Option<ImageSource>,
}

impl AppGroup {
    fn of(item: &DependencyObject) -> Option<Self> {
        let list = list_ancestor(&VisualTreeHelper::GetParent(item).ok()?)?;
        let header = list.GroupHeaderContainerFromItemContainer(item).ok()?;
        let named = find_named(&header, &["Title", "StandardImage"]);
        let name = named
            .get("Title")
            .and_then(|title| title.cast::<TextBlock>().ok())
            .and_then(|title| title.Text().ok())
            .map(|text| text.to_string())
            .unwrap_or_default();
        let logo = named
            .get("StandardImage")
            .and_then(|image| image.cast::<Image>().ok())
            .and_then(|image| image.Source().ok());
        Some(Self {
            name,
            id: header.as_raw() as usize,
            logo,
        })
    }
}

/// Which icon a card's row shows; kept in the icon element's name to tell when it must change.
enum RowIcon {
    Theme(String),
    Logo(ImageSource),
    Glyph,
}

impl RowIcon {
    fn for_app(app: &AppGroup) -> Self {
        if let Some(file) = icons::file_for(&app.name) {
            return Self::Theme(file);
        }
        match &app.logo {
            Some(logo) => Self::Logo(logo.clone()),
            None => Self::Glyph,
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Theme(file) => format!("seelen-icon:theme:{file}"),
            Self::Logo(_) => "seelen-icon:logo".to_string(),
            Self::Glyph => "seelen-icon:glyph".to_string(),
        }
    }
}

/// Adds or updates the icon-and-name row at the top of a card's text.
fn update_row(panel: &StackPanel, app: &AppGroup) -> Result<()> {
    let children = panel.Children()?;
    let wanted = RowIcon::for_app(app);
    let wanted_key = wanted.key();

    if children.Size()? > 0 {
        let first = children.GetAt(0)?;
        if first.cast::<FrameworkElement>()?.Name()? == ROW_NAME {
            if row_is_current(&first.cast()?, &app.name, &wanted_key) {
                return Ok(());
            }
            // recycled for another app, or its icon changed
            children.RemoveAt(0)?;
        }
    }

    let row = StackPanel::new()?;
    row.SetName(&HSTRING::from(ROW_NAME))?;
    row.SetOrientation(Orientation::Horizontal)?;
    row.SetMargin(Thickness {
        Left: 0.0,
        Top: 0.0,
        Right: 0.0,
        Bottom: 6.0,
    })?;
    let row_children = row.Children()?;
    row_children.Append(&row_icon(wanted, &wanted_key)?)?;

    let name = TextBlock::new()?;
    name.SetText(&HSTRING::from(app.name.as_str()))?;
    name.SetFontSize(12.0)?;
    name.SetFontWeight(semibold())?;
    name.SetForeground(&solid(SECONDARY_TEXT)?)?;
    name.SetMargin(Thickness {
        Left: 6.0,
        Top: 0.0,
        Right: 0.0,
        Bottom: 0.0,
    })?;
    name.SetVerticalAlignment(VerticalAlignment::Center)?;
    name.SetTextTrimming(TextTrimming::CharacterEllipsis)?;
    row_children.Append(&name)?;

    children.InsertAt(0, &row)?;
    Ok(())
}

fn row_is_current(row: &StackPanel, app_name: &str, icon_key: &str) -> bool {
    let check = || -> Result<bool> {
        let children = row.Children()?;
        if children.Size()? < 2 {
            return Ok(false);
        }
        let text = children.GetAt(1)?.cast::<TextBlock>()?.Text()?;
        let icon = children.GetAt(0)?.cast::<FrameworkElement>()?;
        let mut current = text == app_name && icon.Name()? == icon_key;
        if current && let Ok(image) = icon.cast::<Image>() {
            // an icon still loading reads as no source: look again later
            current = image.Source().is_ok();
        }
        Ok(current)
    };
    check().unwrap_or(false)
}

fn row_icon(icon: RowIcon, key: &str) -> Result<FrameworkElement> {
    let source = match icon {
        RowIcon::Theme(file) => icons::image(&file),
        RowIcon::Logo(source) => Some(source),
        RowIcon::Glyph => None,
    };
    let element: FrameworkElement = match source {
        Some(source) => {
            let image = Image::new()?;
            image.SetSource(&source)?;
            image.cast()?
        }
        None => {
            let glyph = FontIcon::new()?;
            glyph.SetGlyph(&HSTRING::from(DEFAULT_APP_GLYPH))?;
            glyph.SetFontFamily(&FontFamily::CreateInstanceWithName(&HSTRING::from(
                "Segoe MDL2 Assets",
            ))?)?;
            glyph.SetFontSize(16.0)?;
            glyph.SetForeground(&solid(SECONDARY_TEXT)?)?;
            glyph.cast()?
        }
    };
    element.SetName(&HSTRING::from(key))?;
    element.SetWidth(16.0)?;
    element.SetHeight(16.0)?;
    element.SetVerticalAlignment(VerticalAlignment::Center)?;
    Ok(element)
}

/// Banners already show the app's logo and name; swaps in the theme's icon.
fn patch_banner(toast: &DependencyObject) -> Result<()> {
    let named = find_named(toast, &["SenderName", "AppLogo"]);
    let (Some(sender), Some(logo)) = (named.get("SenderName"), named.get("AppLogo")) else {
        return Ok(());
    };
    let app_name = sender.cast::<TextBlock>()?.Text()?.to_string();
    // The logo's tag holds the app it was done for.
    let done_for = logo
        .Tag()
        .and_then(|tag| tag.cast::<IPropertyValue>())
        .and_then(|value| value.GetString())
        .map(|value| value.to_string());
    if done_for.is_ok_and(|done_for| done_for == app_name) {
        return Ok(());
    }
    match icons::file_for(&app_name) {
        Some(file) => {
            if let Some(source) = icons::image(&file) {
                let image: Image = logo.cast()?;
                image.SetSource(&source)?;
                image.SetWidth(16.0)?;
                image.SetHeight(16.0)?;
            }
        }
        // the map is still loading: try again on the next pass
        None if !icons::loaded() => return Ok(()),
        None => {}
    }
    logo.SetTag(&PropertyValue::CreateString(&HSTRING::from(app_name))?)?;
    Ok(())
}

/// A conversation header: the item has a title but no message and no time.
fn is_conversation_header(named: &HashMap<&str, FrameworkElement>) -> bool {
    named.contains_key("Title")
        && !named.get("Content").is_some_and(is_shown)
        && !named.get("Attribution").is_some_and(is_shown)
}

/// What makes two cards of the same app group copies of each other: the same visible title,
/// message and time.
fn repeat_key(named: &HashMap<&str, FrameworkElement>, app: &AppGroup) -> Option<String> {
    if app.name.is_empty() || app.id == 0 {
        return None;
    }
    let title = visible_text(named, "Title");
    let content = visible_text(named, "Content");
    if title.is_empty() && content.is_empty() {
        return None;
    }
    let attribution = visible_text(named, "Attribution");
    Some(
        [
            app.id.to_string(),
            app.name.to_lowercase(),
            title,
            content,
            attribution,
        ]
        .join("\u{1f}"),
    )
}

fn is_shown(element: &FrameworkElement) -> bool {
    element
        .Visibility()
        .is_ok_and(|visibility| visibility == Visibility::Visible)
}

fn visible_text(named: &HashMap<&str, FrameworkElement>, name: &str) -> String {
    named
        .get(name)
        .filter(|element| is_shown(element))
        .and_then(|element| element.cast::<TextBlock>().ok())
        .and_then(|text| text.Text().ok())
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn list_ancestor(start: &DependencyObject) -> Option<ListViewBase> {
    let mut current = start.clone();
    for _ in 0..40 {
        if let Ok(list) = current.cast::<ListViewBase>() {
            return Some(list);
        }
        current = VisualTreeHelper::GetParent(&current).ok()?;
    }
    None
}

/// The first element with each of `names` under `root` (excluded), depth first.
fn find_named<'a>(root: &impl Interface, names: &[&'a str]) -> HashMap<&'a str, FrameworkElement> {
    let mut found = HashMap::new();
    if let Ok(root) = root.cast::<DependencyObject>() {
        search(&root, names, &mut found, 0);
    }
    found
}

fn search<'a>(
    element: &DependencyObject,
    names: &[&'a str],
    found: &mut HashMap<&'a str, FrameworkElement>,
    depth: usize,
) {
    if depth > MAX_SEARCH_DEPTH || found.len() == names.len() {
        return;
    }
    let count = VisualTreeHelper::GetChildrenCount(element).unwrap_or(0);
    for i in 0..count {
        let Ok(child) = VisualTreeHelper::GetChild(element, i) else {
            continue;
        };
        if let Ok(fe) = child.cast::<FrameworkElement>()
            && let Ok(name) = fe.Name()
            && let Some(wanted) = names.iter().find(|wanted| name == **wanted)
        {
            found.entry(*wanted).or_insert(fe);
        }
        search(&child, names, found, depth + 1);
    }
}
