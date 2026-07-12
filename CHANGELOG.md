# Changelog

All notable changes to scrumtui are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [2.0.0] — 2026-07-12

Major release with bug fixes, improved UI responsiveness, and enhanced demo data.

### Added
- **`scrumtui demo` command**: Quick-start with postdoc computational materials science sample data in a temporary database. Includes realistic research tasks, paper submissions, and collaboration workflows. Demo data is automatically cleaned up on exit, leaving your actual database untouched.
- **Responsive history view columns**: Issue list in sprint history now adapts column widths to terminal width, ensuring epic names and other metadata remain visible on resize.
- **Sprint membership log**: New `issue_sprint_log` table records every (issue, sprint) pairing that has ever existed. Replaces the broken `primary_sprint_id` approach — issues that carry across A→B→C now appear correctly in all three sprints' history views.

### Fixed
- **Sprint history view epic display**: Fixed hard-coded column widths that prevented epic names from displaying on narrow terminals. Columns now dynamically allocate space: ~40% title, ~25% epic, rest for other fields.
- **Sprint history missing issues**: Fixed bug where issues moved from sprint A→B→C would disappear from A and B's history views. The old `primary_sprint_id` column only tracked one hop. The new `issue_sprint_log` table is append-only: every time `set_issue_sprint` or `move_incomplete_issues_to_sprint` runs, a row is inserted, so all affected sprints show the issue. The `sprint_id` column still tracks current assignment for the backlog/kanban.
- **Page navigation keybinds**: `Ctrl-F`/`Ctrl-B` and `Ctrl-D`/`Ctrl-U` are unreliable — terminals, tmux, and readline all intercept them before the app sees them. Replaced with `PgDn`/`PgUp` (physical keys that always reach the app in raw mode) across all four views. Also fixed a root cause bug: the global `u` handler (undo) was matching before modifier checks, eating `Ctrl-U` in every view.

### Changed
- README updated with 2.0.0 release notes and demo command documentation.
- Demo seed data changed from generic DFT/MXene workflow to postdoc-focused defect analysis research pipeline (computational materials science use case).

---

## [1.5.0] — 2026-07-10

### Added
- **Yank to clipboard** (`y` in backlog): copy issue title, description, and subtask tree (with status checkboxes) to clipboard via `pbcopy`.
- **Gantt epic detail navigation**: enter the epic popup (`e`/`Enter`) and use `j`/`k`/`g`/`G` to navigate issues, press `Enter` to edit.
- **Sprint history navigation**: `g`/`G` to jump to first/last sprint; `Ctrl-D`/`Ctrl-U` for page up/down (both now work in backlog, gantt, and history views).
- **Auto-fix short sprints**: when you open the history view (`4`), any past sprints shorter than 7 days are automatically back-dated to exactly 7 days (start = end − 6), with no overlaps.
- **Velocity per day**: analysis panel now shows sp/day instead of raw story points — computed as `done_sp / sprint_duration` averaged over up to 5 prior sprints; safe start is scaled to the current sprint's actual duration.

### Changed
- **Status field in issue form**: replaced the dropdown with `h`/`l` cycling (no more arrow keys or Enter to open). Shows one status at a time: TODO ↔ IN PROGRESS ↔ DONE.
- **Analysis panel cleanup**: removed "cycle time" row (avg IP→Done hours/SP); removed "done sp" sparkline; simplified "safe start" and "scope" to show just the numbers without trailing hints.
- **Sprint duration display**: removed the 7-day cap — sprints now display their real duration. "Day X/Y" progress is only shown for the active sprint.

### Fixed
- **Gantt popup scrolling**: implemented selection-based navigation (sel) separate from scroll offset, so j/k highlight issues instead of just paging; selection auto-scrolls to stay visible.

---

## [1.4.0] — 2026-07-06

