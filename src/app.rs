use std::collections::HashSet;
use std::path::PathBuf;

use crate::tracer::{FrameInfo, Incoming, Session, TracerMsg};
use crate::value::{Focus, ValueNode, show_value};

const ACCENT: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);
const CURRENT_LINE_BG: egui::Color32 = egui::Color32::from_rgb(38, 72, 112);
const FOCUS_LINE_BG: egui::Color32 = egui::Color32::from_rgb(32, 62, 94);
const CURRENT_FOCUS_BG: egui::Color32 = egui::Color32::from_rgb(46, 92, 138);
const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 120, 120);
const OK: egui::Color32 = egui::Color32::from_rgb(130, 210, 130);
const DEMO_SRC: &str = include_str!("../examples/demo.py");

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

pub struct App {
    script_path: Option<PathBuf>,
    code: Vec<String>,
    current_line: Option<usize>,
    focus: Vec<Focus>,
    focus_lines: HashSet<usize>,
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
            focus_lines: HashSet::new(),
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
        self.focus_lines.clear();
        self.locals.clear();
        self.globals.clear();
        self.stack.clear();
        self.output.clear();
        self.last_exception = None;
        self.scrolled_line = None;
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

    fn step(&mut self) {
        if let Some(session) = &mut self.session
            && session.send("step").is_ok()
        {
            self.status = Status::Running;
            self.focus.clear();
        }
    }

    fn continue_run(&mut self) {
        if let Some(session) = &mut self.session
            && session.send("continue").is_ok()
        {
            self.status = Status::Running;
            self.focus.clear();
        }
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
                    if !self.focus.is_empty() {
                        self.focus_lines.insert(line);
                    }
                }
                self.locals = event.locals;
                self.globals = event.globals;
                self.stack = event.stack;
                self.append_output(&event.output);

                if let Some(exception) = event.exception {
                    self.last_exception = Some(exception);
                }
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
                    .add_filter("Python", &["py"])
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
                .add_enabled(self.status.is_paused(), egui::Button::new("Шаг"))
                .clicked()
            {
                self.step();
            }

            if ui
                .add_enabled(self.status.is_paused(), egui::Button::new("Продолжить"))
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
                ui.weak("файл не выбран — откройте .py или перетащите его в окно");
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (text, color) = self.status.label();
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

                        if !self.focus.is_empty() {
                            let labels = self
                                .focus
                                .iter()
                                .map(|focus| focus.label.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            ui.horizontal_wrapped(|ui| {
                                ui.weak("Элемент:");
                                ui.colored_label(ACCENT, "->");
                                ui.monospace(labels);
                            });
                        }
                        ui.add_space(4.0);

                        let variables = if show_globals {
                            &self.globals
                        } else {
                            &self.locals
                        };

                        if variables.is_empty() {
                            ui.weak("нет данных");
                        } else {
                            for (name, value) in variables {
                                let mut path = Vec::new();
                                show_value(ui, name, value, name, &mut path, &self.focus);
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
    }

    fn code_panel(&mut self, ui: &mut egui::Ui) {
        if self.code.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.heading("CodeVisualizer");
                    ui.label("Пошаговая визуализация выполнения Python-кода");
                    ui.add_space(8.0);
                    ui.label("Откройте .py файл, перетащите его в окно или запустите демо.");
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
        let focus_active = !self.focus.is_empty();
        let focus_lines = &self.focus_lines;
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
                    let has_focus = focus_lines.contains(&number);
                    let fill = if is_current {
                        if focus_active {
                            CURRENT_FOCUS_BG
                        } else {
                            CURRENT_LINE_BG
                        }
                    } else if has_focus {
                        FOCUS_LINE_BG
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

                    if has_focus {
                        let rect = response.response.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                            0.0,
                            ACCENT,
                        );
                    }

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
        if let Some(path) = dropped.into_iter().next() {
            self.load_file(path);
        }

        self.poll();

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("side")
            .default_size(430.0)
            .show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.code_panel(ui));
    }
}
