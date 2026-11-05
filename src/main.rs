mod app;
mod tracer;
mod value;

use std::sync::Arc;

const ICON_PNG: &[u8] = include_bytes!("../assets/codevisualizer.png");

fn main() -> eframe::Result {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("CodeVisualizer")
            .with_app_id("codevisualizer")
            .with_icon(window_icon()),
        ..Default::default()
    };
    prefer_x11_for_file_drops(&mut options);

    eframe::run_native(
        "codevisualizer",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
}

fn window_icon() -> Arc<egui::IconData> {
    // Keep this at 192x192 or smaller: winit sends the whole X11 icon in one
    // ChangeProperty request, and 256x256 exceeds the legacy request limit on
    // some X servers, leaving the window without an icon.
    Arc::new(
        eframe::icon_data::from_png_bytes(ICON_PNG)
            .expect("bundled application icon is a valid PNG"),
    )
}

/// winit's Wayland backend does not implement file drag-and-drop (as of 0.30).
/// Running through XWayland makes drops from Dolphin and other applications
/// work. Set `CODEVISUALIZER_NATIVE_WAYLAND=1` to keep the native Wayland
/// backend and give up file dragging.
#[cfg(target_os = "linux")]
fn prefer_x11_for_file_drops(options: &mut eframe::NativeOptions) {
    if std::env::var_os("CODEVISUALIZER_NATIVE_WAYLAND").is_some()
        || std::env::var_os("DISPLAY").is_none()
    {
        return;
    }

    use winit::platform::x11::EventLoopBuilderExtX11 as _;
    options.event_loop_builder = Some(Box::new(|builder| {
        builder.with_x11();
    }));
}

#[cfg(not(target_os = "linux"))]
fn prefer_x11_for_file_drops(_options: &mut eframe::NativeOptions) {}

fn setup_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());

    let mut fonts = egui::FontDefinitions::default();

    // Noto Sans / Noto Sans Mono are present on Arch with the `noto-fonts` package.
    // They guarantee Cyrillic support in both UI labels and code.
    if let Ok(bytes) = std::fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf") {
        fonts.font_data.insert(
            "noto_sans".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "noto_sans".to_owned());
        }
    }

    if let Ok(bytes) = std::fs::read("/usr/share/fonts/noto/NotoSansMono-Regular.ttf") {
        fonts.font_data.insert(
            "noto_mono".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "noto_mono".to_owned());
        }
    }

    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_window_icon_is_loaded() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (192, 192));
        assert_eq!(icon.rgba.len(), 192 * 192 * 4);
    }
}
