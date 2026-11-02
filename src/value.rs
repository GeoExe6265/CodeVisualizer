use serde::Deserialize;

const FOCUS_ARROW: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);
const FOCUS_ELEMENT_BG: egui::Color32 = egui::Color32::from_rgb(43, 79, 119);
const FOCUS_ELEMENT_BORDER: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);

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

/// Whether the value is a list/array-like structure, a dict, or an object
/// that contains such structures. Scalar variables are hidden in the default
/// "structures only" view.
pub fn is_data_structure(value: &ValueNode) -> bool {
    matches!(value.type_name.as_str(), "list" | "tuple" | "dict")
        || value
            .entries
            .iter()
            .any(|(_, item)| is_data_structure(item))
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
        let row = |ui: &mut egui::Ui| {
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
        };

        if focused {
            egui::Frame::NONE
                .fill(FOCUS_ELEMENT_BG)
                .stroke(egui::Stroke::new(1.0, FOCUS_ELEMENT_BORDER))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(4, 1))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    row(ui);
                });
        } else {
            row(ui);
        }
        return;
    }

    // Dicts are shown whole. For custom objects, only attributes that are
    // data structures are shown, so helper fields do not clutter the panel.
    let show_all_entries = value.type_name == "dict";
    let visible_entries: Vec<(&ValueNode, &ValueNode)> = value
        .entries
        .iter()
        .filter(|(_, item)| show_all_entries || is_data_structure(item))
        .map(|(key, item)| (key, item))
        .collect();
    let hidden_entries = value.entries.len().saturating_sub(visible_entries.len());
    let visible_count = visible_entries.len();
    let shown = value.items.len() + visible_count;
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

        for (key, item) in visible_entries {
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

        if hidden_entries > 0 {
            ui.weak(format!("… скрыто атрибутов: {hidden_entries}"));
        }
        if matches!(value.type_name.as_str(), "list" | "tuple" | "dict") && value.count() > shown {
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
    let background = if focused {
        FOCUS_ELEMENT_BG
    } else {
        egui::Color32::TRANSPARENT
    };
    let mut job = egui::text::LayoutJob::default();
    if focused {
        job.append(
            "-> ",
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: FOCUS_ARROW,
                background,
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
            background,
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

    #[test]
    fn structures_only_keeps_containers() {
        let scalar = ValueNode {
            type_name: "int".to_owned(),
            repr: "1".to_owned(),
            ..Default::default()
        };
        let list = ValueNode {
            type_name: "list".to_owned(),
            items: vec![scalar.clone()],
            ..Default::default()
        };
        let dict = ValueNode {
            type_name: "dict".to_owned(),
            entries: vec![(scalar.clone(), scalar.clone())],
            ..Default::default()
        };
        let object = ValueNode {
            type_name: "Box".to_owned(),
            entries: vec![
                (scalar.clone(), list.clone()),
                (scalar.clone(), scalar.clone()),
            ],
            ..Default::default()
        };

        assert!(!is_data_structure(&scalar));
        assert!(is_data_structure(&list));
        assert!(is_data_structure(&dict));
        assert!(is_data_structure(&object));
    }
}