### Added
- **Sprint carry-forward**: when a new sprint is activated, all TODO/IN PROGRESS issues from the old sprint are automatically moved into the new sprint (in addition to having their `carry_count` incremented). A status message reports how many issues were carried forward.
- **Sprint history analysis panel**: a new panel appears below the burnup chart in the history view showing per-sprint contextual stats — velocity (avg done SP from prior sprints only, so it changes as you navigate), completion %, scope creep (SP added mid-sprint vs starting load), avg IP→Done cycle time with hours/SP, a velocity sparkline (oldest→selected, rightmost bar always the selected sprint), and a "safe start" recommendation showing the max SP to commit to based on p25 of historical done SP.
- **Relative velocity**: all history analysis figures are now computed relative to the selected sprint — velocity uses only sprints that ended before it, and the sparkline grows as you navigate forward in time, so earlier sprints show their historical context rather than current averages.

### Changed
- **Kanban keybinds** (breaking): `h`/`l` now navigate between columns (left/right cursor movement); `Ctrl-H`/`Ctrl-L` now move the selected issue to the previous/next column. Previously this was the opposite.
- **Description field cursor**: fixed a double-space prefix bug where the field's `value_display` already contained a leading space and the render loop prepended another, causing the real terminal cursor to sit two characters past the end of the text. The render loop now iterates over the raw description directly.
- **Sprint duration display**: sprint durations are now capped at 7 days in the stats header and day counter. Sprints recorded with `start_date == end_date` (a common data artifact from not setting an end date) are treated as 7-day sprints for display and analysis purposes.
- **"vs avg" row replaced**: the opaque "vs avg" percentage line is replaced by a "safe start" recommendation — `≤Xsp` derived from the p25 of prior done SP, with a contextual label (`commitment looks good` / `over-committed vs history` for the active sprint; `was within safe range` / `started above safe range` for past sprints).
- Help overlay, hint bars, and README updated to reflect the new kanban keybinds.

---

## [1.1.0] — 2026-06-22

### Changed
- **Kanban cards**: item number (`#id`) removed from the metadata line; titles now word-wrap across multiple lines instead of truncating
- **Epic status** in the Gantt view now uses the same logic as subtasks — all-done → green, all-todo → yellow, any mix (done+todo or any in-progress) → cyan
- **Epics** in the Gantt view are now sorted by date started (newest first) instead of alphabetically
- **Subtasks** now display in creation order (oldest first) in the backlog, kanban panel, and issue form — previously they were shown newest-first
- **Word deletion** (`Alt+Backspace`) now also triggers on `Ctrl+Backspace` and `Ctrl+W` in all text fields (search, issue form, sprint form, subtask titles)
- **Bottom hint bars** cleaned up across all views — removed stale shortcuts (`^j/^k`, `[S]mgr`, view-switch keys); each view now shows only: new, edit, status, search, undo, help (plus any view-specific essentials)

---

## [1.0.0] — 2026-06-16

First stable release. All core features are present and working: backlog, kanban, gantt, sprint history, subtask management, CLI subcommands, and Jira import/export.

### Added
- **Subtask kanban panel** stacks all subtasks for the focused parent in a single vertical list — each row shows the status symbol, title, status badge, and due date inline; Tab to focus, `<`/`>` to cycle parents
- **Carry-over indicator**: when a new sprint is activated, any TODO/IN PROGRESS issues from the old sprint have their `carry_count` incremented; an orange `↩N` badge appears in both the backlog and kanban so carried items visually surface as higher priority for the new week
- **Configurable database path** (in priority order): `--db <path>` flag, `SCRUMTUI_DB` environment variable, `~/.config/scrumtui/config` with `db_path = /path/to/file.db`, fallback to `~/.scrumtui.db`
- **Sprint date normalization**: new sprints default to the full Monday–Sunday week containing today; all existing sprints longer than 7 days have their start date moved to the Monday of the week containing their end date

### Changed
- Issue list (backlog): no bold anywhere — active titles use the terminal's default foreground (readable on both light and dark themes), done titles use `Gray` (one step lighter than `DarkGray`, clearly faded without being invisible); SP and epic keep their normal colors on all issues
- Kanban cards: all titles bold regardless of status; done cards use `Gray + Bold`
- Status badges no longer bold in the issue list
- Sprint form now defaults to the Monday–Sunday week containing today instead of an arbitrary 4-day window
- Help overlay condensed: Universal and Global sections merged, redundant per-view navigation entries removed, popup height reduced
- README condensed to roughly half the length
- Issue form: Enter now saves from the description field instead of inserting a newline

