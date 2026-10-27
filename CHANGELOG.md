# Changelog

All notable changes to scrumtui are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [2.4.1] - 2026-10-27

### Fixed
- **`y` copies on Linux without a clipboard tool.** When none of `wl-copy`, `xclip` or `xsel` is installed, a background `scrumtui __clipboard` process holds the copied text and serves pastes until something else is copied, as `xclip` does. Before, `y` failed with "no clipboard tool found".
- README screenshots restored.

### Changed
- Brighter text in the issue form: the field you're typing in and subtask titles are white, and labels and the other fields are light gray instead of dark gray.

---

## [2.4.0] - 2026-10-21

### Changed
- **One frog a day.** The frog is settled once a day and stays, whether you start it, finish it or leave it: starting it (TODO → IN PROGRESS) no longer swaps in another issue, and finishing it doesn't bring a second one the same day. An unfinished frog carries over to the next day; a finished one is replaced then. `f` makes an issue today's frog, and `f` on the frog means no frog today. A frog pinned in 2.3.0 becomes today's frog.
- The frog's line in the detail panel no longer suggests adding a first step, and it sits right under the title.
- **Sprints default to Monday to Friday.** At the weekend, a new sprint is for the coming week. The automatic fix for short sprints now only touches sprints shorter than five days; it used to stretch anything under seven days, which would have rewritten every Monday-to-Friday sprint.
- Top bar: more space between items. On narrow terminals the sprint summary drops the progress bar and the day count instead of running into the view tabs, and a sprint that hasn't started yet shows its first day.
- Kanban cards list the frog, carry-over count and due date before the epic, so a narrow column cuts the epic rather than those.
- The detail panel shows the time spent right after the due date.
- README screenshots use a smaller terminal (88×28), so their text reads larger.

---

## [2.3.0] - 2026-10-21

