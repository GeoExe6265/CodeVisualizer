use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Deserialize;

const ACTION_ARROW: egui::Color32 = egui::Color32::from_rgb(110, 210, 140);
const ACTION_ELEMENT_BG: egui::Color32 = egui::Color32::from_rgb(43, 92, 61);
const ACTION_ELEMENT_BORDER: egui::Color32 = egui::Color32::from_rgb(110, 210, 140);
const SOURCE_ELEMENT_BG: egui::Color32 = egui::Color32::from_rgb(104, 48, 53);
const SOURCE_ELEMENT_BORDER: egui::Color32 = egui::Color32::from_rgb(240, 115, 115);
const MOVE_ANIMATION_SECONDS: f32 = 0.5;

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemKey {
    type_name: String,
    repr: String,
}

impl From<&ValueNode> for ItemKey {
    fn from(value: &ValueNode) -> Self {
        Self {
            type_name: value.type_name.clone(),
            repr: value.repr.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct ListMotion {
    previous: Vec<ItemKey>,
    current: Vec<ItemKey>,
    started_at: f64,
}

impl ListMotion {
    fn new(keys: Vec<ItemKey>, now: f64) -> Self {
        Self {
            previous: keys.clone(),
            current: keys,
            started_at: now - f64::from(MOVE_ANIMATION_SECONDS),
        }
    }

    fn observe(&mut self, keys: Vec<ItemKey>, now: f64) {
        if keys != self.current {
            self.previous = std::mem::replace(&mut self.current, keys);
            self.started_at = now;
        }
    }

    fn progress(&self, now: f64) -> f32 {
        (((now - self.started_at) as f32) / MOVE_ANIMATION_SECONDS).clamp(0.0, 1.0)
    }
}

/// Animation state for reordered list rows. It is keyed by the path of each
/// expanded list, so a matrix can animate every one of its inner lists.
#[derive(Default)]
pub struct ValueAnimations {
    lists: HashMap<String, ListMotion>,
}

impl ValueAnimations {
    pub fn clear(&mut self) {
        self.lists.clear();
    }

    fn motion(&mut self, key: String, keys: Vec<ItemKey>, now: f64) -> ListMotion {
        let motion = self
            .lists
            .entry(key)
            .or_insert_with(|| ListMotion::new(keys.clone(), now));
        motion.observe(keys, now);
        if motion.progress(now) >= 1.0 {
            motion.previous.clone_from(&motion.current);
        }
        motion.clone()
    }
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
    #[serde(default)]
    pub role: FocusRole,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FocusRole {
    Primary,
    Source,
    #[default]
    Peer,
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

fn focus_role(focus: &[Focus], root: &str, path: &[FocusStep]) -> Option<FocusRole> {
    focus
        .iter()
        .find(|target| target.matches(root, path))
        .map(|target| target.role)
}

fn animation_key(root: &str, path: &[FocusStep]) -> String {
    format!("{root}|{path:?}")
}

fn match_positions(previous: &[ItemKey], current: &[ItemKey]) -> Vec<Option<usize>> {
    let mut used = vec![false; previous.len()];
    current
        .iter()
        .map(|key| {
            let found = previous
                .iter()
                .enumerate()
                .find(|(index, old)| !used[*index] && *old == key)
                .map(|(index, _)| index);
            if let Some(index) = found {
                used[index] = true;
            }
            found
        })
        .collect()
}

fn ease_in_out(progress: f32) -> f32 {
    progress * progress * (3.0 - 2.0 * progress)
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
    animations: &mut ValueAnimations,
) {
    let role = focus_role(focus, root, path);

    if !value.is_container() {
        show_scalar_row(ui, name, value, role, false);
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

    // Animate simple rows only. Nested matrices keep their existing tree
    // rendering; their inner scalar lists animate independently.
    let animate_items = matches!(value.type_name.as_str(), "list" | "tuple")
        && !value.items.is_empty()
        && value.entries.is_empty()
        && value.items.iter().all(|item| !item.is_container());
    let motion = if animate_items {
        let now = ui.input(|input| input.time);
        let keys = value.items.iter().map(ItemKey::from).collect();
        Some(animations.motion(animation_key(root, path), keys, now))
    } else {
        None
    };

    egui::CollapsingHeader::new(header_widget(
        ui,
        header,
        type_color(&value.type_name),
        role,
    ))
    .default_open(true)
    .show(ui, |ui| {
        if let Some(motion) = &motion {
            show_animated_list(ui, value, root, path, focus, motion);
        } else {
            for (index, item) in value.items.iter().enumerate() {
                path.push(FocusStep::Index { index });
                show_value(
                    ui,
                    &format!("[{index}]"),
                    item,
                    root,
                    path,
                    focus,
                    animations,
                );
                path.pop();
            }
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
            show_value(ui, &key_name, item, root, path, focus, animations);
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

fn show_animated_list(
    ui: &mut egui::Ui,
    value: &ValueNode,
    root: &str,
    path: &mut Vec<FocusStep>,
    focus: &[Focus],
    motion: &ListMotion,
) {
    let now = ui.input(|input| input.time);
    let progress = motion.progress(now);
    if progress < 1.0 {
        ui.ctx().request_repaint();
    }
    let eased = ease_in_out(progress);
    let old_positions = match_positions(&motion.previous, &motion.current);

    let text_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let row_height = text_height + 2.0;
    let spacing = ui.spacing().item_spacing.y;
    let slot = row_height + spacing;
    let visual_len = value.items.len().max(motion.previous.len());
    let total_height = (slot * visual_len as f32 - spacing).max(0.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), total_height),
        egui::Sense::hover(),
    );

    let mut rows: Vec<(usize, f32, bool)> = value
        .items
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let old_index = old_positions.get(index).copied().flatten();
            let from = old_index.unwrap_or(index) as f32 * slot;
            let to = index as f32 * slot;
            let y = from + (to - from) * eased;
            let moving = old_index.is_some_and(|old| old != index) && progress < 1.0;
            (index, y, moving)
        })
        .collect();
    rows.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));

    for (index, y, moving) in rows {
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + y),
            egui::vec2(rect.width(), row_height),
        );
        path.push(FocusStep::Index { index });
        let role = focus_role(focus, root, path);
        let name = format!("[{index}]");
        let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row_rect));
        show_scalar_row(&mut row_ui, &name, &value.items[index], role, moving);
        path.pop();
    }
}