---

## [0.5.0] — 2026-06-16

### Added
- **Subtask kanban panel** now stacks subtasks in a single vertical list instead of three separate columns — each row shows the status badge, title, and due date inline
- **Carry-over indicator**: when a new sprint is activated, any TODO/IN PROGRESS issues from the old sprint get their `carry_count` incremented; backlog and kanban show an orange `↩N` badge (e.g. `↩2`) on carried items so they visually surface as higher priority
- **Configurable database path** via three mechanisms (in priority order):
  1. `--db <path>` CLI flag (works with all subcommands: `scrumtui --db ~/work.db`)
  2. `SCRUMTUI_DB` environment variable
  3. `~/.config/scrumtui/config` (or `$XDG_CONFIG_HOME/scrumtui/config`) with line `db_path = /path/to/file.db`
  - Falls back to `~/.scrumtui.db` for backward compatibility
- **Sprint date normalization**: new sprints default to the Monday–Sunday week containing today; existing sprints longer than 7 days have their start date moved to the Monday of the week containing their end date

### Fixed
- Issue and subtask list colors: non-done items now render in bright white (bold for parents, normal for subtasks); done items are faded `DarkGray` — previously the contrast was inverted
- Status badges (TODO / IP / DONE) are now bold in the issue list for clearer legibility

---

## [0.4.0] — 2026-06-10

### Added
- Status dropdown in issue form with `h`/`l` cycling and arrow-key selection
- Sprint deletion from the sprint manager and sprint history view (`d` to confirm)
- Undo support for sprint membership toggle (`u`)

### Fixed
- Sprint form navigation wraps correctly with `BackTab`
- Help menu updated to document new sprint and form keybindings

---

## [0.3.0] — 2026-06-09

### Added
- Sprint history view (`4`) with per-sprint stats (issues done, story points, elapsed days) and a burnup chart
- Burnup chart is now reusable and rendered in both the sprint manager popup and the history view
- Sprint list shows an active-sprint indicator (`●`)
- Kanban subtask panel with dynamic sizing based on subtask count
- Due date color coding in the kanban card metadata line (red = overdue, yellow = due within 7 days)
- Scope line on burnup chart showing total story point scope over time

### Changed
- Sprint detail layout widened to accommodate burnup chart alongside stats
- Issue list in history view includes navigation hint bar
- Help menu updated with history and kanban subtask navigation

---

## [0.2.0] — 2026-06-08

### Added
- `rank` field on issues for stable backlog ordering; `Ctrl+j`/`Ctrl+k` to reorder
- Sprint history view skeleton (`4` key)
- Due date autocomplete dropdown in the issue form
- Burnup chart in the sprint manager popup
- Gantt epic detail popup with issue list and search

### Changed
- Backlog footer line dynamically fills terminal width
- Kanban detail pane renders created/updated/completed timestamps
- Help popup updated with all new keybindings

### Fixed
- Backlog footer no longer overflows narrow terminals

---

## [0.1.1] — 2026-06-02

### Fixed
- Minor rendering and navigation tweaks following initial release
- Corrected import ordering

---

## [0.1.0] — 2026-06-01

### Added
- Initial release
- Backlog view with sprint section, subtask tree, search, and show/hide completed toggle
- Kanban view with three-column board (TODO / IN PROGRESS / DONE)
- Gantt / epic timeline view
- Sprint manager popup (create, edit, activate sprints)
- Issue form with title, epic, story points, status, due date, description, and inline subtask editor
- Undo stack (status changes, edits, deletes, sprint toggles, rank swaps)
- Soft-delete trash with restore and permanent purge
- CLI subcommands: `add`, `status`, `list`, `import`, `export`, `init`, `init --demo`
- Jira CSV import
- Markdown export
- SQLite backend (`~/.scrumtui.db`)
- Signal handling (SIGTERM / SIGHUP / SIGINT) for clean terminal restore
