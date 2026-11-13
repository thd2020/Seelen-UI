//! Generates the Rust bindings for the parts of `Windows.UI.Xaml` the notification center
//! component uses. The `windows` crate dropped `Windows.UI.Xaml`, but its metadata still ships
//! with `windows-bindgen`. Everything outside `Windows.UI.Xaml` comes from the `windows` crate.

const XAML_TYPES: &[&str] = &[
    "DependencyObject",
    "UIElement",
    "FrameworkElement",
    "Visibility",
    "Thickness",
    "CornerRadius",
    "HorizontalAlignment",
    "VerticalAlignment",
    "TextTrimming",
    "Window",
    "Media.VisualTreeHelper",
    "Media.Brush",
    "Media.SolidColorBrush",
    "Media.XamlCompositionBrushBase",
    "Media.AcrylicBrush",
    "Media.AcrylicBackgroundSource",
    "Media.ImageSource",
    "Media.FontFamily",
    "Media.Imaging.BitmapSource",
    "Media.Imaging.BitmapImage",
    "Media.Imaging.SvgImageSource",
    "Controls.Panel",
    "Controls.Grid",
    "Controls.Border",
    "Controls.ContentPresenter",
    "Controls.ItemsPresenter",
    "Controls.Control",
    "Controls.ContentControl",
    "Controls.Image",
    "Controls.TextBlock",
    "Controls.StackPanel",
    "Controls.IconElement",
    "Controls.FontIcon",
    "Controls.ItemsControl",
    "Controls.ListViewBase",
    "Controls.UIElementCollection",
    "Controls.Orientation",
    "Controls.Primitives.Selector",
    "Controls.Primitives.SelectorItem",
];

const FROM_WINDOWS_CRATE: &[&str] = &[
    "Windows.Foundation",
    "Windows.UI.Color",
    "Windows.UI.Composition",
    "Windows.UI.Core",
    "Windows.UI.Text",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("xaml.rs");

    let mut args = vec![
        "--out".to_string(),
        out.to_string_lossy().to_string(),
        "--flat".to_string(),
        "--no-allow".to_string(),
        "--no-comment".to_string(),
        "--filter".to_string(),
    ];
    args.extend(XAML_TYPES.iter().map(|t| format!("Windows.UI.Xaml.{t}")));
    for namespace in FROM_WINDOWS_CRATE {
        args.push("--reference".to_string());
        args.push(format!("windows,skip-root,{namespace}"));
    }

    // Members whose types are outside the filter are skipped with a warning, which is expected:
    // only what the component calls needs to be there.
    let _ = windows_bindgen::bindgen(args);
}
