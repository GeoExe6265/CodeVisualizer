use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::tracer::{FrameInfo, Incoming, Session, TracerMsg};
use crate::value::{Focus, FocusRole, ValueAnimations, ValueNode, is_data_structure, show_value};

const ACCENT: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);
const ACTION_GREEN: egui::Color32 = egui::Color32::from_rgb(110, 210, 140);
const SOURCE_RED: egui::Color32 = egui::Color32::from_rgb(240, 115, 115);
const CURRENT_LINE_BG: egui::Color32 = egui::Color32::from_rgb(38, 72, 112);
const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 120, 120);
const OK: egui::Color32 = egui::Color32::from_rgb(130, 210, 130);
const DEMO_SRC: &str = include_str!("../examples/demo.py");
const HISTORY_LIMIT: usize = 500;

fn is_python_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("py") || extension.eq_ignore_ascii_case("pyw")
        })
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Idle,
    Running,
    Paused,
    Finished,
    Stopped,
    Error(String),
}

impl Status {
    fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }

    fn is_paused(&self) -> bool {
        matches!(self, Self::Paused)
    }

    fn label(&self) -> (String, egui::Color32) {
        match self {
            Self::Idle => ("Готов".to_owned(), egui::Color32::GRAY),
            Self::Running => ("Выполняется…".to_owned(), ACCENT),
            Self::Paused => ("Пауза".to_owned(), OK),
            Self::Finished => ("Завершено".to_owned(), OK),
            Self::Stopped => (
                "Остановлено".to_owned(),
                egui::Color32::from_rgb(230, 180, 90),
            ),
            Self::Error(message) => (format!("Ошибка: {message}"), ERROR),
        }
    }
}

#[derive(Clone, Default)]
struct ViewSnapshot {
    current_line: Option<usize>,
    focus: Vec<Focus>,
    locals: Vec<(String, ValueNode)>,
    globals: Vec<(String, ValueNode)>,
    stack: Vec<FrameInfo>,
    output: String,
    last_exception: Option<String>,
}

/// Saved UI states let the user step back without trying to rewind the
/// Python process. Stepping forward through the saved states is local until
/// the newest state is reached, then the tracer resumes live execution.
#[derive(Default)]
struct ViewHistory {
    entries: VecDeque<ViewSnapshot>,
    cursor: usize,
}

