# CodeVisualizer

**Пошаговый визуализатор выполнения Python-кода** для Linux: текущая строка,
значения переменных, стек вызовов и вывод программы — в одном окне.

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org)
[![Python](https://img.shields.io/badge/python-3.10%2B-blue)](https://www.python.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

![Скриншот CodeVisualizer](docs/screenshot.png)

## Возможности

- открытие `.py` файла: диалог, перетаскивание в окно или аргумент командной строки;
- кнопки **Запустить**, **Шаг**, **Продолжить**, **Стоп**;
- подсветка текущей строки с автопрокруткой;
- дерево локальных и глобальных переменных: списки, словари, множества, кортежи;
- стек вызовов с номерами строк;
- перехват `print` и вывода в `stderr`;
- показ необработанного исключения вместе с состоянием на месте ошибки;
- нативный интерфейс на Rust + egui, работает на Wayland и X11;
- демо-пример `examples/demo.py` (пузырьковая сортировка и рекурсивный `fib`).

## Быстрый старт

### 1. Зависимости для сборки из исходников

Нужны Python 3.10+, Rust 1.95+ и шрифты с кириллицей. Готовый архив из
**Releases** собирать не нужно — Python уже лежит внутри (см. ниже).

Arch Linux:

```bash
sudo pacman -S --needed python noto-fonts gcc pkgconf
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Другие дистрибутивы: установите Python 3.10+, компилятор C, `pkg-config` и
Rust через [rustup.rs](https://rustup.rs). Для диалога выбора файла нужен
`xdg-desktop-portal` (обычно уже установлен в KDE и GNOME).

### 2. Запуск

Скачайте репозиторий (Code → Download ZIP) или склонируйте его, затем:

```bash
cd CodeVisualizer
./run.sh examples/demo.py
```

`run.sh` сам соберёт release-версию и запустит приложение. Без аргумента
откроется пустое окно — файл можно выбрать кнопкой «Открыть…», перетащить в
окно, вставить путь в поле на главном экране или нажать «Демо».

Можно запускать и напрямую через Cargo:

```bash
cargo run --release --locked -- examples/demo.py
```

### Готовый бинарник

Если не хочется собирать вручную, скачайте архив из раздела **Releases**:

```bash
tar -xzf codevisualizer-linux-x86_64.tar.gz
./codevisualizer examples/demo.py
```

В архив уже входит портативный Python 3.12, поэтому ни Python, ни Rust
устанавливать не нужно. Если в системе есть Python 3.10+, приложение
использует его (вместе с установленными пакетами); иначе берёт встроенный
Python со стандартной библиотекой. Нужен только x86_64 Linux с glibc 2.35+ и
обычным графическим окружением (Wayland или X11): Ubuntu 22.04+, Debian 12+,
Arch Linux, Fedora 36+.

Если на системе нет `xdg-desktop-portal`, диалог «Открыть…» может быть
недоступен — перетащите файл в окно, вставьте путь на главном экране или
запустите `./codevisualizer file.py`.

Встроенный Python содержит только стандартную библиотеку. Если скрипту нужны
сторонние пакеты, поставьте их в него:

```bash
./python-runtime/bin/python3 -m pip install numpy
```

## Установка в меню приложений

```bash
./install.sh
```

Скрипт соберёт release-версию, скопирует бинарник в `~/.local/bin`, установит
ярлык и иконку в `~/.local/share`. После этого CodeVisualizer появится в меню
KDE и в списке «Открыть с помощью» для `.py` файлов.

Удаление:

```bash
./install.sh --uninstall
```

## Управление

| Кнопка | Действие |
|---|---|
| Запустить | запускает Python-трейсер и останавливается на первой строке |
| Шаг | выполняет одну строку и останавливается на следующей |
| Продолжить | выполняет программу до конца (точек останова пока нет) |
| Стоп | завершает процесс Python |

Слева — код с подсветкой текущей строки. Справа — переменные, стек вызовов и
вывод программы. Переменные можно сворачивать и разворачивать, как в обычном
отладчике.

## Как это работает

```
Rust + egui (UI)  ←— JSON-строки через stdin/stdout —→  python/tracer.py
      │                                                       │
 подсветка строки,                                   sys.settrace,
 дерево переменных,                                  снимки f_locals/f_globals,
 стек вызовов, вывод                                 перехват stdout/stderr
```

- `src/main.rs` — точка входа, шрифты и оформление;
- `src/app.rs` — интерфейс и состояние приложения;
- `src/tracer.rs` — запуск Python-процесса и протокол обмена;
- `src/value.rs` — отображение сериализованных значений Python;
- `python/tracer.py` — трейсер (встраивается в бинарник через `include_str!`).

При запуске по умолчанию используется системный `python3` (3.10+), чтобы были
доступны установленные пользователем пакеты. Если его нет, готовый архив
берёт Python из `python-runtime/`.

Python-скрипт выполняется в отдельном процессе. Трейсер останавливается на
каждой строке, сериализует состояние в JSON и ждёт команду `step`, `continue`
или `stop`. Rust-интерфейс читает эти сообщения и обновляет окно.

## Сборка

```bash
cargo build --release --locked
./target/release/codevisualizer examples/demo.py
```

Готовый бинарник: `target/release/codevisualizer` (Linux x86_64).

## Релиз

```bash
git tag v0.1.0
git push origin v0.1.0
```

Workflow `.github/workflows/release.yml` соберёт
`codevisualizer-linux-x86_64.tar.gz` с Python 3.12 внутри и прикрепит архив к
GitHub Releases.

## Разработка

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Для отладки интерфейса есть переменные окружения:

```bash
CODEVISUALIZER_AUTOSTART=1 CODEVISUALIZER_AUTOSTEPS=20 \
  ./target/debug/codevisualizer examples/demo.py
```

`AUTOSTART` сразу запускает трейсер, `AUTOSTEPS` автоматически делает указанное
число шагов. Переменная `CODEVISUALIZER_PYTHON=/path/to/python3` позволяет
запустить трейсер другим интерпретатором.

## Ограничения текущей версии

- трассируется только выбранный файл; код импортированных модулей не подсвечивается;
- потоки, запущенные программой, не трассируются;
- нет точек останова и условных остановок;
- «Продолжить» всегда выполняет программу до конца;
- интерфейс приложения пока на русском языке.

## Дорожная карта

- [ ] точки останова по клику на номер строки;
- [ ] подсветка синтаксиса Python;
- [ ] графические диаграммы структур данных (столбики для списков, деревья);
- [ ] трассировка импортированных модулей;
- [ ] английская локализация интерфейса.

## Лицензия

MIT — см. [LICENSE](LICENSE).

---

**English:** CodeVisualizer is a step-by-step Python execution visualizer for
Linux, built with Rust + egui and Python's `sys.settrace`. It shows the current
line, variables, call stack and program output. The prebuilt release bundles
Python 3.12 and needs no Rust or Python installation; it prefers the system
Python 3.10+ when available (so pip packages work), otherwise falls back to the
bundled interpreter. Building from source requires Rust 1.95+ and Python 3.10+.
MIT licensed.
