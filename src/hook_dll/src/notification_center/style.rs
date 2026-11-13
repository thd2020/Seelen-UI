//! The look of Windows 10's notification center and banners: a frosted card under the top bar,
//! notifications as rounded cards, round icon buttons, and banners as the same card.
//!
//! Each rule targets elements of the host's XAML (Windows 10 21H2's `ShellExperienceHost`) by a
//! selector: the element's direct visual ancestors, then the element, each written `Class#Name`
//! (`Class` alone or `#Name` alone match any name or class). Classes of the XAML framework match
//! by their short name (`Grid`), the shell's own ones by their full name
//! (`ActionCenter.ItemView`). Every value is only written when it differs, so passes over a
//! styled tree change nothing and don't start new layout passes.

use std::{cell::RefCell, collections::HashMap};

use windows::{
    UI::{Color, Text::FontWeight},
    core::{HSTRING, IInspectable, Interface, Result},
};

use super::xaml::{
    AcrylicBackgroundSource, AcrylicBrush, Border, Brush, ContentControl, ContentPresenter,
    Control, CornerRadius, DependencyObject, FontFamily, FontIcon, FrameworkElement, Grid,
    HorizontalAlignment, Image, ItemsPresenter, Panel, SolidColorBrush, TextBlock, Thickness,
    UIElement, VerticalAlignment, Visibility, VisualTreeHelper,
};

/// Deepest the walk goes; the host's trees are far shallower.
const MAX_DEPTH: usize = 80;

/// Width × height of the center's content, the card filling the center's window.
const CARD_WIDTH: f64 = slu_ipc::notification_center::CARD_WIDTH as f64;
const CARD_HEIGHT: f64 = slu_ipc::notification_center::CARD_HEIGHT as f64;

const TRANSPARENT: u32 = 0x00FF_FFFF;
const ACRYLIC_TINT: u32 = 0xFFF4_F4F6;
const ACRYLIC_TINT_OPACITY: f64 = 0.62;
const ACRYLIC_FALLBACK: u32 = 0xF5F4_F4F6;
const CARD_EDGE: u32 = 0x99FF_FFFF;
const ITEM_PLATE: u32 = 0xCCFF_FFFF;
const ITEM_EDGE: u32 = 0x1400_0000;
const BUTTON_PLATE: u32 = 0xE6FF_FFFF;
const BUTTON_EDGE: u32 = 0x1F00_0000;
const BUTTON_GLYPH: u32 = 0xD900_0000;
pub const SECONDARY_TEXT: u32 = 0x8A00_0000;
const ATTRIBUTION_TEXT: u32 = 0x8000_0000;

/// Segoe MDL2 Assets glyphs of the round buttons.
const SETTINGS_GLYPH: &str = "\u{E713}";
const CLEAR_ALL_GLYPH: &str = "\u{EA99}";

type Apply = fn(&DependencyObject) -> Result<()>;