impl ViewHistory {
    fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
    }

    fn push(&mut self, snapshot: ViewSnapshot) {
        if self.cursor + 1 < self.entries.len() {
            self.entries.truncate(self.cursor + 1);
        }
        self.entries.push_back(snapshot);
        if self.entries.len() > HISTORY_LIMIT {
            self.entries.pop_front();
        }
        self.cursor = self.entries.len().saturating_sub(1);
    }

    fn back(&mut self) -> Option<ViewSnapshot> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).cloned()
    }

    fn forward(&mut self) -> Option<ViewSnapshot> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor).cloned()
    }

    fn latest(&mut self) -> Option<ViewSnapshot> {
        if self.entries.is_empty() {
            return None;
        }
        self.cursor = self.entries.len() - 1;
        self.entries.back().cloned()
    }

    fn update_latest_output(&mut self, output: String) {
        if let Some(latest) = self.entries.back_mut() {
            latest.output = output;
        }
    }

    fn has_back(&self) -> bool {
        self.cursor > 0
    }

    fn has_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    fn position(&self) -> usize {
        self.cursor + 1
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn draw_drop_overlay(ctx: &egui::Context, files: &[PathBuf]) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("file-drop-overlay"),
    ));
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(200));

    let border = screen.shrink(18.0);
    painter.rect_stroke(
        border,
        12.0,
        egui::Stroke::new(3.0, ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.text(
        screen.center() - egui::vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        "Отпустите файл, чтобы открыть его",
        egui::FontId::proportional(26.0),
        egui::Color32::WHITE,
    );

    let supported = files.iter().find(|path| is_python_source(path));
    let detail = if let Some(path) = supported {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    } else {
        "Поддерживаются файлы Python: .py и .pyw".to_owned()
    };
    painter.text(
        screen.center() + egui::vec2(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        detail,
        egui::FontId::proportional(16.0),
        egui::Color32::from_gray(205),
    );
}

pub struct App {
    script_path: Option<PathBuf>,
    code: Vec<String>,
    current_line: Option<usize>,
    focus: Vec<Focus>,
    only_structures: bool,
    locals: Vec<(String, ValueNode)>,
    globals: Vec<(String, ValueNode)>,
    stack: Vec<FrameInfo>,
    output: String,
    last_exception: Option<String>,
    status: Status,
    session: Option<Session>,
    show_globals: bool,
    scrolled_line: Option<usize>,
    auto_steps: usize,
    program_input: String,
    path_input: String,
    animations: ValueAnimations,
    history: ViewHistory,
    ctx: egui::Context,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut app = Self {
            script_path: None,
            code: Vec::new(),
            current_line: None,
            focus: Vec::new(),
            only_structures: true,
            locals: Vec::new(),
            globals: Vec::new(),
            stack: Vec::new(),
            output: String::new(),
            last_exception: None,
            status: Status::Idle,
            session: None,
            show_globals: false,
            scrolled_line: None,
            auto_steps: 0,
            program_input: String::new(),
            path_input: String::new(),
            animations: ValueAnimations::default(),
            history: ViewHistory::default(),
            ctx,
        };

        if let Some(path) = std::env::args().nth(1) {
            app.load_file(PathBuf::from(path));
        }

        if std::env::var_os("CODEVISUALIZER_DEMO").is_some() {
            app.load_demo();
        }

        // Debug hooks used by the screenshot/integration workflow.
        app.auto_steps = std::env::var("CODEVISUALIZER_AUTOSTEPS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        if app.script_path.is_some() && std::env::var_os("CODEVISUALIZER_AUTOSTART").is_some() {
            app.start();
        }

        app
    }

    fn load_file(&mut self, path: PathBuf) {
        self.stop_session();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.code = text
                    .lines()
                    .map(|line| line.replace('\t', "    "))
                    .collect();
                if self.code.is_empty() {
                    self.code.push(String::new());
                }
                self.script_path = Some(path);
                self.reset_view();
                self.status = Status::Idle;
            }
            Err(err) => {
                self.status = Status::Error(format!("не удалось прочитать файл: {err}"));
            }
        }
    }

    fn load_demo(&mut self) {
        let dir = std::env::temp_dir().join("codevisualizer");
        let path = dir.join("demo.py");
        let result = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, DEMO_SRC));
        match result {
            Ok(()) => self.load_file(path),
            Err(err) => {
                self.status = Status::Error(format!("не удалось создать демо-файл: {err}"));
            }
        }
    }

    fn reset_view(&mut self) {
        self.current_line = None;
        self.focus.clear();
        self.locals.clear();
        self.globals.clear();
        self.stack.clear();
        self.output.clear();
        self.last_exception = None;
        self.scrolled_line = None;
        self.animations.clear();
        self.history.clear();
    }

    fn start(&mut self) {
        let Some(path) = self.script_path.clone() else {
            return;
        };
        self.stop_session();
        self.reset_view();
        match Session::spawn(&path, &self.program_input, self.ctx.clone()) {
            Ok(session) => {
                self.session = Some(session);
                self.status = Status::Running;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.status = Status::Error(
                    "Python 3 не найден — установите python3 или используйте архив с python-runtime"
                        .to_owned(),
                );
            }
            Err(err) => {
                self.status = Status::Error(format!("не удалось запустить Python: {err}"));
            }
        }
    }

    fn snapshot(&self) -> ViewSnapshot {
        ViewSnapshot {
            current_line: self.current_line,
            focus: self.focus.clone(),
            locals: self.locals.clone(),
            globals: self.globals.clone(),
            stack: self.stack.clone(),
            output: self.output.clone(),
            last_exception: self.last_exception.clone(),
        }
    }

    fn remember_view(&mut self) {
        self.history.push(self.snapshot());
    }

    fn apply_snapshot(&mut self, snapshot: ViewSnapshot) {
        self.current_line = snapshot.current_line;
        self.focus = snapshot.focus;
        self.locals = snapshot.locals;
        self.globals = snapshot.globals;
        self.stack = snapshot.stack;
        self.output = snapshot.output;
        self.last_exception = snapshot.last_exception;
        self.scrolled_line = None;
    }

    fn can_go_back(&self) -> bool {
        self.history.has_back() && !matches!(self.status, Status::Running)
    }

    fn can_go_forward(&self) -> bool {
        self.history.has_forward() || (self.status.is_paused() && self.session.is_some())
    }

    fn can_continue(&self) -> bool {
        self.status.is_paused() && self.session.is_some()
    }

    fn go_back(&mut self) {
        if !self.can_go_back() {
            return;
        }
        if let Some(snapshot) = self.history.back() {
            self.apply_snapshot(snapshot);
        }
    }

    fn step(&mut self) {
        // While reviewing saved states, "Шаг" moves through the history first.
        // This makes back/forward safe and instant without restarting Python.
        if let Some(snapshot) = self.history.forward() {
            self.apply_snapshot(snapshot);
            return;
        }

        if let Some(session) = &mut self.session
            && session.send("step").is_ok()
        {
            self.status = Status::Running;
            self.focus.clear();
        }
    }

    fn continue_run(&mut self) {
        if !self.can_continue() {
            return;
        }

        // "Продолжить" resumes the live process. If the user is inspecting
        // older states, first return the view to the live pause point.
        if self.history.has_forward()
            && let Some(snapshot) = self.history.latest()
        {
            self.apply_snapshot(snapshot);
        }

        if let Some(session) = &mut self.session
            && session.send("continue").is_ok()
        {
            self.status = Status::Running;
            self.focus.clear();
        }
    }

    fn handle_dropped_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }

        if let Some(path) = paths.into_iter().find(|path| is_python_source(path)) {
            self.load_file(path);
            return;
        }

        self.append_output(
            "[CodeVisualizer] файл не открыт: перетащите файл Python (.py или .pyw)\n",
        );
    }

    fn stop_session(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.kill();
        }
    }

    fn poll(&mut self) {
        let mut messages = Vec::new();
        if let Some(session) = &self.session {
            while let Ok(message) = session.rx.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            self.handle_message(message);
        }
    }

    fn handle_message(&mut self, message: Incoming) {
        match message {
            Incoming::Msg(TracerMsg::Event(event)) => {
                self.focus = event.focus;
                if let Some(line) = event.line {
                    self.current_line = Some(line);
                }
                self.locals = event.locals;
                self.globals = event.globals;
                self.stack = event.stack;
                self.append_output(&event.output);

                if let Some(exception) = event.exception {
                    self.last_exception = Some(exception);
                }
                self.remember_view();
                if event.kind == "line" {
                    self.status = Status::Paused;
                    if self.auto_steps > 0 {
                        self.auto_steps -= 1;
                        self.step();
                    }
                }
            }
            Incoming::Msg(TracerMsg::Finished {
                output,
                error,
                stopped,
            }) => {
                self.append_output(&output);
                // The last saved pause point should show the final output too
                // when the user reviews history after the program has ended.
                self.history.update_latest_output(self.output.clone());
                self.session = None;
                self.status = if let Some(error) = error {
                    Status::Error(error)
                } else if stopped {
                    Status::Stopped
                } else {
                    Status::Finished
                };
            }
            Incoming::Msg(TracerMsg::Error { message }) => {
                self.session = None;
                self.status = Status::Error(message);
            }
            Incoming::Stderr(text) => {
                self.append_output(&format!("[stderr] {text}\n"));
            }
            Incoming::Eof => {
                if self.session.take().is_some()
                    && !matches!(
                        self.status,
                        Status::Finished | Status::Stopped | Status::Error(_)
                    )
                {
                    self.status = Status::Error("процесс Python завершился неожиданно".to_owned());
                }
            }
        }
    }

    fn append_output(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.output.push_str(text);
        if self.output.chars().count() > 100_000 {
            let skip = self.output.chars().count() - 100_000;
            self.output = self.output.chars().skip(skip).collect();
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Открыть…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Python", &["py", "pyw"])
                    .pick_file()
            {
                self.load_file(path);
            }

            if ui.button("Демо").clicked() {
                self.load_demo();
            }

            ui.separator();

            let has_file = self.script_path.is_some();
            if ui
                .add_enabled(
                    has_file && !self.status.is_active(),
                    egui::Button::new("Запустить"),
                )
                .clicked()
            {
                self.start();
            }

            if ui
                .add_enabled(self.can_go_back(), egui::Button::new("Назад"))
                .on_hover_text("Вернуться к предыдущей сохранённой остановке")
                .clicked()
            {
                self.go_back();
            }

            if ui
                .add_enabled(self.can_go_forward(), egui::Button::new("Шаг"))
                .on_hover_text(if self.history.has_forward() {
                    "Перейти к следующей сохранённой остановке"
                } else {
                    "Выполнить текущую строку"
                })
                .clicked()
            {
                self.step();
            }

            if ui
                .add_enabled(self.can_continue(), egui::Button::new("Продолжить"))
                .on_hover_text(if self.history.has_forward() {
                    "Вернуться к текущему состоянию и продолжить выполнение"
                } else {
                    "Выполнить программу до конца"
                })
                .clicked()
            {
                self.continue_run();
            }

            if ui
                .add_enabled(self.status.is_active(), egui::Button::new("Стоп"))
                .clicked()
            {
                self.stop_session();
                self.focus.clear();
                self.status = Status::Stopped;
            }

            ui.separator();

            if let Some(path) = &self.script_path {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                ui.label(egui::RichText::new(name).strong());
                ui.add(
                    egui::Label::new(egui::RichText::new(path.display().to_string()).weak())
                        .truncate(),
                );
            } else {
                ui.weak("файл не выбран — откройте .py/.pyw или перетащите его в окно");
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (text, color) = if self.history.has_forward() {
                (
                    format!(
                        "Просмотр истории: {}/{}",
                        self.history.position(),
                        self.history.len()
                    ),
                    ACCENT,
                )
            } else {
                self.status.label()
            };
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate());

            if let Some(line) = self.current_line {
                ui.separator();
                ui.monospace(format!("строка {line}"));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("Python 3 · sys.settrace");
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let mut show_globals = self.show_globals;
        let mut only_structures = self.only_structures;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(exception) = &self.last_exception {
                    ui.colored_label(ERROR, exception);
                    ui.separator();
                }

                egui::CollapsingHeader::new("Переменные")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut show_globals, false, "Локальные");
                            ui.selectable_value(&mut show_globals, true, "Глобальные");
                        });
                        ui.checkbox(&mut only_structures, "только массивы и словари");
                        if only_structures {
                            ui.weak("скаляры и вспомогательные переменные скрыты");
                        }

                        if !self.focus.is_empty() {
                            let primary_index = self
                                .focus
                                .iter()
                                .position(|focus| focus.role == FocusRole::Primary)
                                .unwrap_or(0);
                            ui.horizontal_wrapped(|ui| {
                                ui.weak("Действие:");
                                for (index, focus) in self.focus.iter().enumerate() {
                                    if index > 0 {
                                        ui.weak(",");
                                    }
                                    if index == primary_index {
                                        ui.colored_label(ACTION_GREEN, "->");
                                        ui.colored_label(ACTION_GREEN, focus.label.as_str());
                                    } else if focus.role == FocusRole::Source {
                                        ui.colored_label(SOURCE_RED, focus.label.as_str());
                                    } else {
                                        ui.colored_label(ACTION_GREEN, focus.label.as_str());
                                    }
                                }
                            });
                        }
                        ui.add_space(4.0);

                        let variables = if show_globals {
                            &self.globals
                        } else {
                            &self.locals
                        };
                        let visible: Vec<(&String, &ValueNode)> = variables
                            .iter()
                            .filter(|(_, value)| !only_structures || is_data_structure(value))
                            .map(|(name, value)| (name, value))
                            .collect();

                        if visible.is_empty() {
                            if only_structures && !variables.is_empty() {
                                ui.weak("нет массивов или словарей");
                            } else {
                                ui.weak("нет данных");
                            }
                        } else {
                            for (name, value) in visible {
                                let mut path = Vec::new();
                                show_value(
                                    ui,
                                    name,
                                    value,
                                    name,
                                    &mut path,
                                    &self.focus,
                                    &mut self.animations,
                                );
                            }
                        }
                    });

                egui::CollapsingHeader::new(format!("Стек вызовов ({})", self.stack.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        if self.stack.is_empty() {
                            ui.weak("нет данных");
                        } else {
                            let last = self.stack.len().saturating_sub(1);
                            for (index, frame) in self.stack.iter().enumerate().rev() {
                                ui.horizontal(|ui| {
                                    if index == last {
                                        ui.colored_label(ACCENT, ">");
                                    } else {
                                        ui.label("  ");
                                    }
                                    ui.monospace(frame.func.as_str());
                                    ui.weak(format!("строка {}", frame.line));
                                });
                            }
                        }
                    });

                egui::CollapsingHeader::new("Ввод программы")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Данные для input() и sys.stdin:");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.program_input)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY)
                                .hint_text("3\n1 2 3"),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Очистить").clicked() {
                                self.program_input.clear();
                            }
                            ui.weak("применяется при следующем запуске");
                        });
                    });

                egui::CollapsingHeader::new("Вывод")
                    .default_open(true)
                    .show(ui, |ui| {
                        if self.output.is_empty() {
                            ui.weak("—");
                        } else {
                            ui.monospace(self.output.as_str());
                        }
                    });
            });

        self.show_globals = show_globals;
        self.only_structures = only_structures;
    }

    fn code_panel(&mut self, ui: &mut egui::Ui) {
        if self.code.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.heading("CodeVisualizer");
                    ui.label("Пошаговая визуализация выполнения Python-кода");
                    ui.add_space(8.0);
                    ui.label("Откройте .py/.pyw файл, перетащите его в окно или запустите демо.");
                    ui.add_space(10.0);
                    ui.label("Если диалог выбора файла недоступен, вставьте путь:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.path_input)
                                .hint_text("/путь/к/script.py")
                                .desired_width(380.0),
                        );
                        if ui.button("Открыть путь").clicked() {
                            let path = PathBuf::from(self.path_input.trim());
                            if !path.as_os_str().is_empty() {
                                self.load_file(path);
                            }
                        }
                    });
                    ui.add_space(12.0);
                    if ui.button("Загрузить демо").clicked() {
                        self.load_demo();
                    }
                    ui.add_space(8.0);
                    ui.weak("Для задач с input() заполните «Ввод программы» справа.");
                });
            });
            return;
        }

        let current_line = self.current_line;
        let mut scrolled_line = self.scrolled_line;
        let code = &self.code;
        let line_height = ui.text_style_height(&egui::TextStyle::Monospace);

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut current_rect = None;

                for (index, line) in code.iter().enumerate() {
                    let number = index + 1;
                    let is_current = current_line == Some(number);
                    let fill = if is_current {
                        CURRENT_LINE_BG
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let response = egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [42.0, line_height],
                                    egui::Label::new(
                                        egui::RichText::new(format!("{number:>4}"))
                                            .monospace()
                                            .color(egui::Color32::from_gray(110)),
                                    ),
                                );
                                ui.label(
                                    egui::RichText::new(if line.is_empty() {
                                        " "
                                    } else {
                                        line.as_str()
                                    })
                                    .monospace(),
                                );
                            });
                        });

                    if is_current {
                        current_rect = Some(response.response.rect);
                    }
                }

                if scrolled_line != current_line
                    && let Some(rect) = current_rect
                {
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    scrolled_line = current_line;
                }
            });

        self.scrolled_line = scrolled_line;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ui.ctx().input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        self.handle_dropped_files(dropped);

        self.poll();

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("side")
            .default_size(430.0)
            .show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.code_panel(ui));

        let hovered: Vec<PathBuf> = ui.ctx().input(|input| {
            input
                .raw
                .hovered_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !hovered.is_empty() {
            draw_drop_overlay(ui.ctx(), &hovered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(line: usize) -> ViewSnapshot {
        ViewSnapshot {
            current_line: Some(line),
            ..ViewSnapshot::default()
        }
    }

    fn test_app() -> App {
        App {
            script_path: None,
            code: Vec::new(),
            current_line: None,
            focus: Vec::new(),
            only_structures: true,
            locals: Vec::new(),
            globals: Vec::new(),
            stack: Vec::new(),
            output: String::new(),
            last_exception: None,
            status: Status::Paused,
            session: None,
            show_globals: false,
            scrolled_line: None,
            auto_steps: 0,
            program_input: String::new(),
            path_input: String::new(),
            animations: ValueAnimations::default(),
            history: ViewHistory::default(),
            ctx: egui::Context::default(),
        }
    }

    #[test]
    fn app_back_and_step_restore_saved_views() {
        let mut app = test_app();
        for (line, output) in [(1, "a"), (2, "ab"), (3, "abc")] {
            app.history.push(ViewSnapshot {
                current_line: Some(line),
                output: output.to_owned(),
                ..ViewSnapshot::default()
            });
        }
        app.history.latest();

        app.current_line = Some(3);
        app.output = "abc".to_owned();
        app.go_back();
        assert_eq!(app.current_line, Some(2));
        assert_eq!(app.output, "ab");

        app.go_back();
        assert_eq!(app.current_line, Some(1));
        assert_eq!(app.output, "a");

        app.step();
        assert_eq!(app.current_line, Some(2));
        assert_eq!(app.output, "ab");
    }

    #[test]
    fn history_moves_back_then_truncates_when_stepping_again() {
        let mut history = ViewHistory::default();
        history.push(snapshot(1));
        history.push(snapshot(2));
        history.push(snapshot(3));

        assert_eq!(history.back().unwrap().current_line, Some(2));
        assert_eq!(history.back().unwrap().current_line, Some(1));
        assert!(history.back().is_none());
        assert_eq!(history.forward().unwrap().current_line, Some(2));

        history.push(snapshot(9));
        assert_eq!(history.len(), 3);
        assert_eq!(history.position(), 3);
        assert!(!history.has_forward());
        assert_eq!(history.latest().unwrap().current_line, Some(9));
    }

    #[test]
    fn finished_output_updates_latest_snapshot() {
        let mut history = ViewHistory::default();
        history.push(snapshot(1));
        history.push(snapshot(2));
        history.update_latest_output("result\n".to_owned());

        assert_eq!(history.latest().unwrap().output, "result\n");
        assert_eq!(history.back().unwrap().output, "");
    }

    #[test]
    fn accepts_python_source_extensions() {
        assert!(is_python_source(Path::new("script.py")));
        assert!(is_python_source(Path::new("script.PY")));
        assert!(is_python_source(Path::new("script.pyw")));
        assert!(!is_python_source(Path::new("script.txt")));
        assert!(!is_python_source(Path::new("script")));
        assert!(!is_python_source(Path::new("py")));
    }

    #[test]
    fn dropped_python_file_is_loaded() {
        let dir = std::env::temp_dir().join("codevisualizer-test-drop");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("drop-{}.py", std::process::id()));
        std::fs::write(&path, "x = 1\n").unwrap();

        let mut app = test_app();
        app.handle_dropped_files(vec![PathBuf::from("notes.txt"), path.clone()]);

        assert_eq!(app.code, vec!["x = 1".to_owned()]);
        assert_eq!(app.script_path.as_deref(), Some(path.as_path()));
        assert_eq!(app.status, Status::Idle);
        let _ = std::fs::remove_file(path);
    }
}
