# scrumtui

A minimal, local, terminal-based scrum system driven by keyboard shortcuts. I use scrum every day to manage my research, and got sick of the existing scrum systems, so I vibe-coded this TUI to fit my needs. Maybe you'll find it useful too!

**Version 1.2.0**

---

> **⚠ AI-GENERATED CODE DISCLAIMER**
>
> The majority of this codebase was generated with the assistance of OpenCode and Claude Sonnet 4.6. It has been reviewed and lightly edited by a human, but has not been rigorously audited. Use at your own risk, and inspect any code before relying on it in a critical context.

---

![scrumtui backlog view](image.png)

## What it is

`scrumtui` is a lightweight personal scrum system that lives entirely on your machine — no server, no account, no browser. Everything is stored in a single SQLite file. Four views: **Backlog** (`1`), **Kanban** (`2`), **Gantt** (`3`), **Sprint History** (`4`). The sprint manager (`S`) includes a live burnup chart.

---

## Building

Requires [Rust](https://rustup.rs/) (stable, 1.75+). SQLite is bundled at compile time.

```bash
git clone https://github.com/katnykiel/scrumtui
cd scrumtui
cargo build --release
./target/release/scrumtui
```

---

## Keys

`j`/`k` navigate, `h`/`l` advance/regress status, `Tab` next field or panel, `e`/`Enter` edit, `Esc` cancel. `?` opens the full help overlay.

**Backlog:** `n` new · `e` edit · `d`/`T` trash · `s`/`S` sprint toggle/manager · `c` toggle done · `/` search · `Ctrl-j`/`Ctrl-k` reorder

**Kanban:** `[`/`]` switch column · `h`/`l` regress/advance status · `Tab` parent↔subtask panel · `<`/`>` cycle parent

**Forms:** `Tab`/`Shift-Tab` next/prev field · `h`/`l` regress/advance subtask status · `Del` clear due date · `Ctrl-N` add subtask · `x` remove subtask · `Ctrl-S` save

---

## Database path

Resolved in order: `--db <path>` flag → `SCRUMTUI_DB` env var → `~/.config/scrumtui/config` (`db_path = ...`) → `~/.scrumtui.db`.

---

## CLI

```bash
scrumtui add "Title" -e epic -p 2 -d 2026-06-20 --sprint
scrumtui status 42 done
scrumtui list [--all] [--sprint] [-s todo|ip|done]
scrumtui import export.csv
scrumtui export [output.md]
```

---

## Issue fields

Title, epic, story points, status, due date (`YYYY-MM-DD`), description, subtasks. Epic and due date autocomplete from existing values. Subtask statuses roll up to the parent automatically.

---

## Jira import

Handles Jira's default CSV export. Stories/Tasks/Bugs become top-level issues; Subtasks are linked to their parent. Run `scrumtui import export.csv`.

---

## Limitations

- One active sprint at a time
- Terminal should be at least ~100 columns wide