fn show_scalar_row(
    ui: &mut egui::Ui,
    name: &str,
    value: &ValueNode,
    role: Option<FocusRole>,
    occlude: bool,
) {
    let show_pointer = role == Some(FocusRole::Primary);
    let row = |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            if show_pointer {
                ui.label(egui::RichText::new("->").color(ACTION_ARROW).strong());
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

    let highlight = match role {
        Some(FocusRole::Source) => Some((SOURCE_ELEMENT_BG, SOURCE_ELEMENT_BORDER)),
        Some(_) => Some((ACTION_ELEMENT_BG, ACTION_ELEMENT_BORDER)),
        None => None,
    };
    if let Some((fill, border)) = highlight {
        egui::Frame::NONE
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(4, 1))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                row(ui);
            });
    } else if occlude {
        egui::Frame::NONE
            .fill(ui.visuals().panel_fill)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                row(ui);
            });
    } else {
        row(ui);
    }
}

fn header_widget(
    ui: &egui::Ui,
    text: String,
    color: egui::Color32,
    role: Option<FocusRole>,
) -> egui::WidgetText {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let show_pointer = role == Some(FocusRole::Primary);
    let background = match role {
        Some(FocusRole::Source) => SOURCE_ELEMENT_BG,
        Some(_) => ACTION_ELEMENT_BG,
        None => egui::Color32::TRANSPARENT,
    };
    let mut job = egui::text::LayoutJob::default();
    if show_pointer {
        job.append(
            "-> ",
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: ACTION_ARROW,
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
            role: FocusRole::Primary,
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

    fn item_key(repr: &str) -> ItemKey {
        ItemKey {
            type_name: "int".to_owned(),
            repr: repr.to_owned(),
        }
    }

    #[test]
    fn matches_reordered_rows_by_value() {
        let previous = vec![item_key("10"), item_key("20"), item_key("30")];
        let current = vec![item_key("30"), item_key("20"), item_key("10")];

        assert_eq!(
            match_positions(&previous, &current),
            [Some(2), Some(1), Some(0)]
        );
    }

    #[test]
    fn matches_duplicate_rows_without_reusing_an_item() {
        let previous = vec![item_key("1"), item_key("2"), item_key("1")];
        let current = vec![item_key("1"), item_key("1"), item_key("2")];

        assert_eq!(
            match_positions(&previous, &current),
            [Some(0), Some(2), Some(1)]
        );
    }

    #[test]
    fn starts_motion_when_order_changes() {
        let previous = vec![item_key("10"), item_key("20"), item_key("30")];
        let current = vec![item_key("30"), item_key("20"), item_key("10")];
        let mut motion = ListMotion::new(previous.clone(), 10.0);
        assert_eq!(motion.progress(10.0), 1.0);

        motion.observe(current.clone(), 20.0);

        assert_eq!(motion.previous, previous);
        assert_eq!(motion.current, current);
        assert_eq!(motion.progress(20.0), 0.0);
        assert!(motion.progress(20.2) > 0.0);
        assert_eq!(motion.progress(30.0), 1.0);
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
