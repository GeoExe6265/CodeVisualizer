use serde::Deserialize;

/// A serialized Python value produced by `python/tracer.py`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ValueNode {
    #[serde(rename = "t", default)]
    pub type_name: String,
    #[serde(default)]
    pub repr: String,
    #[serde(default)]
    pub len: Option<usize>,
    #[serde(default)]
    pub items: Vec<ValueNode>,
    #[serde(default)]
    pub entries: Vec<(ValueNode, ValueNode)>,
}

impl ValueNode {
    fn is_container(&self) -> bool {
        !self.items.is_empty() || !self.entries.is_empty()
    }

    fn count(&self) -> usize {
        self.len.unwrap_or(self.items.len() + self.entries.len())
    }
}

pub fn show_value(ui: &mut egui::Ui, name: &str, value: &ValueNode) {
    if !value.is_container() {
        ui.horizontal(|ui| {
            ui.monospace(name);
            ui.monospace("=");
            ui.label(
                egui::RichText::new(&value.repr)
                    .monospace()
                    .color(type_color(&value.type_name)),
            );
        });
        return;
    }

    let header = format!("{name} = {}({})", value.type_name, value.count());
    egui::CollapsingHeader::new(
        egui::RichText::new(header)
            .monospace()
            .color(type_color(&value.type_name)),
    )
    .default_open(true)
    .show(ui, |ui| {
        for (index, item) in value.items.iter().enumerate() {
            show_value(ui, &format!("[{index}]"), item);
        }

        for (key, item) in &value.entries {
            let key_name = if key.repr.is_empty() {
                format!("<{}>", key.type_name)
            } else {
                key.repr.clone()
            };
            show_value(ui, &key_name, item);
        }

        let shown = value.items.len() + value.entries.len();
        if value.count() > shown {
            ui.weak(format!("… показаны первые {shown}"));
        }
    });
}

fn type_color(type_name: &str) -> egui::Color32 {
    match type_name {
        "int" | "float" | "complex" => egui::Color32::from_rgb(120, 200, 255),
        "str" | "bytes" | "bytearray" => egui::Color32::from_rgb(150, 220, 140),
        "bool" => egui::Color32::from_rgb(240, 200, 100),
        "NoneType" => egui::Color32::from_gray(150),
        "list" | "tuple" | "set" | "frozenset" | "dict" => egui::Color32::from_rgb(240, 170, 110),
        _ => egui::Color32::from_rgb(220, 220, 220),
    }
}
