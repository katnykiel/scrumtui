# scrumtui

A minimal, local, terminal-based scrum board driven by keyboard shortcuts.

---

> **⚠ AI-GENERATED CODE DISCLAIMER**
>
> The majority of this codebase was generated with the assistance of Claude Sonnet. It has been reviewed and lightly edited by a human, but has not been rigorously audited. Use at your own risk, and inspect any code before relying on it in a critical context.

---

![scrumtui backlog view](image.png)

## What it is

`scrumtui` is a lightweight personal scrum system that lives entirely on your machine. There is no server, no account, no browser. Everything is stored in a single SQLite file at `~/.scrumtui.db`. The UI runs in your terminal using [ratatui](https://github.com/ratatui-org/ratatui).

It has three views:

| View | Key | Description |
|------|-----|-------------|
| **Backlog** | `1` | Full issue list, sprint at the top. Create, edit, delete, and move issues. |
| **Kanban** | `2` | Three-column board (TODO / IN PROGRESS / DONE) for the active sprint. |
| **Gantt** | `3` | Timeline chart grouped by epic, with bar per issue. |

The sprint manager (opened with `S`) includes a live **burnup chart** showing ideal vs. actual story-point completion over the sprint period.

---

## Building

Requires [Rust](https://rustup.rs/) (stable, 1.75+). No system SQLite needed — it is bundled at compile time.

```bash
git clone <this-repo>
cd scrumtui
cargo build --release
./target/release/scrumtui
```

The binary is fully self-contained. You can copy it anywhere on your `PATH`.

On the very first run, the database is empty and **sample data is loaded automatically** so you can explore all views immediately. To reset the sample data, delete `~/.scrumtui.db` and run again.

---

## Usage

### Navigation (all views)

| Key | Action |
|-----|--------|
| `1` / `2` / `3` | Switch to Backlog / Kanban / Gantt |
| `q` or `Ctrl-C` | Quit |
| `?` | Open / close the help overlay |

### Backlog view

| Key | Action |
|-----|--------|
| `j` / `k` or `↓` / `↑` | Move selection down / up |
| `g` / `G` | Jump to first / last issue |
| `n` | Create a new issue |
| `e` or `Enter` | Edit the selected issue |
| `d` | Delete the selected issue (confirm with `d` again) |
| `s` | Toggle the selected issue in/out of the active sprint |
| `S` | Open the sprint manager (create or edit the sprint, view burnup) |

The sprint is shown at the top with a box around it. Issues below the sprint box are in the backlog.

### Kanban view

| Key | Action |
|-----|--------|
| `h` / `l` or `←` / `→` | Switch between TODO / IN PROGRESS / DONE columns |
| `j` / `k` | Move selection up / down within a column |
| `>` or `.` | Advance the selected issue to the next status |
| `<` or `,` | Regress the selected issue to the previous status |
| `e` or `Enter` | Edit the selected issue |

### Gantt view

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll down / up |

Issues are grouped by epic. Each issue takes two rows: the title on the first line, and a timeline bar with story points and dates on the second. Bars use `░` (TODO), `▓` (in progress), `█` (done).

### Forms (issue & sprint editor)

| Key | Action |
|-----|--------|
| `Tab` / `Shift-Tab` | Move to next / previous field |
| `h` / `l` (in Status field) | Cycle through TODO → IN PROGRESS → DONE |
| `Space` (in Active toggle) | Toggle sprint active yes/no |
| `Enter` | Save |
| `Esc` | Cancel without saving |

### Delete confirmation

When you press `d` on an issue, a confirmation popup appears. Press `d` again to confirm deletion, or `n` / `Esc` to cancel.

---

## Issue fields

| Field | Required | Notes |
|-------|----------|-------|
| Title | ✓ | |
| Story Points | ✓ | Any positive number, including decimals (e.g. `0.5`, `2.5`) |
| Epic | ✓ | Free text label for grouping (e.g. `dft`, `writing`) |
| Status | ✓ | TODO / IN PROGRESS / DONE |
| Due Date | — | Format: `YYYY-MM-DD` |
| Description | — | Free text; shown in the detail pane at the bottom of the backlog |

All changes (status, edits) are written to the SQLite database immediately.

---

## Data

- **Database**: `~/.scrumtui.db` (SQLite, auto-created on first run)
- **Tables**: `issues`, `sprints`
- Each issue records `created_at`, `updated_at`, and `completed_at` (set automatically when status becomes DONE)
- The burnup chart in the sprint manager uses `completed_at` to compute actual daily progress

---

## Limitations / known gaps

- Only one sprint can be active at a time
- No child issues (planned but not implemented)
- No export, no sync, no notifications
- The terminal must be at least ~100 columns wide for the full layout to render cleanly