/// The rules, in the order they apply.
const RULES: &[(&[&str], Apply)] = &[
    // The card: 396 × 560 (the width Windows gives the center), filling the center's window.
    (
        &["ActionCenter.ActionCenterPage", "Grid#RootGrid"],
        root_grid,
    ),
    (&["ActionCenter.ActionCenterPage"], transparent_background),
    (&["Grid#ActionCenterBackground"], card_background),
    (&["Image#DropShadow"], hide_drop_shadow),
    (&["Grid#NotificationCenterBackground"], notification_area),
    // No quick-action tiles: Seelen's own quick settings replace them. The collapse button only
    // folds the tiles, so it goes too.
    (&["Grid#ControlCenterBackground"], collapse),
    (
        &["ControlCenter.ExpandCollapseButton#ExpandCollapseButtonControl"],
        collapse,
    ),
    // One card per notification. The per-app header rows stay in the layout at zero height: the
    // cards read each notification's app from them (see `cards`).
    (
        &["ActionCenter.NotificationListViewHeaderItem"],
        flatten_header,
    ),
    // The divider between conversations: the cards are already apart.
    (&["Rectangle#SubgroupNotificationRule"], collapse),
    (&["Border#ItemOpaquePlating"], item_plate),
    (&["Border#RevealBackground"], item_reveal),
    // The side gutter sits inside each card's row, not around the list: Windows pins each row's
    // drawn position to the list's left edge (for swipe to dismiss), which shifted a padded list.
    (
        &[
            "ScrollContentPresenter#ScrollContentPresenter",
            "ItemsPresenter",
        ],
        no_padding,
    ),
    (&["ActionCenter.ItemView", "Grid#MainGrid"], item_gutter),
    (&["TextBlock#Attribution"], attribution),
    // Round icon buttons: settings (gear) at the top, clear all (broom) at the bottom.
    (
        &["Button#ManageNotificationSettingsButton"],
        settings_button,
    ),
    (
        &[
            "ActionCenter.ClearAllButton#ClearAllButtonControl",
            "Button#ClearAll",
        ],
        clear_all_button,
    ),
    (
        &[
            "Button#ManageNotificationSettingsButton",
            "Grid",
            "Border#Border",
        ],
        round_button_plate,
    ),
    (
        &[
            "ActionCenter.ClearAllButton#ClearAllButtonControl",
            "Button#ClearAll",
            "Grid",
            "Border#Border",
        ],
        round_button_plate,
    ),
    // Banners: the same frosted card and pill action buttons, no accent plate behind the icon.
    (&["Border#ToastBorder"], banner_card),
    (&["Border#ToastRevealBorder"], banner_reveal),
    (&["Border#AppLogoBorder"], transparent_background),
    (&["TextBlock#SenderName"], sender_name),
    (
        &["Button#VerbButton", "ContentPresenter#ContentPresenter"],
        pill_button,
    ),
];

thread_local! {
    /// Brushes are XAML objects: one per color and thread, so values compare equal across passes.
    static BRUSHES: RefCell<HashMap<u32, Brush>> = RefCell::new(HashMap::new());
    static ACRYLIC: RefCell<Option<Brush>> = const { RefCell::new(None) };
}

/// Class and name of an element in the walked tree.
pub struct Ident {
    /// Short name for the XAML framework's classes (`Grid`), full name for the others.
    pub class: String,
    pub name: String,
}

impl Ident {
    fn of(element: &DependencyObject) -> Self {
        let full = element
            .cast::<IInspectable>()
            .and_then(|i| i.GetRuntimeClassName())
            .map(|c| c.to_string())
            .unwrap_or_default();
        let class = match full.strip_prefix("Windows.UI.Xaml.") {
            Some(rest) => rest.rsplit('.').next().unwrap_or(rest).to_string(),
            None => full,
        };
        let name = element
            .cast::<FrameworkElement>()
            .and_then(|fe| fe.Name())
            .map(|n| n.to_string())
            .unwrap_or_default();
        Self { class, name }
    }

    /// Whether this matches one step of a selector: `Class#Name`, `Class` or `#Name`.
    fn matches(&self, step: &str) -> bool {
        let (class, name) = match step.split_once('#') {
            Some((class, name)) => (class, Some(name)),
            None => (step, None),
        };
        (class.is_empty() || class == self.class) && name.is_none_or(|name| name == self.name)
    }
}

/// Whether the element at the end of `chain` (its ancestors before it) matches `selector`.
pub fn matches(chain: &[Ident], selector: &[&str]) -> bool {
    selector.len() <= chain.len()
        && chain[chain.len() - selector.len()..]
            .iter()
            .zip(selector)
            .all(|(ident, step)| ident.matches(step))
}

/// Calls `visit` for every element under `root` (included), parents before children, with the
/// chain of idents from the root down to the element.
pub fn walk<F>(root: &impl Interface, visit: &mut F)
where
    F: FnMut(&DependencyObject, &[Ident]),
{
    let Ok(root) = root.cast::<DependencyObject>() else {
        return;
    };
    let mut chain = Vec::new();
    walk_from(&root, &mut chain, visit);
}

