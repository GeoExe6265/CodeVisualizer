mod app;
mod tracer;
mod value;

use std::sync::Arc;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("CodeVisualizer"),
        ..Default::default()
    };

    eframe::run_native(
        "codevisualizer",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
}

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
