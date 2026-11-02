#!/usr/bin/env python3
"""Trace backend for CodeVisualizer.

Runs a Python script, pauses at every executed line and reports the current
state (line, call stack, local and global variables, captured output) to the
Rust UI as JSON lines on stdout.

The optional second argument is a UTF-8 file whose contents are exposed to the
script as stdin, so input() works. Commands are read from the real stdin pipe:
    step      - run until the next line and pause again
    continue  - run until the program finishes
    stop      - abort execution
"""

import ast
import io
import json
import os
import sys
import traceback

MAX_ITEMS = 50
MAX_DEPTH = 4
MAX_REPR = 300
MAX_VARS = 100
MAX_FOCUS = 4

SUBSCRIPT_NODES = []
METHOD_NODES = []
METHOD_NAMES = {
    "append",
    "clear",
    "extend",
    "get",
    "insert",
    "items",
    "keys",
    "pop",
    "popitem",
    "remove",
    "reverse",
    "setdefault",
    "sort",
    "update",
    "values",
}

if len(sys.argv) < 2:
    print("usage: tracer.py <script.py> [input.txt]", file=sys.stderr)
    raise SystemExit(2)

TARGET = os.path.abspath(sys.argv[1])
INPUT_PATH = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else None
PROTO = sys.__stdout__
COMMANDS = sys.__stdin__

# Allow the traced script to import modules that live next to it. When Python
# runs tracer.py, sys.path[0] points at the tracer directory instead.
SCRIPT_DIR = os.path.dirname(TARGET)
if SCRIPT_DIR and SCRIPT_DIR not in sys.path:
    sys.path.insert(0, SCRIPT_DIR)


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

    try:
        attributes = vars(value)
    except TypeError:
        attributes = None

    if attributes:
        if id(value) in seen:
            return {"t": type_name, "repr": f"<{type_name}, циклическая ссылка>"}
        if depth >= MAX_DEPTH:
            return {"t": type_name, "repr": f"<{type_name}, атрибутов {len(attributes)}>"}
        inner = seen | {id(value)}
        entries = [
            [{"t": "str", "repr": name}, encode(item, depth + 1, inner)]
            for name, item in list(attributes.items())[:MAX_ITEMS]
        ]
        return {"t": type_name, "entries": entries}

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


class Unresolved(Exception):
    pass


def lookup_name(name, frame):
    if name in frame.f_locals:
        return frame.f_locals[name]
    if name in frame.f_globals:
        return frame.f_globals[name]
    raise Unresolved


def eval_index(node, frame):
    """Evaluate the small index expressions used by algorithm code.

    Only names, literals and simple arithmetic are supported. Calling arbitrary
    user code here would make tracing change the program being traced.
    """
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.Name):
        return lookup_name(node.id, frame)
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub)):
        value = eval_index(node.operand, frame)
        if isinstance(value, (int, float, complex)):
            return value if isinstance(node.op, ast.UAdd) else -value
        raise Unresolved
    if isinstance(node, ast.BinOp):
        left = eval_index(node.left, frame)
        right = eval_index(node.right, frame)
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            if isinstance(node.op, ast.Add):
                return left + right
            if isinstance(node.op, ast.Sub):
                return left - right
            if isinstance(node.op, ast.Mult):
                return left * right
            if isinstance(node.op, ast.FloorDiv) and right != 0:
                return left // right
            if isinstance(node.op, ast.Mod) and right != 0:
                return left % right
            if isinstance(node.op, ast.Pow) and abs(right) <= 1000:
                return left**right
        raise Unresolved
    if isinstance(node, ast.Tuple):
        return tuple(eval_index(item, frame) for item in node.elts)
    raise Unresolved


def resolve_chain(node, frame):
    """Return (root variable, path, current value) for a simple expression."""
    if isinstance(node, ast.Name):
        return node.id, [], lookup_name(node.id, frame)

    if isinstance(node, ast.Attribute):
        root, steps, value = resolve_chain(node.value, frame)
        try:
            attributes = vars(value)
        except TypeError:
            raise Unresolved
        if node.attr not in attributes:
            raise Unresolved
        return root, steps + [("attr", node.attr)], attributes[node.attr]

    if isinstance(node, ast.Subscript):
        root, steps, value = resolve_chain(node.value, frame)
        key = eval_index(node.slice, frame)
        if isinstance(value, dict):
            try:
                if key not in value:
                    raise Unresolved
            except TypeError:
                raise Unresolved
            return root, steps + [("key", key)], value[key]
        if isinstance(value, (list, tuple)):
            if not isinstance(key, int):
                raise Unresolved
            length = len(value)
            if key < 0:
                key += length
            if key < 0 or key >= length:
                raise Unresolved
            return root, steps + [("index", key)], value[key]
        raise Unresolved

    raise Unresolved