fn walk_from<F>(element: &DependencyObject, chain: &mut Vec<Ident>, visit: &mut F)
where
    F: FnMut(&DependencyObject, &[Ident]),
{
    if chain.len() >= MAX_DEPTH {
        return;
    }
    chain.push(Ident::of(element));
    visit(element, chain);
    let count = VisualTreeHelper::GetChildrenCount(element).unwrap_or(0);
    for i in 0..count {
        if let Ok(child) = VisualTreeHelper::GetChild(element, i) {
            walk_from(&child, chain, visit);
        }
    }
    chain.pop();
}

/// Applies the rules matching the element at the end of `chain`.
pub fn apply(element: &DependencyObject, chain: &[Ident]) {
    for (selector, apply) in RULES {
        if matches(chain, selector) {
            let _ = apply(element);
        }
    }
}

// ---- values ----

pub fn solid(argb: u32) -> Result<Brush> {
    BRUSHES.with_borrow_mut(|brushes| {
        if let Some(brush) = brushes.get(&argb) {
            return Ok(brush.clone());
        }
        let brush: Brush = SolidColorBrush::CreateInstanceWithColor(color(argb))?.cast()?;
        brushes.insert(argb, brush.clone());
        Ok(brush)
    })
}

fn acrylic() -> Result<Brush> {
    ACRYLIC.with_borrow_mut(|cached| {
        if let Some(brush) = cached {
            return Ok(brush.clone());
        }
        let brush = AcrylicBrush::new()?;
        brush.SetBackgroundSource(AcrylicBackgroundSource::HostBackdrop)?;
        brush.SetTintColor(color(ACRYLIC_TINT))?;
        brush.SetTintOpacity(ACRYLIC_TINT_OPACITY)?;
        brush.SetFallbackColor(color(ACRYLIC_FALLBACK))?;
        let brush: Brush = brush.cast()?;
        *cached = Some(brush.clone());
        Ok(brush)
    })
}