### Added
- **New issues from the kanban board**: `n` creates an issue in the active sprint, in the focused column. In the subtask panel, `n` adds a subtask to the shown parent.
- **Starting the next sprint from the TUI**: once the active sprint has ended, `S` opens a new sprint for the following week, and `Ctrl-N` in the sprint manager switches between "new sprint" and "edit current". The form lists the unfinished issues that saving will carry over. Before, rolling over meant deactivating the old sprint first, which skipped the carry-over.
- **The frog** (from "eat the frog"): the active sprint's unstarted issue that has been put off the longest (most carry-overs, then oldest, then biggest) is marked `frog` and named on startup. The detail panel says how long it has waited and suggests splitting off a 15-minute first step. `f` pins your own pick; finishing it hands over to the next one.
- **Sprint history analysis**: planned, added, removed and carried-over scope per sprint; "plan kept" (share of the first-day plan that got done); "carried in → done"; and a breakdown by epic. The issue list shows what happened to each issue in that sprint and can be browsed (`Tab`, then `e` to edit). The sprint list shows a completion bar per sprint, and `n` creates a sprint. On terminals narrower than 110 columns the analysis moves under the issue list.
- **Top bar** with the four views and the active sprint's progress, or a reminder once it has ended.
- **Live reload**: changes written by another process (`scrumtui add` from a script or an AI assistant) show up in a running TUI within a second, with a note naming new issues.
- **CLI**: `edit`, `show`, `add --parent` (subtasks), `-m`/`--desc`, `--backlog`, `list --epic`, `--db=path`, and a due-date column plus a sprint marker in `list`. Due dates accept `today`, `tomorrow` and `none`.
- `g`/`G`, `Home`/`End` and `PgDn`/`PgUp` in every list: kanban columns and subtasks, trash, epic detail, form subtasks, history. `g`/`G` jump to the top/bottom as in `less` (vim's `gg` works too).
- A form with unsaved changes asks for a second `Esc` before discarding them.
- The due-date dropdown suggests today, tomorrow, the end of the sprint and next Monday; "tom" + `Tab` picks tomorrow.
- `Ctrl-N` adds a subtask from any field of the issue form; `n` adds one inside the subtask list. `↑`/`↓` move between form fields, and `Ctrl-S` also saves the sprint form.
- Editing an issue from a gantt epic's detail returns to that epic's list.
- Backlog search also matches subtask titles, and `Esc` clears a kept search filter.
- `y` (copy) and `f` (frog) on the kanban board.
- New demo data: six weekly sprints of a computational materials postdoc (DFT training data, fine-tuning a machine-learned potential, active learning, a paper, mentoring, proposals) with carry-overs and scope changes. `scrumtui --db FILE init --demo` keeps it in a file.
- `cargo test render_readme_snapshots -- --ignored` renders the README screenshots.

### Changed
- **Help overlay condensed**: each key appears once with its meaning in every view, followed by the few view-specific keys. The README keys section matches it.
- The selected row highlights only its title, instead of reversing every column into colored blocks.
- The backlog title column adapts to the terminal width, the detail panel grows to fit the description, and lists keep their scroll position instead of pinning the selection to the bottom edge.
- Gantt bars use the full width and get a `today` marker.
- In the kanban subtask panel, `h`/`l` cycle through parents (like `<`/`>`).
- After `s`, the cursor moves on to the next issue, so repeated `s` pulls a run of issues into the sprint. The same after moving an issue to the trash.
- `S` (sprint manager) and `T` (trash) work in every view.
- New sprints are named after their ISO week ("Week 44").
- No emoji: the trash title and form errors are plain text.
- The README asks for Rust 1.88+, which the locked dependencies need (1.75 couldn't read the lockfile).

### Fixed
- **Velocity and done counts in sprint history**: an issue carried from sprint A to B to C and finished in C counted as done in all three. It now counts as done only where it was finished, and as carried over in the others.
- Issues taken out of a running sprint stayed in its history as if they had been planned. Removals are now recorded and shown, and undoing a sprint toggle restores the history exactly.
- Scope creep counted issues *created* after the sprint started rather than issues *added* to it after planning. Carried-in issues and issues added on the day a late sprint was set up count as planned.
- The burnup scope line steps up and down when issues join or leave the sprint.
- Gantt: the selected epic could scroll off screen, and the date header didn't line up with the bars.
- Parents whose status rolls up from their subtasks never got a start time, so "actual time" stayed empty.
- Undoing a status change reset timestamps (undoing Done → In Progress gave the issue a new completion time); undo now restores the previous row exactly.
- `Tab` in the due-date field didn't complete a typed partial date, and with more than six matches the epic and due-date dropdowns could pick a different entry than the highlighted one.
- Ctrl-combinations typed letters into text fields (Ctrl-A inserted "a", Ctrl-N in the title inserted "n").
- The kanban hint advertised `n` and `/`, and the gantt hint `/`, none of which existed there.
- The detail panel cut off the time line when an issue had a description.
- Copied (`y`) subtask checklists were in reverse order.
- Deleting a sprint or purging an issue left rows in the sprint log; fixing short sprints could move a start date later.
- CLI: invalid `-p`, `-d` or `-s` values silently fell back to defaults, unknown commands opened the TUI, and `status` on a parent with subtasks put it out of sync with them.
- The CLI and the TUI wait up to 5 s for each other's writes instead of failing with "database is locked".
- `scrumtui list | head` exits quietly instead of panicking with "Broken pipe".
- Markdown export lists subtasks in creation order and escapes `|` in titles.
- Two `scrumtui demo` sessions no longer share (and delete) the same temporary database.

---

## [2.2.0] — 2026-10-21

### Removed
- **Colemak auto-detection** (2.1.0) reverted. Keys are layout-independent again: `n` new, `e` edit, `h`/`j`/`k`/`l` navigate.

### Added
- **Arrow keys everywhere navigation happens**: `←`/`↓`/`↑`/`→` work like `h`/`j`/`k`/`l` in every view, the trash list, form status fields, the subtask list and the sprint "active" toggle. Ctrl-arrows work like Ctrl-h/j/k/l: `Ctrl-←`/`Ctrl-→` move an issue between kanban columns, `Ctrl-↑`/`Ctrl-↓` reorder the backlog.
- **Unfinished sprint issues float to the top**: in the backlog's sprint section and the kanban columns, unfinished issues (TODO / IN PROGRESS) come first, ordered by how long they've been around (most sprint carry-overs first, then oldest). Done issues follow. The order refreshes on view switch, so an issue doesn't jump while you change its status.
- **Linux clipboard support for `y`**: yank now falls back to `wl-copy`, `xclip` or `xsel` when `pbcopy` isn't available.
- Unit tests for key handling and sprint ordering (`cargo test`).

### Changed
- **Burnup legend** moved from the top-right to the bottom-right corner so it no longer covers the lines.
- Epic / due-date autocomplete dropdowns are navigated with `↑`/`↓` (or `Ctrl-j`/`Ctrl-k`) so plain `j`/`k` can be typed.

### Fixed
- **Backlog reorder (`Ctrl-j`/`Ctrl-k`) had no visible effect**: issues were listed by `created_at`, ignoring the rank being swapped. The list is now ordered by rank, which matches creation order for unreordered data. New subtasks now get a rank too, so they keep creation order.
- **Typing `j`/`k` in the epic field** moved the dropdown selection instead of inserting the letter.
- **Editing a subtask title after removing another subtask** edited the wrong subtask (raw vs. visible index).
- **Kanban subtask focus** could land on the wrong row after switching columns (used a per-column index for the flat subtask list).
- Help overlay listed `h`/`l` as "advance / regress" (it's regress / advance) and was missing the reorder keys.

---

## [2.0.1] — 2026-08-03

### Fixed
- **Safe start rounding**: safe-start value in the history analysis panel was displaying raw floating-point output (e.g. `≤12.666666666666666sp`). Now rounded to one decimal place.
- **Sprint history epic column invisible after resize**: hard minimum column widths (`max(20)` for title, `max(10)` for epic) could exceed the available panel width, pushing the epic column off-screen with no recovery on resize. Column widths are now purely proportional to the actual available space (62% title, 38% epic of flexible room), so the epic column always appears when there is any space to spare.
- **Orphan process on terminal close**: when the terminal was closed without quitting scrumtui first, the process would block in `event::poll()` for up to one hour before noticing the SIGHUP signal. The idle poll timeout has been reduced from 3600 s to 1 s, so the process exits within one second of receiving any termination signal.
- **Reduced CPU usage**: the 1 s idle poll cap also bounds any spurious wakeup cost, and non-actionable terminal events (focus, mouse, paste) no longer trigger unnecessary full redraws.

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
