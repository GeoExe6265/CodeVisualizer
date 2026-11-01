#!/usr/bin/env python3
"""Trace backend for CodeVisualizer.

Runs a Python script, pauses at every executed line and reports the current
state (line, call stack, local and global variables, captured output) to the
Rust UI as JSON lines on stdout.

Commands are read from stdin, one per line:
    step      - run until the next line and pause again
    continue  - run until the program finishes
    stop      - abort execution
"""

import io
import json
import os
import sys
import traceback

MAX_ITEMS = 50
MAX_DEPTH = 4
MAX_REPR = 300
MAX_VARS = 100

if len(sys.argv) < 2:
    print("usage: tracer.py <script.py>", file=sys.stderr)
    raise SystemExit(2)

TARGET = os.path.abspath(sys.argv[1])
PROTO = sys.__stdout__
COMMANDS = sys.__stdin__


class StopExecution(BaseException):
    pass


class Capture(io.TextIOBase):
    def __init__(self):
        self.parts = []

    def write(self, text):
        self.parts.append(text)
        return len(text)

    def flush(self):
        pass


capture = Capture()
output_sent = 0
stepping = True


def emit(payload):
    try:
        PROTO.write(json.dumps(payload, ensure_ascii=False) + "\n")
        PROTO.flush()
    except OSError:
        raise StopExecution()


def output_delta():
    global output_sent
    text = "".join(capture.parts)
    if len(text) > output_sent:
        chunk = text[output_sent:]
        output_sent = len(text)
        return chunk
    return ""


def shorten(text):
    if len(text) > MAX_REPR:
        return text[:MAX_REPR] + "…"
    return text


def safe_repr(value):
    try:
        return shorten(repr(value))
    except Exception:
        return f"<{type(value).__name__}: repr() завершился ошибкой>"


def encode(value, depth=0, seen=None):
    if seen is None:
        seen = set()
    type_name = type(value).__name__

    if value is None or isinstance(value, (bool, int, float, complex, str, bytes, bytearray)):
        return {"t": type_name, "repr": safe_repr(value)}

    if isinstance(value, (list, tuple, set, frozenset)):
        if id(value) in seen:
            return {"t": type_name, "repr": f"<{type_name}, циклическая ссылка>"}
        if depth >= MAX_DEPTH:
            return {"t": type_name, "repr": f"<{type_name}, длина {len(value)}>"}
        inner = seen | {id(value)}
        items = [encode(item, depth + 1, inner) for item in list(value)[:MAX_ITEMS]]
        return {"t": type_name, "len": len(value), "items": items}

    if isinstance(value, dict):
        if id(value) in seen:
            return {"t": type_name, "repr": "<dict, циклическая ссылка>"}
        if depth >= MAX_DEPTH:
            return {"t": type_name, "repr": f"<dict, длина {len(value)}>"}
        inner = seen | {id(value)}
        entries = [
            [encode(key, depth + 1, inner), encode(item, depth + 1, inner)]
            for key, item in list(value.items())[:MAX_ITEMS]
        ]
        return {"t": type_name, "len": len(value), "entries": entries}

    if callable(value) or isinstance(value, (type, type(sys))):
        name = getattr(value, "__name__", "")
        return {"t": type_name, "repr": f"<{type_name} {name}>".strip()}

    return {"t": type_name, "repr": safe_repr(value)}


def visible(items):
    result = []
    for name, value in items:
        if name.startswith("__"):
            continue
        if callable(value) or isinstance(value, (type, type(sys))):
            continue
        try:
            encoded = encode(value)
        except Exception:
            encoded = {"t": type(value).__name__, "repr": "<ошибка сериализации>"}
        result.append((name, encoded))
        if len(result) >= MAX_VARS:
            break
    return result


def stack_frames(frame):
    frames = []
    current = frame
    while current is not None:
        if os.path.abspath(current.f_code.co_filename) == TARGET:
            frames.append({"func": current.f_code.co_name, "line": current.f_lineno})
        current = current.f_back
    frames.reverse()
    return frames


def snapshot(frame, kind):
    return {
        "type": "event",
        "kind": kind,
        "line": frame.f_lineno,
        "stack": stack_frames(frame),
        "locals": visible(frame.f_locals.items()),
        "globals": visible(frame.f_globals.items()),
        "output": output_delta(),
    }


def wait_for_command():
    line = COMMANDS.readline()
    if not line:
        raise StopExecution()
    command = line.strip()
    if command == "stop":
        raise StopExecution()
    return command


def tracer(frame, event, arg):
    global stepping

    if os.path.abspath(frame.f_code.co_filename) != TARGET:
        return None

    if event == "line":
        if stepping:
            emit(snapshot(frame, "line"))
            command = wait_for_command()
            if command == "continue":
                stepping = False
        return tracer

    return tracer


def error_from_traceback(exc):
    """Emit a final snapshot at the line that raised an uncaught exception."""
    error_text = "".join(traceback.format_exception_only(type(exc), exc)).strip()
    frame = None
    tb = exc.__traceback__
    while tb is not None:
        if os.path.abspath(tb.tb_frame.f_code.co_filename) == TARGET:
            frame = tb.tb_frame
        tb = tb.tb_next

    if frame is not None:
        payload = snapshot(frame, "exception")
        payload["exception"] = error_text
        emit(payload)

    return error_text


def main():
    try:
        with open(TARGET, "r", encoding="utf-8") as handle:
            source = handle.read()
    except OSError as exc:
        emit({"type": "error", "message": f"Не удалось прочитать файл: {exc}"})
        return 1

    namespace = {
        "__name__": "__main__",
        "__file__": TARGET,
        "__builtins__": __builtins__,
    }

    sys.stdout = capture
    sys.stderr = capture
    sys.stdin = io.StringIO("")

    try:
        code = compile(source, TARGET, "exec")
    except SyntaxError as exc:
        message = "".join(traceback.format_exception_only(type(exc), exc)).strip()
        emit({"type": "error", "message": message})
        return 1

    sys.settrace(tracer)
    try:
        exec(code, namespace)
        status = {"type": "finished", "output": output_delta()}
    except StopExecution:
        status = {"type": "finished", "output": output_delta(), "stopped": True}
    except BaseException as exc:
        status = {
            "type": "finished",
            "output": output_delta(),
            "error": error_from_traceback(exc),
        }
    finally:
        sys.settrace(None)

    emit(status)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