fn color(argb: u32) -> Color {
    let [a, r, g, b] = argb.to_be_bytes();
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn uniform(value: f64) -> Thickness {
    Thickness {
        Left: value,
        Top: value,
        Right: value,
        Bottom: value,
    }
}

fn radius(value: f64) -> CornerRadius {
    CornerRadius {
        TopLeft: value,
        TopRight: value,
        BottomRight: value,
        BottomLeft: value,
    }
}

pub fn semibold() -> FontWeight {
    FontWeight { Weight: 600 }
}

/// Writes a value only when it differs from the current one.
macro_rules! set {
    ($target:expr, $getter:ident, $setter:ident, $value:expr) => {{
        let value = $value;
        if $target.$getter().ok().as_ref() != Some(&value) {
            $target.$setter(value)?;
        }
    }};
}

/// Writes a brush only when the target doesn't have that same brush object already.
macro_rules! set_brush {
    ($target:expr, $getter:ident, $setter:ident, $brush:expr) => {{
        let brush = $brush;
        if $target.$getter().ok().as_ref() != Some(&brush) {
            $target.$setter(&brush)?;
        }
    }};
}

// ---- rules ----

fn root_grid(element: &DependencyObject) -> Result<()> {
    let fe: FrameworkElement = element.cast()?;
    set!(fe, Width, SetWidth, CARD_WIDTH);
    set!(fe, Height, SetHeight, CARD_HEIGHT);
    set!(
        fe,
        HorizontalAlignment,
        SetHorizontalAlignment,
        HorizontalAlignment::Left
    );
    set!(
        fe,
        VerticalAlignment,
        SetVerticalAlignment,
        VerticalAlignment::Top
    );
    set!(fe, Margin, SetMargin, uniform(0.0));
    Ok(())
}

fn transparent_background(element: &DependencyObject) -> Result<()> {
    set_background(element, solid(TRANSPARENT)?)
}

fn card_background(element: &DependencyObject) -> Result<()> {
    let grid: Grid = element.cast()?;
    set!(grid, CornerRadius, SetCornerRadius, radius(12.0));
    set!(grid, BorderThickness, SetBorderThickness, uniform(1.0));
    set_brush!(grid, BorderBrush, SetBorderBrush, solid(CARD_EDGE)?);
    set_brush!(grid, Background, SetBackground, acrylic()?);
    Ok(())
}

fn hide_drop_shadow(element: &DependencyObject) -> Result<()> {
    collapse(element)?;
    let image: Image = element.cast()?;
    set!(image, Opacity, SetOpacity, 0.0);
    set!(image, Width, SetWidth, 0.0);
    set!(image, Height, SetHeight, 0.0);
    // a null source reads as an error
    if image.Source().is_ok() {
        image.SetSource(None)?;
    }
    Ok(())
}

fn notification_area(element: &DependencyObject) -> Result<()> {
    transparent_background(element)?;
    let fe: FrameworkElement = element.cast()?;
    set!(fe, Height, SetHeight, CARD_HEIGHT);
    set!(
        fe,
        VerticalAlignment,
        SetVerticalAlignment,
        VerticalAlignment::Top
    );
    Ok(())
}

pub fn collapse(element: &DependencyObject) -> Result<()> {
    let ui: UIElement = element.cast()?;
    set!(ui, Visibility, SetVisibility, Visibility::Collapsed);
    Ok(())
}

fn flatten_header(element: &DependencyObject) -> Result<()> {
    let fe: FrameworkElement = element.cast()?;
    set!(fe, MinHeight, SetMinHeight, 0.0);
    set!(fe, Height, SetHeight, 0.0);
    set!(fe, Margin, SetMargin, uniform(0.0));
    set!(fe, Opacity, SetOpacity, 0.0);
    set!(fe, IsHitTestVisible, SetIsHitTestVisible, false);
    let control: Control = element.cast()?;
    set!(control, Padding, SetPadding, uniform(0.0));
    Ok(())
}

fn item_plate(element: &DependencyObject) -> Result<()> {
    let border: Border = element.cast()?;
    set_brush!(border, Background, SetBackground, solid(ITEM_PLATE)?);
    set!(border, CornerRadius, SetCornerRadius, radius(14.0));
    set!(border, BorderThickness, SetBorderThickness, uniform(1.0));
    set_brush!(border, BorderBrush, SetBorderBrush, solid(ITEM_EDGE)?);
    Ok(())
}

fn item_reveal(element: &DependencyObject) -> Result<()> {
    let border: Border = element.cast()?;
    set!(border, CornerRadius, SetCornerRadius, radius(14.0));
    Ok(())
}

fn no_padding(element: &DependencyObject) -> Result<()> {
    let presenter: ItemsPresenter = element.cast()?;
    set!(presenter, Padding, SetPadding, uniform(0.0));
    Ok(())
}

fn item_gutter(element: &DependencyObject) -> Result<()> {
    let fe: FrameworkElement = element.cast()?;
    let gutter = Thickness {
        Left: 16.0,
        Top: 0.0,
        Right: 16.0,
        Bottom: 8.0,
    };
    set!(fe, Margin, SetMargin, gutter);
    Ok(())
}

fn attribution(element: &DependencyObject) -> Result<()> {
    let text: TextBlock = element.cast()?;
    set_brush!(text, Foreground, SetForeground, solid(ATTRIBUTION_TEXT)?);
    set!(text, FontSize, SetFontSize, 11.0);
    Ok(())
}

fn settings_button(element: &DependencyObject) -> Result<()> {
    glyph_content(element, SETTINGS_GLYPH)
}

fn clear_all_button(element: &DependencyObject) -> Result<()> {
    glyph_content(element, CLEAR_ALL_GLYPH)
}

/// Replaces a button's content with a Segoe MDL2 Assets glyph.
fn glyph_content(element: &DependencyObject, glyph: &str) -> Result<()> {
    let button: ContentControl = element.cast()?;
    let current = button
        .Content()
        .ok()
        .and_then(|content| content.cast::<FontIcon>().ok())
        .and_then(|icon| icon.Glyph().ok());
    if current.is_some_and(|current| current == glyph) {
        return Ok(());
    }
    let icon = FontIcon::new()?;
    icon.SetGlyph(&HSTRING::from(glyph))?;
    icon.SetFontFamily(&FontFamily::CreateInstanceWithName(&HSTRING::from(
        "Segoe MDL2 Assets",
    ))?)?;
    icon.SetFontSize(16.0)?;
    icon.SetForeground(&solid(BUTTON_GLYPH)?)?;
    button.SetContent(&icon)?;
    Ok(())
}

fn round_button_plate(element: &DependencyObject) -> Result<()> {
    let border: Border = element.cast()?;
    set!(border, Width, SetWidth, 36.0);
    set!(border, Height, SetHeight, 36.0);
    set!(border, Padding, SetPadding, uniform(0.0));
    set!(border, CornerRadius, SetCornerRadius, radius(18.0));
    set_brush!(border, Background, SetBackground, solid(BUTTON_PLATE)?);
    set!(border, BorderThickness, SetBorderThickness, uniform(1.0));
    set_brush!(border, BorderBrush, SetBorderBrush, solid(BUTTON_EDGE)?);
    Ok(())
}

fn banner_card(element: &DependencyObject) -> Result<()> {
    let border: Border = element.cast()?;
    set!(border, CornerRadius, SetCornerRadius, radius(14.0));
    set!(border, BorderThickness, SetBorderThickness, uniform(1.0));
    set_brush!(border, BorderBrush, SetBorderBrush, solid(CARD_EDGE)?);
    set_brush!(border, Background, SetBackground, acrylic()?);
    Ok(())
}

fn banner_reveal(element: &DependencyObject) -> Result<()> {
    item_reveal(element)
}

fn sender_name(element: &DependencyObject) -> Result<()> {
    let text: TextBlock = element.cast()?;
    set!(text, FontSize, SetFontSize, 12.0);
    set!(text, FontWeight, SetFontWeight, semibold());
    set_brush!(text, Foreground, SetForeground, solid(SECONDARY_TEXT)?);
    Ok(())
}

fn pill_button(element: &DependencyObject) -> Result<()> {
    let presenter: ContentPresenter = element.cast()?;
    set!(presenter, CornerRadius, SetCornerRadius, radius(14.0));
    set_brush!(presenter, Background, SetBackground, solid(ITEM_PLATE)?);
    set!(presenter, BorderThickness, SetBorderThickness, uniform(1.0));
    set_brush!(presenter, BorderBrush, SetBorderBrush, solid(ITEM_EDGE)?);
    Ok(())
}

/// Sets the background of whatever kind of element has one.
fn set_background(element: &DependencyObject, brush: Brush) -> Result<()> {
    if let Ok(panel) = element.cast::<Panel>() {
        set_brush!(panel, Background, SetBackground, brush);
    } else if let Ok(border) = element.cast::<Border>() {
        set_brush!(border, Background, SetBackground, brush);
    } else if let Ok(control) = element.cast::<Control>() {
        set_brush!(control, Background, SetBackground, brush);
    } else if let Ok(presenter) = element.cast::<ContentPresenter>() {
        set_brush!(presenter, Background, SetBackground, brush);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(class: &str, name: &str) -> Ident {
        Ident {
            class: class.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn selectors_match_direct_ancestors_only() {
        let chain = [
            ident("ActionCenter.ActionCenterPage", ""),
            ident("Grid", "RootGrid"),
        ];
        assert!(matches(
            &chain,
            &["ActionCenter.ActionCenterPage", "Grid#RootGrid"]
        ));
        assert!(matches(&chain, &["Grid#RootGrid"]));
        assert!(matches(&chain, &["#RootGrid"]));
        assert!(matches(&chain, &["Grid"]));
        assert!(!matches(&chain, &["Grid#Other"]));
        assert!(!matches(&chain, &["Border#RootGrid"]));

        let deeper = [
            ident("ActionCenter.ActionCenterPage", ""),
            ident("Border", ""),
            ident("Grid", "RootGrid"),
        ];
        assert!(!matches(
            &deeper,
            &["ActionCenter.ActionCenterPage", "Grid#RootGrid"]
        ));
    }

    #[test]
    fn selectors_longer_than_the_chain_never_match() {
        let chain = [ident("Grid", "RootGrid")];
        assert!(!matches(
            &chain,
            &["ActionCenter.ActionCenterPage", "Grid#RootGrid"]
        ));
    }

    #[test]
    fn colors_unpack_as_argb() {
        let c = color(0x80FF_0102);
        assert_eq!((c.A, c.R, c.G, c.B), (0x80, 0xFF, 0x01, 0x02));
    }
}
