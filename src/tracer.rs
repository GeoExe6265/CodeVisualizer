use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use serde::Deserialize;

use crate::value::ValueNode;

const TRACER_SRC: &str = include_str!("../python/tracer.py");
static SESSION_ID: AtomicU64 = AtomicU64::new(0);

fn python_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CODEVISUALIZER_PYTHON") {
        return PathBuf::from(path);
    }

    // Prefer the system interpreter: it has access to packages the user
    // installed with pip. The bundled runtime is the fallback for clean systems.
    if system_python_works() {
        return PathBuf::from("python3");
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let bundled = dir.join("python-runtime/bin/python3");
        if bundled.is_file() {
            return bundled;
        }
    }

    PathBuf::from("python3")
}

fn system_python_works() -> bool {
    Command::new("python3")
        .args([
            "-c",
            "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug, Clone, Deserialize)]
pub struct FrameInfo {
    #[serde(default)]
    pub func: String,
    #[serde(default)]
    pub line: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TracerEvent {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub line: Option<usize>,
    #[serde(default)]
    pub stack: Vec<FrameInfo>,
    #[serde(default)]
    pub locals: Vec<(String, ValueNode)>,
    #[serde(default)]
    pub globals: Vec<(String, ValueNode)>,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub exception: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum TracerMsg {
    #[serde(rename = "event")]
    Event(TracerEvent),
    #[serde(rename = "finished")]
    Finished {
        #[serde(default)]
        output: String,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        stopped: bool,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

pub enum Incoming {
    Msg(TracerMsg),
    Stderr(String),
    Eof,
}

pub struct Session {
    child: Child,
    stdin: ChildStdin,
    input_path: PathBuf,
    pub rx: Receiver<Incoming>,
}

impl Session {
    pub fn spawn(script: &Path, program_input: &str, ctx: egui::Context) -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join("codevisualizer");
        std::fs::create_dir_all(&dir)?;
        let tracer_path = dir.join("tracer.py");
        std::fs::write(&tracer_path, TRACER_SRC)?;

        let input_path = dir.join(format!(
            "input-{}-{}.txt",
            std::process::id(),
            SESSION_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let normalized_input = program_input.replace("\r\n", "\n");
        std::fs::write(&input_path, normalized_input)?;

        let spawn = Command::new(python_binary())
            .arg("-u")
            .arg(&tracer_path)
            .arg(script)
            .arg(&input_path)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawn {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::remove_file(&input_path);
                return Err(err);
            }
        };

        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (tx, rx) = mpsc::channel();

        let tx_out = tx.clone();
        let ctx_out = ctx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        match serde_json::from_str::<TracerMsg>(&line) {
                            Ok(msg) => {
                                let _ = tx_out.send(Incoming::Msg(msg));
                            }
                            Err(err) => {
                                let _ = tx_out.send(Incoming::Stderr(format!(
                                    "не удалось разобрать ответ трейсера: {err}: {line}"
                                )));
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
                ctx_out.request_repaint();
            }
            let _ = tx_out.send(Incoming::Eof);
            ctx_out.request_repaint();
        });

        let ctx_err = ctx;
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx.send(Incoming::Stderr(line));
                ctx_err.request_repaint();
            }
        });

        Ok(Self {
            child,
            stdin,
            input_path,
            rx,
        })
    }

    pub fn send(&mut self, command: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.input_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn steps_through_script() {
        let dir = std::env::temp_dir().join("codevisualizer-test");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("sample.py");
        std::fs::write(&script, "x = 1\ny = x + 2\nprint(y)\n").unwrap();

        let mut session = Session::spawn(&script, "", egui::Context::default()).unwrap();

        let first = next_event(&session);
        assert_eq!(first.kind, "line");
        assert_eq!(first.line, Some(1));

        session.send("step").unwrap();
        let second = next_event(&session);
        assert_eq!(second.line, Some(2));
        assert!(second.locals.iter().any(|(name, _)| name == "x"));

        session.send("continue").unwrap();
        loop {
            match session
                .rx
                .recv_timeout(Duration::from_secs(10))
                .expect("tracer timed out")
            {
                Incoming::Msg(TracerMsg::Finished { output, error, .. }) => {
                    assert!(error.is_none(), "unexpected error: {error:?}");
                    assert!(output.contains('3'), "missing output: {output:?}");
                    break;
                }
                Incoming::Msg(_) => {}
                Incoming::Stderr(line) => panic!("stderr: {line}"),
                Incoming::Eof => panic!("unexpected EOF"),
            }
        }
    }

    #[test]
    fn feeds_stdin_to_script() {
        let dir = std::env::temp_dir().join("codevisualizer-test-input");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo.py");
        std::fs::write(&script, "name = input()\nprint(name.upper())\n").unwrap();

        let mut session = Session::spawn(&script, "hello\n", egui::Context::default()).unwrap();

        let first = next_event(&session);
        assert_eq!(first.kind, "line");
        assert_eq!(first.line, Some(1));

        session.send("continue").unwrap();
        loop {
            match session
                .rx
                .recv_timeout(Duration::from_secs(10))
                .expect("tracer timed out")
            {
                Incoming::Msg(TracerMsg::Finished { output, error, .. }) => {
                    assert!(error.is_none(), "unexpected error: {error:?}");
                    assert!(output.contains("HELLO"), "missing output: {output:?}");
                    break;
                }
                Incoming::Msg(_) => {}
                Incoming::Stderr(line) => panic!("stderr: {line}"),
                Incoming::Eof => panic!("unexpected EOF"),
            }
        }
    }

    fn next_event(session: &Session) -> TracerEvent {
        match session
            .rx
            .recv_timeout(Duration::from_secs(10))
            .expect("tracer timed out")
        {
            Incoming::Msg(TracerMsg::Event(event)) => event,
            Incoming::Msg(TracerMsg::Finished { error, .. }) => {
                panic!("finished early: {error:?}")
            }
            Incoming::Msg(TracerMsg::Error { message }) => panic!("error: {message}"),
            Incoming::Stderr(line) => panic!("stderr: {line}"),
            Incoming::Eof => panic!("unexpected EOF"),
        }
    }
}
