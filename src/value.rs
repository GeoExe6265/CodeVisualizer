use serde::Deserialize;

const FOCUS_ARROW: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);

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

/// A list/dict element used by the current source line.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Focus {
    #[serde(default)]
    pub variable: String,
    #[serde(default)]
    pub path: Vec<FocusStep>,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum FocusStep {
    #[serde(rename = "index")]
    Index { index: usize },
    #[serde(rename = "key")]
    Key { key: String },
    #[serde(rename = "attr")]
    Attr { name: String },
}

impl FocusStep {
    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Index { index: left }, Self::Index { index: right }) => left == right,
            (Self::Key { key: left }, Self::Key { key: right }) => left == right,
            (Self::Attr { name: left }, Self::Attr { name: right }) => left == right,
            (Self::Key { key }, Self::Attr { name }) => key == name,
            (Self::Attr { name }, Self::Key { key }) => name == key,
            _ => false,
        }
    }
}

impl Focus {
    fn matches(&self, root: &str, path: &[FocusStep]) -> bool {
        self.variable == root
            && self.path.len() == path.len()
            && self
                .path
                .iter()
                .zip(path)
                .all(|(left, right)| left.matches(right))
    }
}

fn is_focused(focus: &[Focus], root: &str, path: &[FocusStep]) -> bool {
    focus.iter().any(|target| target.matches(root, path))
}

impl ValueNode {
    fn is_container(&self) -> bool {
        !self.items.is_empty() || !self.entries.is_empty()
    }

    fn count(&self) -> usize {
        self.len.unwrap_or(self.items.len() + self.entries.len())
    }
}

pub fn show_value(
    ui: &mut egui::Ui,
    name: &str,
    value: &ValueNode,
    root: &str,
    path: &mut Vec<FocusStep>,
    focus: &[Focus],
) {
    let focused = is_focused(focus, root, path);

    if !value.is_container() {
        ui.horizontal(|ui| {
            if focused {
                ui.label(egui::RichText::new("->").color(FOCUS_ARROW).strong());
            }
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
    egui::CollapsingHeader::new(header_widget(
        ui,
        header,
        type_color(&value.type_name),
        focused,
    ))
    .default_open(true)
    .show(ui, |ui| {
        for (index, item) in value.items.iter().enumerate() {
            path.push(FocusStep::Index { index });
            show_value(ui, &format!("[{index}]"), item, root, path, focus);
            path.pop();
        }

        for (key, item) in &value.entries {
            let key_name = if key.repr.is_empty() {
                format!("<{}>", key.type_name)
            } else {
                key.repr.clone()
            };
            path.push(FocusStep::Key {
                key: key.repr.clone(),
            });
            show_value(ui, &key_name, item, root, path, focus);
            path.pop();
        }

        let shown = value.items.len() + value.entries.len();
        if value.count() > shown {
            ui.weak(format!("… показаны первые {shown}"));
        }
    });
}

fn header_widget(
    ui: &egui::Ui,
    text: String,
    color: egui::Color32,
    focused: bool,
) -> egui::WidgetText {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    if focused {
        job.append(
            "-> ",
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: FOCUS_ARROW,
                ..Default::default()
            },
        );
    }
    job.append(
        &text,
        0.0,
        egui::TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    job.into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_matches_object_and_dict_entries() {
        let focus = Focus {
            variable: "box".to_owned(),
            path: vec![
                FocusStep::Attr {
                    name: "items".to_owned(),
                },
                FocusStep::Index { index: 1 },
            ],
            label: "box.items[1]".to_owned(),
        };

        assert!(focus.matches(
            "box",
            &[
                FocusStep::Key {
                    key: "items".to_owned()
                },
                FocusStep::Index { index: 1 }
            ]
        ));
        assert!(!focus.matches(
            "box",
            &[
                FocusStep::Key {
                    key: "items".to_owned()
                },
                FocusStep::Index { index: 0 }
            ]
        ));
    }
}