def step_id(kind, value):
    if kind == "attr":
        return ("attr", value)
    return (kind, safe_repr(value))


def label_for(root, steps):
    text = root
    for kind, value in steps:
        if kind == "attr":
            text += "." + str(value)
        elif kind == "index":
            text += f"[{value}]"
        else:
            text += f"[{safe_repr(value)}]"
    return text


def step_payload(kind, value):
    if kind == "index":
        return {"kind": "index", "index": value}
    if kind == "key":
        return {"kind": "key", "key": safe_repr(value)}
    return {"kind": "attr", "name": value}


def is_prefix(short, long):
    return len(short) <= len(long) and all(a == b for a, b in zip(short, long))


def focus_targets(frame):
    """Find list/dict elements touched by the current source line."""
    line = frame.f_lineno
    found = []

    for node in SUBSCRIPT_NODES:
        end = getattr(node, "end_lineno", None) or node.lineno
        if not (node.lineno <= line <= end):
            continue
        try:
            root, steps, _ = resolve_chain(node, frame)
        except Exception:
            continue
        found.append((root, steps, None))

    for node in METHOD_NODES:
        end = getattr(node, "end_lineno", None) or node.lineno
        if not (node.lineno <= line <= end):
            continue
        try:
            root, steps, value = resolve_chain(node.func.value, frame)
        except Exception:
            continue
        if not isinstance(value, (list, tuple, dict)):
            continue
        found.append((root, steps, f"{label_for(root, steps)}.{node.func.attr}"))

    targets = []
    seen = set()
    for root, steps, label_override in found:
        signature = (root, tuple(step_id(kind, value) for kind, value in steps))
        if signature in seen:
            continue
        seen.add(signature)
        targets.append((root, steps, label_override))

    # For matrix[i][j] prefer the element over the intermediate row.
    targets.sort(key=lambda item: len(item[1]), reverse=True)
    kept = []
    result = []
    for root, steps, label_override in targets:
        signature = tuple(step_id(kind, value) for kind, value in steps)
        if any(
            root == other_root and is_prefix(signature, other_signature)
            for other_root, other_signature in kept
        ):
            continue
        kept.append((root, signature))
        result.append(
            {
                "variable": root,
                "path": [step_payload(kind, value) for kind, value in steps],
                "label": label_override or label_for(root, steps),
            }
        )
        if len(result) >= MAX_FOCUS:
            break
    return result


def snapshot(frame, kind):
    return {
        "type": "event",
        "kind": kind,
        "line": frame.f_lineno,
        "stack": stack_frames(frame),
        "locals": visible(frame.f_locals.items()),
        "globals": visible(frame.f_globals.items()),
        "output": output_delta(),
        "focus": focus_targets(frame),
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
    global SUBSCRIPT_NODES, METHOD_NODES

    try:
        with open(TARGET, "r", encoding="utf-8") as handle:
            source = handle.read()
    except OSError as exc:
        emit({"type": "error", "message": f"Не удалось прочитать файл: {exc}"})
        return 1

    try:
        tree = ast.parse(source, filename=TARGET)
    except (SyntaxError, ValueError):
        tree = None
    if tree is not None:
        SUBSCRIPT_NODES = [
            node for node in ast.walk(tree) if isinstance(node, ast.Subscript)
        ]
        METHOD_NODES = [
            node
            for node in ast.walk(tree)
            if isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr in METHOD_NAMES
        ]

    input_handle = None
    if INPUT_PATH:
        try:
            input_handle = open(INPUT_PATH, "r", encoding="utf-8")
        except OSError as exc:
            emit({"type": "error", "message": f"Не удалось открыть файл ввода: {exc}"})
            return 1

    namespace = {
        "__name__": "__main__",
        "__file__": TARGET,
        "__builtins__": __builtins__,
    }

    sys.stdout = capture
    sys.stderr = capture
    sys.stdin = input_handle if input_handle is not None else io.StringIO("")
    sys.argv = [TARGET]

    try:
        code = compile(source, TARGET, "exec")
    except SyntaxError as exc:
        message = "".join(traceback.format_exception_only(type(exc), exc)).strip()
        emit({"type": "error", "message": message})
        if input_handle is not None:
            input_handle.close()
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
        if input_handle is not None:
            input_handle.close()

    emit(status)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
