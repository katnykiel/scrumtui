use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::db::{Db, SprintLogRow};
use crate::models::{Issue, Outcome, Sprint, SprintEntry, Status};

/// Rows moved by PgDn/PgUp (and Ctrl-D/U, Ctrl-F/B).
const PAGE: usize = 10;
/// Most entries an autocomplete dropdown shows (and lets you select).
pub const DROPDOWN_ROWS: usize = 6;

// ── Fuzzy match ───────────────────────────────────────────────────────────────

/// Returns true if every character in `query` appears in `haystack` in order
/// (subsequence / fuzzy match). Both should already be lowercased.
fn fuzzy_match(query: &str, haystack: &str) -> bool {
    let mut haystack_chars = haystack.chars();
    for qc in query.chars() {
        if !haystack_chars.any(|hc| hc == qc) {
            return false;
        }
    }
    true
}

// ── List navigation ───────────────────────────────────────────────────────────

/// A movement within a list. Every list in the app uses the same keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// Map a key to a list movement: j/k (↓/↑) by one, g/G or Home/End to the top/bottom
/// (as in `less`; vim's `gg` works too), PgDn/PgUp or Ctrl-D/U and Ctrl-F/B by a page.
fn nav_of(key: &KeyEvent) -> Option<Nav> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down if !ctrl => Some(Nav::Down),
        KeyCode::Char('k') | KeyCode::Up if !ctrl => Some(Nav::Up),
        KeyCode::Char('g') | KeyCode::Home if !ctrl => Some(Nav::Top),
        KeyCode::Char('G') | KeyCode::End if !ctrl => Some(Nav::Bottom),
        KeyCode::PageDown => Some(Nav::PageDown),
        KeyCode::PageUp => Some(Nav::PageUp),
        KeyCode::Char('d') | KeyCode::Char('f') if ctrl => Some(Nav::PageDown),
        KeyCode::Char('u') | KeyCode::Char('b') if ctrl => Some(Nav::PageUp),
        _ => None,
    }
}

/// Move a selection index within a list of `len` items.
fn nav_index(sel: usize, len: usize, nav: Nav) -> usize {
    let last = len.saturating_sub(1);
    let moved = match nav {
        Nav::Up => sel.saturating_sub(1),
        Nav::Down => sel + 1,
        Nav::PageUp => sel.saturating_sub(PAGE),
        Nav::PageDown => sel + PAGE,
        Nav::Top => 0,
        Nav::Bottom => last,
    };
    moved.min(last)
}

// ── View ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Backlog,
    Kanban,
    Gantt,
    SprintHistory,
}

// ── Backlog display items ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BacklogItem {
    SprintHeader(Sprint),
    SprintFooter,
    BacklogHeader,
    Issue(Issue, bool),       // (issue, is_in_sprint)
    Subtask(Issue, bool),     // (subtask issue, parent_is_in_sprint)
}

// ── Subtask draft (held in IssueForm while editing) ───────────────────────────

#[derive(Debug, Clone)]
pub struct SubtaskDraft {
    pub id: Option<i64>,
    pub title: String,
    pub status_idx: usize,
    pub deleted: bool,
}

impl SubtaskDraft {
    fn blank() -> Self {
        SubtaskDraft { id: None, title: String::new(), status_idx: 0, deleted: false }
    }
}

// ── Issue form ────────────────────────────────────────────────────────────────

pub const F_TITLE: usize = 0;
pub const F_EPIC: usize = 1;
pub const F_SP: usize = 2;
pub const F_STATUS: usize = 3;
pub const F_DUE: usize = 4;
pub const F_DESC: usize = 5;

#[derive(Debug, Clone)]
pub struct IssueForm {
    pub editing_id: Option<i64>,
    /// When set, the newly created issue will be added to this sprint.
    pub sprint_id: Option<i64>,
    pub title: String,
    pub story_points: String,
    pub epic: String,
    pub status_idx: usize,
    pub due_date: String,
    pub description: String,
    pub focused_field: usize,  // F_TITLE … F_DESC
    pub error: Option<String>,
    /// Whether the epic autocomplete dropdown is visible.
    pub epic_dropdown_open: bool,
    /// Selected row in the epic dropdown.
    pub epic_dropdown_sel: usize,
    /// Whether the due-date autocomplete dropdown is visible.
    pub due_date_dropdown_open: bool,
    /// Selected row in the due-date dropdown.
    pub due_date_dropdown_sel: usize,
    /// Subtasks (only populated when editing an existing issue).
    pub subtasks: Vec<SubtaskDraft>,
    /// Selected row in the subtask list.
    pub subtask_sel: usize,
    /// Whether keyboard focus is in the subtask list.
    pub in_subtask_list: bool,
    /// Whether the currently selected subtask title is being edited.
    pub subtask_editing: bool,
    /// Set by a first Esc on a form with unsaved changes; a second Esc discards them.
    pub confirm_discard: bool,
    /// Field values when the form opened (see `is_dirty`).
    baseline: String,
}

impl IssueForm {
    pub fn new() -> Self {
        let mut form = IssueForm {
            editing_id: None,
            sprint_id: None,
            title: String::new(),
            story_points: String::from("1"),
            epic: String::new(),
            status_idx: 0,
            due_date: String::new(),
            description: String::new(),
            focused_field: F_TITLE,
            error: None,
            epic_dropdown_open: false,
            epic_dropdown_sel: 0,
            due_date_dropdown_open: false,
            due_date_dropdown_sel: 0,
            subtasks: Vec::new(),
            subtask_sel: 0,
            in_subtask_list: false,
            subtask_editing: false,
            confirm_discard: false,
            baseline: String::new(),
        };
        form.seal();
        form
    }

    pub fn from_issue(issue: &Issue) -> Self {
        let mut form = IssueForm::new();
        form.editing_id = Some(issue.id);
        form.title = issue.title.clone();
        form.story_points = issue.story_points.to_string();
        form.epic = issue.epic.clone();
        form.status_idx = issue.status.index();
        form.due_date = issue.due_date_str();
        form.description = issue.description.clone().unwrap_or_default();
        form.seal();
        form
    }

    /// Returns a mutable reference to the string buffer of the currently focused text field,
    /// or None if the focused field is not a text field (e.g. status).
    pub fn active_text_field(&mut self) -> Option<&mut String> {
        match self.focused_field {
            F_TITLE => Some(&mut self.title),
            F_EPIC => Some(&mut self.epic),
            F_SP => Some(&mut self.story_points),
            F_DUE => Some(&mut self.due_date),
            F_DESC => Some(&mut self.description),
            _ => None, // status cycles with h/l
        }
    }

    pub fn field_count() -> usize {
        6
    }

    pub fn validate(&self) -> Option<String> {
        if self.title.trim().is_empty() {
            return Some("Title is required.".into());
        }
        match self.story_points.parse::<f64>() {
            Err(_) => return Some("Story points must be a number (e.g. 1, 2.5).".into()),
            Ok(v) if v <= 0.0 => return Some("Story points must be positive.".into()),
            _ => {}
        }
        if self.epic.trim().is_empty() {
            return Some("Epic is required.".into());
        }
        if !self.due_date.is_empty()
            && NaiveDate::parse_from_str(&self.due_date, "%Y-%m-%d").is_err()
        {
            return Some("Due date must be YYYY-MM-DD.".into());
        }
        None
    }

    /// Everything the user can change, as one string. Untitled new subtasks don't count.
    fn fingerprint(&self) -> String {
        let subtasks: Vec<String> = self
            .subtasks
            .iter()
            .filter(|s| s.id.is_some() || !s.title.trim().is_empty())
            .map(|s| format!("{:?}|{}|{}|{}", s.id, s.title, s.status_idx, s.deleted))
            .collect();
        [
            self.title.as_str(),
            self.epic.as_str(),
            self.story_points.as_str(),
            self.status_idx.to_string().as_str(),
            self.due_date.as_str(),
            self.description.as_str(),
            subtasks.join("\n").as_str(),
        ]
        .join("\u{1f}")
    }

    /// Record the current values as the unchanged state.
    pub fn seal(&mut self) {
        self.baseline = self.fingerprint();
    }

    pub fn is_dirty(&self) -> bool {
        self.fingerprint() != self.baseline
    }

    /// Index into `subtasks` of the selected visible (not deleted) row.
    fn selected_subtask_index(&self) -> Option<usize> {
        self.subtasks
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.deleted)
            .nth(self.subtask_sel)
            .map(|(i, _)| i)
    }

    fn visible_subtask_count(&self) -> usize {
        self.subtasks.iter().filter(|s| !s.deleted).count()
    }

    /// Append an empty subtask and start editing its title.
    fn add_subtask(&mut self) {
        self.subtasks.push(SubtaskDraft::blank());
        self.in_subtask_list = true;
        self.subtask_sel = self.visible_subtask_count().saturating_sub(1);
        self.subtask_editing = true;
        self.epic_dropdown_open = false;
        self.due_date_dropdown_open = false;
    }
}

// ── Sprint form ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SprintForm {
    pub editing_id: Option<i64>,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub is_active: bool,
    pub focused_field: usize, // 0=name 1=start 2=end 3=active
    pub error: Option<String>,
    /// The sprint being edited and its membership log (for the burnup chart).
    pub sprint: Option<Sprint>,
    pub entries: Vec<SprintEntry>,
}

fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

/// The Monday of the work week to plan for: this week's, or next week's at the weekend.
fn upcoming_monday(today: NaiveDate) -> NaiveDate {
    if today.weekday().num_days_from_monday() >= 5 {
        monday_of(today) + Duration::days(7)
    } else {
        monday_of(today)
    }
}

impl SprintForm {
    /// A new, active Monday–Friday sprint starting on `start`, named after its ISO week.
    fn starting(start: NaiveDate) -> Self {
        SprintForm {
            editing_id: None,
            name: format!("Week {}", start.iso_week().week()),
            start_date: start.format("%Y-%m-%d").to_string(),
            end_date: (start + Duration::days(4)).format("%Y-%m-%d").to_string(),
            is_active: true,
            focused_field: 0,
            error: None,
            sprint: None,
            entries: Vec::new(),
        }
    }

    /// A new sprint for this work week (next week's at the weekend).
    pub fn new() -> Self {
        Self::starting(upcoming_monday(Local::now().date_naive()))
    }

    /// A new sprint following `prev`: the Monday after the week it ends in, and never
    /// a week that's already over.
    pub fn next_after(prev: &Sprint) -> Self {
        let after_prev = monday_of(prev.end_date) + Duration::days(7);
        Self::starting(after_prev.max(upcoming_monday(Local::now().date_naive())))
    }

    pub fn from_sprint(s: &Sprint, entries: Vec<SprintEntry>) -> Self {
        SprintForm {
            editing_id: Some(s.id),
            name: s.name.clone(),
            start_date: s.start_date.format("%Y-%m-%d").to_string(),
            end_date: s.end_date.format("%Y-%m-%d").to_string(),
            is_active: s.is_active,
            focused_field: 0,
            error: None,
            sprint: Some(s.clone()),
            entries,
        }
    }

    pub fn active_text_field(&mut self) -> Option<&mut String> {
        match self.focused_field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.start_date),
            2 => Some(&mut self.end_date),
            _ => None, // 3 = boolean toggle
        }
    }

    pub fn field_count() -> usize {
        4
    }

    pub fn validate(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("Name is required.".into());
        }
        let Ok(start) = NaiveDate::parse_from_str(&self.start_date, "%Y-%m-%d") else {
            return Some("Start date must be YYYY-MM-DD.".into());
        };
        let Ok(end) = NaiveDate::parse_from_str(&self.end_date, "%Y-%m-%d") else {
            return Some("End date must be YYYY-MM-DD.".into());
        };
        if end < start {
            return Some("End date is before the start date.".into());
        }
        None
    }
}

// ── Popup ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum Popup {
    NewIssue(IssueForm),
    EditIssue(IssueForm),
    SprintManager(SprintForm),
    ConfirmDelete(i64, String), // (issue_id, issue_title)
    ConfirmDeleteSprint(i64, String), // (sprint_id, sprint_name)
    Trash { items: Vec<Issue>, sel: usize },
    Help,
    /// Gantt epic detail: epic name, issues list, search query, selected index, scroll offset
    GanttEpicDetail {
        epic: String,
        issues: Vec<Issue>,
        search: String,
        search_active: bool,
        sel: usize,
        scroll: usize,
    },
}

/// Issues in an epic detail popup that match its search box.
pub fn epic_detail_filter<'a>(issues: &'a [Issue], search: &str) -> Vec<&'a Issue> {
    let q = search.to_lowercase();
    issues
        .iter()
        .filter(|i| {
            q.is_empty()
                || i.title.to_lowercase().contains(&q)
                || i.status.label().to_lowercase().contains(&q)
        })
        .collect()
}

// ── Undo ─────────────────────────────────────────────────────────────────────

/// Captures enough state to reverse a single user action.
#[derive(Clone)]
pub enum UndoAction {
    /// A status change: put the issue row back as it was (its parent is re-derived).
    StatusChange { before: Issue },
    /// A saved edit form: restore the issue and its subtasks.
    IssueEdit {
        before: Issue,
        subtasks_before: Vec<Issue>,
        /// Subtasks created by that save, moved to the trash on undo.
        new_subtask_ids: Vec<i64>,
    },
    /// A new issue: undo moves it to the trash.
    Created { issue_id: i64 },
    /// Issue was soft-deleted — undo by restoring from trash.
    SoftDelete { issue_id: i64 },
    /// Sprint membership toggled: restore the assignment and its sprint log exactly.
    SprintToggle { issue_id: i64, old_sprint_id: Option<i64>, log_before: Vec<SprintLogRow> },
    /// Two issues had their ranks swapped.
    RankSwap { id_a: i64, id_b: i64 },
}

// ── Frog ──────────────────────────────────────────────────────────────────────

/// A frog to pick ("eat the frog"): the active sprint's unstarted issue that has been
/// put off the longest (most carry-overs, then oldest, then biggest). Falls back to
/// in-progress issues when everything has been started.
fn pick_frog(issues: &[Issue], sprint: Option<&Sprint>) -> Option<i64> {
    let sid = sprint?.id;
    let unfinished = issues.iter().filter(|i| {
        i.sprint_id == Some(sid) && i.parent_id.is_none() && i.status != Status::Done
    });
    unfinished
        .min_by(|a, b| {
            (a.status != Status::Todo)
                .cmp(&(b.status != Status::Todo))
                .then(b.carry_count.cmp(&a.carry_count))
                .then(a.created_at.cmp(&b.created_at))
                .then(b.story_points.partial_cmp(&a.story_points).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.id.cmp(&b.id))
        })
        .map(|i| i.id)
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub view: View,
    pub db: Db,
    pub issues: Vec<Issue>,
    /// Stable display snapshot — order and membership are frozen for the current
    /// view session.  Only statuses are patched in-place when the user changes
    /// them.  The snapshot is refreshed when the user switches to a different
    /// view, so disappearances and reorderings are deferred until then.
    pub display_issues: Vec<Issue>,
    pub active_sprint: Option<Sprint>,
    /// Index into the flat display list produced by `backlog_items()`
    pub backlog_sel: usize,
    /// First visible row of the backlog list (kept between frames for smooth scrolling).
    pub backlog_offset: usize,
    /// Kanban: current column (0=Todo 1=InProgress 2=Done)
    pub kanban_col: usize,
    /// Kanban: selected row within each column (parent issues panel)
    pub kanban_rows: [usize; 3],
    /// Kanban: first visible card of each column.
    pub kanban_offsets: [usize; 3],
    /// Kanban: 0=parent panel focused, 1=subtask panel focused
    pub kanban_panel: usize,
    /// Kanban: selected row in the subtask panel (one flat list for the focused parent)
    pub kanban_sub_row: usize,
    /// Kanban: index into the list of parents-with-subtasks for the subtask panel
    pub kanban_sub_parent_idx: usize,
    /// Gantt scroll offset (epic rows)
    pub gantt_scroll: usize,
    /// Help popup scroll offset (rows), for screens too short to show it whole
    pub help_scroll: u16,
    /// Gantt selected epic index (for Enter to open detail popup)
    pub gantt_sel: usize,
    pub popup: Option<Popup>,
    /// Popup to return to when the issue form closes (editing from an epic's detail).
    return_popup: Option<Popup>,
    pub status_msg: Option<String>,
    /// When the current status_msg was set (for auto-expiry).
    pub status_msg_at: Option<Instant>,
    /// Whether to show Done issues in the backlog.
    pub show_completed: bool,
    /// Live search query (empty = no filter).
    pub search_query: String,
    /// Whether the search bar is in focus / receiving input.
    pub search_active: bool,
    /// Sprint history: list of all sprints (loaded when view opens).
    pub history_sprints: Vec<Sprint>,
    /// Sprint history: selected sprint index.
    pub history_sel: usize,
    pub history_offset: usize,
    /// Sprint history: every sprint's membership log with outcomes, keyed by sprint id.
    pub history_entries: HashMap<i64, Vec<SprintEntry>>,
    /// Sprint history: 0 = sprint list focused, 1 = issue list focused.
    pub history_focus: usize,
    pub history_issue_sel: usize,
    pub history_issue_offset: usize,
    /// Undo stack — up to 50 entries, most-recent last (pop from end).
    pub undo_stack: Vec<UndoAction>,
    /// Today's frog, the issue to do first (see `refresh_frog`).
    pub frog_id: Option<i64>,
    /// The day `frog_id` was settled for: there's one frog a day.
    frog_day: Option<NaiveDate>,
    /// Last seen `PRAGMA data_version`, to notice writes from other processes.
    data_version: Option<i64>,
    /// Cached unique epic names, most recently started first — refreshed on reload().
    epics_cache: Vec<String>,
    /// Issue IDs that were just marked Done in the backlog this session.
    /// These are kept visible until flush_display() is called (i.e., view switch).
    recently_completed: HashSet<i64>,
}

impl App {
    pub fn new(db: Db) -> Result<Self> {
        // Restore persisted show_completed preference (defaults to true)
        let show_completed = db
            .get_setting("show_completed")
            .ok()
            .flatten()
            .map(|v| v != "false")
            .unwrap_or(true);
        let setting = |key: &str| db.get_setting(key).ok().flatten();
        let mut frog_id: Option<i64> = setting("frog_id").and_then(|v| v.parse().ok());
        let mut frog_day = setting("frog_day")
            .and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok());
        // 2.3.0 kept a pinned frog without a day; it becomes today's frog.
        if let Some(pin) = setting("frog_pin").and_then(|v| v.parse().ok()) {
            frog_id = Some(pin);
            frog_day = Some(Local::now().date_naive());
            let _ = db.delete_setting("frog_pin");
        }
        let data_version = db.data_version().ok();
        let mut app = App {
            view: View::Backlog,
            db,
            issues: Vec::new(),
            display_issues: Vec::new(),
            active_sprint: None,
            backlog_sel: 0,
            backlog_offset: 0,
            kanban_col: 0,
            kanban_rows: [0; 3],
            kanban_offsets: [0; 3],
            kanban_panel: 0,
            kanban_sub_row: 0,
            kanban_sub_parent_idx: 0,
            gantt_scroll: 0,
            help_scroll: 0,
            gantt_sel: 0,
            popup: None,
            return_popup: None,
            status_msg: None,
            status_msg_at: None,
            show_completed,
            search_query: String::new(),
            search_active: false,
            history_sprints: Vec::new(),
            history_sel: 0,
            history_offset: 0,
            history_entries: HashMap::new(),
            history_focus: 0,
            history_issue_sel: 0,
            history_issue_offset: 0,
            undo_stack: Vec::new(),
            frog_id,
            frog_day,
            data_version,
            epics_cache: Vec::new(),
            recently_completed: HashSet::new(),
        };
        app.reload()?;
        app.save_frog();
        app.flush_display();
        if let Some(frog) = app.frog().filter(|f| f.status != Status::Done) {
            let msg = format!("Frog: {}. Start with this one.", frog.title);
            app.set_status(msg);
        }
        Ok(app)
    }

    pub fn reload(&mut self) -> Result<()> {
        let frog_was_open = self.frog().is_some_and(|f| f.status != Status::Done);
        let prev_frog = self.frog_id;
        self.issues = self.db.get_all_issues()?;
        self.active_sprint = self.db.get_active_sprint()?;
        self.epics_cache = Self::build_epics_cache(&self.issues);

        self.refresh_frog();
        if frog_was_open && self.frog_id == prev_frog {
            if let Some(title) = self.frog().filter(|f| f.status == Status::Done).map(|f| f.title.clone()) {
                self.set_status(format!("Frog done: {title}. The next one comes tomorrow."));
            }
        }

        // Clamp selection to valid range
        let n = self.backlog_items().len();
        if self.backlog_sel >= n && n > 0 {
            self.backlog_sel = n - 1;
        }
        Ok(())
    }

    /// Pick up writes made by another process (e.g. `scrumtui add` from a script or
    /// an AI assistant) while the TUI is open. Called once per event-loop tick.
    pub fn sync_external_changes(&mut self) {
        let Ok(version) = self.db.data_version() else { return };
        if self.data_version == Some(version) {
            return;
        }
        self.data_version = Some(version);
        let known: HashSet<i64> = self.issues.iter().map(|i| i.id).collect();
        let keep = self.focused_issue_id();
        if self.reload().is_err() {
            return;
        }
        self.flush_display();
        if self.view == View::SprintHistory {
            self.load_sprint_history();
        }
        if let Some(id) = keep {
            self.focus_issue(id);
        }
        let added: Vec<&Issue> = self
            .issues
            .iter()
            .filter(|i| i.parent_id.is_none() && !known.contains(&i.id))
            .collect();
        let msg = match added.as_slice() {
            [] => "Reloaded: the database was changed outside scrumtui.".to_string(),
            [one] => format!("Added from outside: #{} {}", one.id, one.title),
            many => format!("{} issues added from outside scrumtui.", many.len()),
        };
        self.set_status(msg);
    }

    fn build_epics_cache(issues: &[Issue]) -> Vec<String> {
        use chrono::NaiveDateTime;
        // Compute the earliest created_at across all top-level issues per epic.
        let mut epic_starts: HashMap<String, NaiveDateTime> = HashMap::new();
        for issue in issues.iter().filter(|i| !i.epic.is_empty() && i.parent_id.is_none()) {
            let entry = epic_starts.entry(issue.epic.clone()).or_insert(issue.created_at);
            if issue.created_at < *entry {
                *entry = issue.created_at;
            }
        }
        let mut epics: Vec<String> = epic_starts.keys().cloned().collect();
        // Sort by earliest start date descending — most recently started epic first.
        epics.sort_by(|a, b| epic_starts[b].cmp(&epic_starts[a]));
        epics
    }

    /// Reload from the database and rebuild everything the current view shows.
    fn refresh(&mut self) {
        let _ = self.reload();
        self.flush_display();
        if self.view == View::SprintHistory {
            self.load_sprint_history();
        }
    }

    /// Refresh display_issues from the live issues list.  Called on view switch
    /// so that order/visibility changes take effect when leaving a view.
    pub fn flush_display(&mut self) {
        self.display_issues = Self::display_snapshot(&self.issues, self.active_sprint.as_ref());
        self.recently_completed.clear();
        self.kanban_clamp();
    }

    /// Build the display ordering from the live issues list. Unfinished issues in the
    /// active sprint float to the top of the sprint, longest-standing first (most
    /// carry-overs, then oldest); everything else keeps the DB (rank) order.
    fn display_snapshot(issues: &[Issue], sprint: Option<&Sprint>) -> Vec<Issue> {
        let sprint_id = sprint.map(|s| s.id);
        let unfinished = |i: &Issue| {
            sprint_id.is_some() && i.sprint_id == sprint_id
                && i.parent_id.is_none() && i.status != Status::Done
        };
        let mut out = issues.to_vec();
        // Stable sort: only unfinished sprint issues are reordered among themselves.
        out.sort_by(|a, b| match (unfinished(a), unfinished(b)) {
            (true, true) => b.carry_count.cmp(&a.carry_count)
                .then(a.created_at.cmp(&b.created_at))
                .then(a.id.cmp(&b.id)),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => std::cmp::Ordering::Equal,
        });
        out
    }

    /// Patch the status (and completed_at) of an issue in display_issues without
    /// changing its position or removing it.  This lets the user see the new
    /// status symbol immediately while keeping the item in place.
    fn patch_display_status(&mut self, issue_id: i64, new_status: &Status) {
        if let Some(entry) = self.display_issues.iter_mut().find(|i| i.id == issue_id) {
            let now = chrono::Local::now().naive_local();
            entry.completed_at = if new_status == &Status::Done {
                Some(now)
            } else {
                None
            };
            entry.status = new_status.clone();
        }
        if new_status == &Status::Done {
            self.recently_completed.insert(issue_id);
        } else {
            self.recently_completed.remove(&issue_id);
        }
    }

    /// Patch a subtask's parent in the display snapshot after its status was re-derived.
    fn patch_parent_display(&mut self, parent_id: Option<i64>) {
        if let Some(pid) = parent_id {
            if let Some(status) = self.issue_by_id(pid).map(|p| p.status.clone()) {
                self.patch_display_status(pid, &status);
            }
        }
    }

    /// Set a status message that will display for ~3 seconds.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
        self.status_msg_at = Some(Instant::now());
    }

    /// Returns the current status message if it hasn't expired (3 s TTL).
    pub fn current_status(&self) -> Option<&str> {
        match (&self.status_msg, &self.status_msg_at) {
            (Some(msg), Some(at)) if at.elapsed().as_secs() < 3 => Some(msg),
            _ => None,
        }
    }

    /// Push an action onto the undo stack (capped at 50).
    fn push_undo(&mut self, action: UndoAction) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(action);
    }

    /// Pop the most recent undo entry and reverse it.
    pub fn undo(&mut self) {
        let Some(action) = self.undo_stack.pop() else {
            self.set_status("Nothing to undo.");
            return;
        };
        let (result, msg, focus) = match action {
            UndoAction::StatusChange { before } => {
                let result = self.db.restore_issue_to_snapshot(&before).and_then(|_| {
                    match before.parent_id {
                        Some(pid) => self.db.update_parent_status_from_children(pid),
                        None => Ok(()),
                    }
                });
                if result.is_ok() {
                    // Keep the row in place, like the original status change did.
                    let _ = self.reload();
                    self.patch_display_status(before.id, &before.status);
                    self.patch_parent_display(before.parent_id);
                    self.set_status(format!("Undid status change → {}", before.status.label()));
                    return;
                }
                (result, String::new(), None)
            }
            UndoAction::IssueEdit { before, subtasks_before, new_subtask_ids } => {
                let mut result = self.db.restore_issue_to_snapshot(&before);
                for sub in &subtasks_before {
                    result = result.and_then(|_| self.db.restore_issue_to_snapshot(sub));
                }
                for id in &new_subtask_ids {
                    result = result.and_then(|_| self.db.delete_issue(*id));
                }
                (result, "Undid issue edit.".to_string(), Some(before.id))
            }
            UndoAction::Created { issue_id } => (
                self.db.delete_issue(issue_id),
                "Undid issue creation (moved to trash).".to_string(),
                None,
            ),
            UndoAction::SoftDelete { issue_id } => (
                self.db.restore_issue(issue_id),
                "Undid delete (issue restored).".to_string(),
                Some(issue_id),
            ),
            UndoAction::SprintToggle { issue_id, old_sprint_id, log_before } => (
                self.db.restore_sprint_membership(issue_id, old_sprint_id, &log_before),
                if old_sprint_id.is_some() {
                    "Undid sprint removal (back in sprint).".to_string()
                } else {
                    "Undid sprint add (back in backlog).".to_string()
                },
                Some(issue_id),
            ),
            UndoAction::RankSwap { id_a, id_b } => (
                self.db.swap_rank(id_a, id_b),
                "Undid rank change.".to_string(),
                Some(id_a),
            ),
        };
        match result {
            Err(e) => self.set_status(format!("Undo failed: {e}")),
            Ok(()) => {
                self.refresh();
                if let Some(id) = focus {
                    self.focus_issue(id);
                }
                self.set_status(msg);
            }
        }
    }

    // ── Derived data ───────────────────────────────────────────────────────────

    /// Build the flat display list for the backlog view, respecting filters.
    /// Uses display_issues (stable snapshot) so order and visibility don't
    /// change mid-session when statuses are updated.
    pub fn backlog_items(&self) -> Vec<BacklogItem> {
        let mut items: Vec<BacklogItem> = Vec::new();
        let q = self.search_query.to_lowercase();

        // Build children lookup from the stable snapshot: parent_id → Vec<Issue>
        // DB returns newest-first; we reverse each vec so subtasks appear in creation order.
        let mut children_map: HashMap<i64, Vec<Issue>> = HashMap::new();
        for issue in &self.display_issues {
            if let Some(pid) = issue.parent_id {
                children_map.entry(pid).or_default().push(issue.clone());
            }
        }
        for subs in children_map.values_mut() {
            subs.reverse();
        }

        let text_matches = |issue: &Issue| -> bool {
            let haystack = format!(
                "{} {} {}",
                issue.title.to_lowercase(),
                issue.epic.to_lowercase(),
                issue.description.as_deref().unwrap_or("").to_lowercase()
            );
            fuzzy_match(&q, &haystack)
        };
        // A parent matches when it or one of its subtasks does.
        let search_matches = |issue: &Issue| -> bool {
            q.is_empty()
                || text_matches(issue)
                || children_map
                    .get(&issue.id)
                    .is_some_and(|subs| subs.iter().any(|s| text_matches(s)))
        };

        let emit_with_subtasks = |items: &mut Vec<BacklogItem>, issue: Issue, in_sprint: bool| {
            let id = issue.id;
            items.push(BacklogItem::Issue(issue, in_sprint));
            if let Some(subs) = children_map.get(&id) {
                for sub in subs {
                    items.push(BacklogItem::Subtask(sub.clone(), in_sprint));
                }
            }
        };

        if let Some(sprint) = &self.active_sprint {
            // Sprint issues: always show Done; search filter still applies.
            let sprint_issues: Vec<Issue> = self
                .display_issues
                .iter()
                .filter(|i| i.sprint_id == Some(sprint.id) && i.parent_id.is_none())
                .filter(|i| search_matches(i))
                .cloned()
                .collect();
            if !sprint_issues.is_empty() {
                items.push(BacklogItem::SprintHeader(sprint.clone()));
                for issue in sprint_issues {
                    emit_with_subtasks(&mut items, issue, true);
                }
                items.push(BacklogItem::SprintFooter);
            }
        }

        let backlog: Vec<Issue> = self
            .display_issues
            .iter()
            .filter(|i| {
                let in_sprint = self.active_sprint.as_ref().map(|s| i.sprint_id == Some(s.id)).unwrap_or(false);
                !in_sprint && i.parent_id.is_none()
            })
            .filter(|i| {
                if !self.show_completed && i.status == Status::Done && !self.recently_completed.contains(&i.id) { return false; }
                search_matches(i)
            })
            .cloned()
            .collect();

        if !backlog.is_empty() {
            items.push(BacklogItem::BacklogHeader);
            for issue in backlog {
                emit_with_subtasks(&mut items, issue, false);
            }
        }

        items
    }

    /// The issue currently selected in the backlog (if any).
    /// Selecting a subtask returns the parent issue so it can be edited.
    pub fn selected_issue(&self) -> Option<Issue> {
        let items = self.backlog_items();
        match items.get(self.backlog_sel) {
            Some(BacklogItem::Issue(issue, _)) => Some(issue.clone()),
            Some(BacklogItem::Subtask(sub, _)) => {
                sub.parent_id.and_then(|pid| self.display_issues.iter().find(|i| i.id == pid).cloned())
            }
            _ => None,
        }
    }

    /// Id of the selected backlog row itself (a subtask row gives the subtask).
    fn selected_backlog_row_id(&self) -> Option<i64> {
        match self.backlog_items().get(self.backlog_sel) {
            Some(BacklogItem::Issue(i, _)) | Some(BacklogItem::Subtask(i, _)) => Some(i.id),
            _ => None,
        }
    }

    /// The issue the cursor is on in the current view, for keeping focus across reloads.
    fn focused_issue_id(&self) -> Option<i64> {
        match self.view {
            View::Backlog => self.selected_backlog_row_id(),
            View::Kanban => self.kanban_selected_issue().map(|i| i.id),
            _ => None,
        }
    }

    /// Move the cursor of the current view onto an issue (or subtask), if it's shown.
    pub fn focus_issue(&mut self, id: i64) {
        match self.view {
            View::Backlog => {
                let pos = self.backlog_items().iter().position(|bi| {
                    matches!(bi, BacklogItem::Issue(i, _) | BacklogItem::Subtask(i, _) if i.id == id)
                });
                if let Some(pos) = pos {
                    self.backlog_sel = pos;
                }
            }
            View::Kanban => {
                let Some(issue) = self.display_issues.iter().find(|i| i.id == id).cloned() else {
                    return;
                };
                if let Some(pid) = issue.parent_id {
                    let parents = self.sprint_parents_with_subtasks();
                    if let Some(p) = parents.iter().position(|p| p.id == pid) {
                        self.kanban_sub_parent_idx = p;
                        if let Some(pos) = self.sprint_subtasks_flat().iter().position(|s| s.id == id) {
                            self.kanban_sub_row = pos;
                            self.kanban_panel = 1;
                        }
                    }
                } else {
                    let col = issue.status.index();
                    let parents = self.sprint_parents_by_status(&issue.status);
                    if let Some(pos) = parents.iter().position(|i| i.id == id) {
                        self.kanban_col = col;
                        self.kanban_rows[col] = pos;
                        self.kanban_panel = 0;
                    }
                }
            }
            _ => {}
        }
    }

    /// The top-level issue next to `id` in the same backlog section (the one below,
    /// or above when `id` is last): where the cursor goes when `id` leaves the list.
    fn backlog_neighbor(&self, id: i64) -> Option<i64> {
        let rows: Vec<(i64, bool)> = self
            .backlog_items()
            .iter()
            .filter_map(|bi| match bi {
                BacklogItem::Issue(i, in_sprint) => Some((i.id, *in_sprint)),
                _ => None,
            })
            .collect();
        let pos = rows.iter().position(|(rid, _)| *rid == id)?;
        let section = rows[pos].1;
        rows[pos + 1..]
            .iter()
            .find(|(_, s)| *s == section)
            .or_else(|| rows[..pos].iter().rev().find(|(_, s)| *s == section))
            .map(|(rid, _)| *rid)
    }

    /// Look up an issue by id from the loaded cache.
    pub fn issue_by_id(&self, id: i64) -> Option<&Issue> {
        self.issues.iter().find(|i| i.id == id)
    }

    /// Today's frog, if there is one (it may already be done).
    pub fn frog(&self) -> Option<&Issue> {
        self.frog_id.and_then(|id| self.issue_by_id(id))
    }

    /// Settle today's frog. There's one a day: once settled it stays, whether it's
    /// started, finished or left alone. On a new day an unfinished frog carries
    /// over; a finished one is replaced by the issue that has been put off longest.
    fn refresh_frog(&mut self) {
        let today = Local::now().date_naive();
        if self.frog_day == Some(today) {
            return;
        }
        let unfinished = self
            .frog_id
            .filter(|id| self.issue_by_id(*id).is_some_and(|i| i.status != Status::Done));
        self.frog_id = unfinished.or_else(|| pick_frog(&self.issues, self.active_sprint.as_ref()));
        // Nothing to pick yet (no sprint, or nothing open): try again later today.
        if self.frog_id.is_some() {
            self.frog_day = Some(today);
        }
        self.save_frog();
    }

    fn save_frog(&self) {
        let _ = match self.frog_id {
            Some(id) => self.db.set_setting("frog_id", &id.to_string()),
            None => self.db.delete_setting("frog_id"),
        };
        let _ = match self.frog_day {
            Some(day) => self.db.set_setting("frog_day", &day.format("%Y-%m-%d").to_string()),
            None => self.db.delete_setting("frog_day"),
        };
    }

    /// All sprint parent issues that have at least one subtask, in order.
    pub fn sprint_parents_with_subtasks(&self) -> Vec<Issue> {
        match &self.active_sprint {
            Some(s) => self
                .display_issues
                .iter()
                .filter(|i| i.sprint_id == Some(s.id) && i.parent_id.is_none())
                .filter(|p| self.display_issues.iter().any(|i| i.parent_id == Some(p.id)))
                .cloned()
                .collect(),
            None => vec![],
        }
    }

    /// The currently focused parent for the subtask panel (based on kanban_sub_parent_idx).
    pub fn kanban_sub_parent(&self) -> Option<Issue> {
        let parents = self.sprint_parents_with_subtasks();
        let idx = self.kanban_sub_parent_idx.min(parents.len().saturating_sub(1));
        parents.into_iter().nth(idx)
    }

    /// Top-level sprint issues only (no subtasks), by status.
    pub fn sprint_parents_by_status(&self, status: &Status) -> Vec<Issue> {
        match &self.active_sprint {
            Some(s) => self
                .display_issues
                .iter()
                .filter(|i| i.sprint_id == Some(s.id) && &i.status == status && i.parent_id.is_none())
                .cloned()
                .collect(),
            None => vec![],
        }
    }

    /// Whether any sprint parent issue has subtasks.
    pub fn sprint_has_any_subtasks(&self) -> bool {
        !self.sprint_parents_with_subtasks().is_empty()
    }

    /// The currently selected parent issue in the kanban parent panel.
    pub fn kanban_selected_parent(&self) -> Option<Issue> {
        let status = Status::from_index(self.kanban_col);
        let parents = self.sprint_parents_by_status(&status);
        parents.into_iter().nth(self.kanban_rows[self.kanban_col])
    }

    /// All subtasks of the focused parent, across all statuses (flat list for the sub panel).
    pub fn sprint_subtasks_flat(&self) -> Vec<Issue> {
        match self.kanban_sub_parent() {
            Some(parent) => {
                let mut subs: Vec<Issue> = self
                    .display_issues
                    .iter()
                    .filter(|i| i.parent_id == Some(parent.id))
                    .cloned()
                    .collect();
                // DB returns newest-first; reverse to show subtasks in creation order.
                subs.reverse();
                subs
            }
            None => vec![],
        }
    }

    /// The currently selected subtask in the kanban subtask panel (flat list).
    pub fn kanban_selected_subtask(&self) -> Option<Issue> {
        self.sprint_subtasks_flat().into_iter().nth(self.kanban_sub_row)
    }

    /// Subtask count for a given parent issue.
    pub fn subtask_counts(&self, parent_id: i64) -> (usize, usize) {
        let all: Vec<_> = self.issues.iter().filter(|i| i.parent_id == Some(parent_id)).collect();
        let done = all.iter().filter(|i| i.status == Status::Done).count();
        (done, all.len())
    }

    /// Whether a given issue has any subtasks.
    pub fn has_subtasks(&self, issue_id: i64) -> bool {
        self.issues.iter().any(|i| i.parent_id == Some(issue_id))
    }

    /// Subtasks of an issue in creation order.
    fn subtasks_of(&self, parent_id: i64) -> Vec<&Issue> {
        let mut subs: Vec<&Issue> = self.issues.iter().filter(|i| i.parent_id == Some(parent_id)).collect();
        // DB returns newest-first; reverse to get creation order.
        subs.reverse();
        subs
    }

    /// Build SubtaskDraft list from loaded issues for a given parent.
    pub fn subtask_drafts_for(&self, parent_id: i64) -> Vec<SubtaskDraft> {
        self.subtasks_of(parent_id)
            .into_iter()
            .map(|i| SubtaskDraft {
                id: Some(i.id),
                title: i.title.clone(),
                status_idx: i.status.index(),
                deleted: false,
            })
            .collect()
    }

    /// Unique epics, most recently started first (cached).
    pub fn epics(&self) -> &[String] {
        &self.epics_cache
    }

    /// Epics containing `query` (case-insensitive): exact match first, then prefix
    /// matches, then the rest, each group in `epics()` order.
    pub fn epic_matches(&self, query: &str) -> Vec<String> {
        let q = query.to_lowercase();
        let mut matches: Vec<String> = self
            .epics_cache
            .iter()
            .filter(|e| e.to_lowercase().contains(&q))
            .cloned()
            .collect();
        matches.sort_by_key(|e| {
            let e = e.to_lowercase();
            if e == q { 0 } else if e.starts_with(&q) { 1 } else { 2 }
        });
        matches
    }

    /// Due-date dropdown entries as (date, hint): today, tomorrow, the end of the active
    /// sprint and next Monday, plus every future due date already in use, by date.
    pub fn due_suggestions(&self) -> Vec<(String, String)> {
        let today = Local::now().date_naive();
        let mut dates: BTreeMap<NaiveDate, &str> = BTreeMap::new();
        for d in self.issues.iter().filter_map(|i| i.due_date).filter(|d| *d > today) {
            dates.insert(d, "");
        }
        let next_monday = monday_of(today) + Duration::days(7);
        dates.insert(next_monday, "next Mon");
        if let Some(s) = self.active_sprint.as_ref().filter(|s| s.end_date > today) {
            dates.insert(s.end_date, "sprint end");
        }
        dates.insert(today + Duration::days(1), "tomorrow");
        dates.insert(today, "today");
        dates
            .into_iter()
            .map(|(d, hint)| (d.format("%Y-%m-%d").to_string(), hint.to_string()))
            .collect()
    }

    /// Due-date suggestions whose date or hint contains `query`.
    pub fn due_matches(&self, query: &str) -> Vec<(String, String)> {
        let q = query.to_lowercase();
        self.due_suggestions()
            .into_iter()
            .filter(|(d, hint)| d.contains(&q) || hint.to_lowercase().contains(&q))
            .collect()
    }

    // ── Navigation helpers ─────────────────────────────────────────────────────

    fn is_selectable(item: &BacklogItem) -> bool {
        matches!(item, BacklogItem::Issue(_, _) | BacklogItem::Subtask(_, _))
    }

    fn backlog_nav(&mut self, nav: Nav) {
        let selectable: Vec<usize> = self
            .backlog_items()
            .iter()
            .enumerate()
            .filter(|(_, it)| Self::is_selectable(it))
            .map(|(i, _)| i)
            .collect();
        if selectable.is_empty() {
            return;
        }
        let pos = selectable
            .iter()
            .position(|&i| i >= self.backlog_sel)
            .unwrap_or(selectable.len() - 1);
        self.backlog_sel = selectable[nav_index(pos, selectable.len(), nav)];
    }

    pub fn backlog_sel_to_first_issue(&mut self) {
        self.backlog_nav(Nav::Top);
    }

    fn kanban_nav(&mut self, nav: Nav) {
        if self.kanban_panel == 0 {
            let col = self.kanban_col;
            let len = self.sprint_parents_by_status(&Status::from_index(col)).len();
            self.kanban_rows[col] = nav_index(self.kanban_rows[col], len, nav);
        } else {
            let len = self.sprint_subtasks_flat().len();
            self.kanban_sub_row = nav_index(self.kanban_sub_row, len, nav);
        }
    }

    /// Keep kanban selections inside their (possibly shrunk) lists.
    fn kanban_clamp(&mut self) {
        for col in 0..3 {
            let len = self.sprint_parents_by_status(&Status::from_index(col)).len();
            self.kanban_rows[col] = self.kanban_rows[col].min(len.saturating_sub(1));
        }
        let parents = self.sprint_parents_with_subtasks().len();
        self.kanban_sub_parent_idx = self.kanban_sub_parent_idx.min(parents.saturating_sub(1));
        let subs = self.sprint_subtasks_flat().len();
        self.kanban_sub_row = self.kanban_sub_row.min(subs.saturating_sub(1));
        if parents == 0 {
            self.kanban_panel = 0;
        }
    }

    /// Cycle the subtask panel to the previous (-1) or next (+1) parent.
    fn kanban_cycle_parent(&mut self, dir: i32) {
        let len = self.sprint_parents_with_subtasks().len();
        if len == 0 {
            return;
        }
        self.kanban_sub_parent_idx = if dir < 0 {
            (self.kanban_sub_parent_idx + len - 1) % len
        } else {
            (self.kanban_sub_parent_idx + 1) % len
        };
        self.kanban_sub_row = 0;
    }

    /// The currently focused issue (parent or subtask depending on kanban_panel).
    pub fn kanban_selected_issue(&self) -> Option<Issue> {
        if self.kanban_panel == 0 {
            self.kanban_selected_parent()
        } else {
            self.kanban_selected_subtask()
        }
    }

    /// Sprint history: the selected sprint's issues, done first, then still open,
    /// carried forward, and removed.
    pub fn history_rows(&self) -> Vec<&SprintEntry> {
        let Some(sprint) = self.history_sprints.get(self.history_sel) else {
            return Vec::new();
        };
        let mut rows: Vec<&SprintEntry> = self
            .history_entries
            .get(&sprint.id)
            .map(|v| v.iter().collect())
            .unwrap_or_default();
        rows.sort_by_key(|e| match e.outcome {
            Outcome::Done => 0,
            Outcome::Open => 1,
            Outcome::Carried => 2,
            Outcome::Removed => 3,
        });
        rows
    }

    // ── Opening popups ─────────────────────────────────────────────────────────

    /// Open the edit form for `issue`, optionally with the cursor on one of its subtasks.
    fn open_edit_form(&mut self, issue: &Issue, focus_subtask: Option<i64>) {
        let mut form = IssueForm::from_issue(issue);
        form.subtasks = self.subtask_drafts_for(issue.id);
        if let Some(pos) = focus_subtask.and_then(|sid| form.subtasks.iter().position(|d| d.id == Some(sid))) {
            form.in_subtask_list = true;
            form.subtask_sel = pos;
        }
        form.seal();
        self.popup = Some(Popup::EditIssue(form));
    }

    /// Open the new-issue form, optionally pre-assigned to a sprint and a status.
    fn open_new_form(&mut self, sprint_id: Option<i64>, status: Status) {
        let mut form = IssueForm::new();
        form.sprint_id = sprint_id;
        form.status_idx = status.index();
        form.seal();
        self.popup = Some(Popup::NewIssue(form));
    }

    /// Close the issue form, going back to the epic detail it was opened from (if any).
    fn close_issue_form(&mut self) {
        self.popup = None;
        if let Some(Popup::GanttEpicDetail { epic, sel, .. }) = self.return_popup.take() {
            self.open_epic_detail(epic, sel);
        }
    }

    fn open_epic_detail(&mut self, epic: String, sel: usize) {
        let mut issues: Vec<Issue> = self
            .issues
            .iter()
            .filter(|i| i.parent_id.is_none() && i.epic == epic)
            .cloned()
            .collect();
        // In progress first, then to do, then done (stable: keeps rank order within).
        issues.sort_by_key(|i| match i.status {
            Status::InProgress => 0,
            Status::Todo => 1,
            Status::Done => 2,
        });
        let sel = sel.min(issues.len().saturating_sub(1));
        self.popup = Some(Popup::GanttEpicDetail {
            epic,
            issues,
            search: String::new(),
            search_active: false,
            sel,
            scroll: 0,
        });
    }

    /// `S`: edit the active sprint, or set up the next one once it has ended.
    fn open_sprint_manager(&mut self) {
        let today = Local::now().date_naive();
        let form = match self.active_sprint.clone() {
            Some(s) if s.end_date < today => SprintForm::next_after(&s),
            Some(s) => self.sprint_edit_form(&s),
            None => SprintForm::new(),
        };
        self.popup = Some(Popup::SprintManager(form));
    }

    fn open_new_sprint_form(&mut self) {
        let form = match &self.active_sprint {
            Some(s) => SprintForm::next_after(s),
            None => SprintForm::new(),
        };
        self.popup = Some(Popup::SprintManager(form));
    }

    fn sprint_edit_form(&self, s: &Sprint) -> SprintForm {
        let entries = self
            .db
            .get_sprint_entries()
            .ok()
            .and_then(|mut m| m.remove(&s.id))
            .unwrap_or_default();
        SprintForm::from_sprint(s, entries)
    }

    fn open_trash(&mut self) {
        match self.db.get_trash() {
            Ok(items) => self.popup = Some(Popup::Trash { items, sel: 0 }),
            Err(e) => self.set_status(format!("Error: {e}")),
        }
    }

    fn switch_view(&mut self, view: View) {
        if self.view != view || view == View::SprintHistory {
            self.flush_display();
        }
        if view == View::SprintHistory {
            self.load_sprint_history();
        }
        self.view = view;
    }

    // ── Key handling ───────────────────────────────────────────────────────────

    /// Returns true when the app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Ctrl-C always quits
        if ctrl && key.code == KeyCode::Char('c') {
            return true;
        }

        if self.popup.is_some() {
            return self.handle_popup_key(key);
        }

        // Search bar intercepts typing when active (backlog only — clear it if we switched view)
        if self.search_active && self.view != View::Backlog {
            self.search_active = false;
            self.search_query.clear();
        }
        if self.search_active {
            self.handle_search_key(key);
            return false;
        }

        // Global keys
        match key.code {
            KeyCode::Char('z') if ctrl => {
                self.undo();
                return false;
            }
            KeyCode::Char('u') if !ctrl => {
                self.undo();
                return false;
            }
            KeyCode::Char('q') if !ctrl => return true,
            KeyCode::Char('?') => {
                self.popup = Some(Popup::Help);
                self.help_scroll = 0;
                return false;
            }
            KeyCode::Char('1') => { self.switch_view(View::Backlog); return false; }
            KeyCode::Char('2') => { self.switch_view(View::Kanban); return false; }
            KeyCode::Char('3') => { self.switch_view(View::Gantt); return false; }
            KeyCode::Char('4') => { self.switch_view(View::SprintHistory); return false; }
            KeyCode::Char('S') => {
                self.open_sprint_manager();
                return false;
            }
            KeyCode::Char('T') => {
                self.open_trash();
                return false;
            }
            _ => {}
        }

        let key = arrows_to_vim(key);
        match self.view {
            View::Backlog => self.handle_backlog_key(key),
            View::Kanban => self.handle_kanban_key(key),
            View::Gantt => self.handle_gantt_key(key),
            View::SprintHistory => self.handle_history_key(key),
        }

        false
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_active = false;
                self.search_query.clear();
                self.backlog_sel_to_first_issue();
            }
            KeyCode::Enter => {
                self.search_active = false;
                self.backlog_sel_to_first_issue();
            }
            KeyCode::Backspace => {
                if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                    delete_word(&mut self.search_query);
                } else {
                    self.search_query.pop();
                }
                self.backlog_sel_to_first_issue();
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word(&mut self.search_query);
                self.backlog_sel_to_first_issue();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.push(c);
                self.backlog_sel_to_first_issue();
            }
            _ => {}
        }
    }

    fn handle_backlog_key(&mut self, key: KeyEvent) {
        // Ctrl+j / Ctrl+k for rank reordering
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('j') => { self.backlog_move_rank(1); return; }
                KeyCode::Char('k') => { self.backlog_move_rank(-1); return; }
                _ => {}
            }
        }
        if let Some(nav) = nav_of(&key) {
            self.backlog_nav(nav);
            return;
        }
        match key.code {
            KeyCode::Char('/') => {
                self.search_active = true;
            }
            KeyCode::Esc if !self.search_query.is_empty() => {
                let keep = self.selected_backlog_row_id();
                self.search_query.clear();
                match keep {
                    Some(id) => self.focus_issue(id),
                    None => self.backlog_sel_to_first_issue(),
                }
            }
            KeyCode::Char('c') => {
                let keep = self.selected_issue().map(|i| i.id);
                let fallback = keep.and_then(|id| self.backlog_neighbor(id));
                self.show_completed = !self.show_completed;
                let _ = self.db.set_setting(
                    "show_completed",
                    if self.show_completed { "true" } else { "false" },
                );
                self.backlog_sel_to_first_issue();
                for id in [fallback, keep].into_iter().flatten() {
                    self.focus_issue(id);
                }
                self.set_status(if self.show_completed {
                    "Showing done backlog issues."
                } else {
                    "Hiding done backlog issues."
                });
            }
            KeyCode::Char('n') => {
                // In the sprint section, the new issue goes straight into the sprint.
                let in_sprint = matches!(
                    self.backlog_items().get(self.backlog_sel),
                    Some(BacklogItem::Issue(_, true)) | Some(BacklogItem::Subtask(_, true))
                        | Some(BacklogItem::SprintHeader(_)) | Some(BacklogItem::SprintFooter)
                );
                let sprint_id = self.active_sprint.as_ref().filter(|_| in_sprint).map(|s| s.id);
                self.open_new_form(sprint_id, Status::Todo);
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.selected_issue() {
                    let row = self.selected_backlog_row_id().filter(|id| *id != issue.id);
                    self.open_edit_form(&issue, row);
                }
            }
            KeyCode::Char('d') => {
                if let Some(issue) = self.selected_issue() {
                    self.popup = Some(Popup::ConfirmDelete(issue.id, issue.title.clone()));
                }
            }
            KeyCode::Char('s') => self.backlog_toggle_sprint(),
            KeyCode::Char('l') => self.backlog_advance_status(1),
            KeyCode::Char('h') => self.backlog_advance_status(-1),
            KeyCode::Char('y') => {
                if let Some(issue) = self.selected_issue() {
                    self.yank(&issue);
                }
            }
            KeyCode::Char('f') => {
                if let Some(issue) = self.selected_issue() {
                    self.toggle_frog(&issue);
                }
            }
            _ => {}
        }
    }

    /// `s`: move the selected issue into the active sprint, or back to the backlog.
    /// The cursor moves on to the next issue, so a run of `s` presses triages a list.
    fn backlog_toggle_sprint(&mut self) {
        let Some(issue) = self.selected_issue() else { return };
        let Some(sprint) = self.active_sprint.clone() else {
            self.set_status("No active sprint. Press S to start one.");
            return;
        };
        let new_sprint_id = if issue.sprint_id == Some(sprint.id) { None } else { Some(sprint.id) };
        let next = self.backlog_neighbor(issue.id);
        let log_before = self.db.sprint_log_for_issue(issue.id).unwrap_or_default();
        if let Err(e) = self.db.set_issue_sprint(issue.id, new_sprint_id) {
            self.set_status(format!("Error: {e}"));
            return;
        }
        self.push_undo(UndoAction::SprintToggle {
            issue_id: issue.id,
            old_sprint_id: issue.sprint_id,
            log_before,
        });
        let _ = self.reload();
        self.flush_display();
        self.focus_issue(next.unwrap_or(issue.id));
        self.set_status(if new_sprint_id.is_some() {
            format!("Moved to {}.", sprint.name)
        } else {
            "Moved to backlog.".to_string()
        });
    }

    /// Move the selected issue up (-1) or down (+1) in rank order.
    /// "Up" in the display (Ctrl-K) = higher rank value = appears first in rank DESC sort.
    fn backlog_move_rank(&mut self, dir: i32) {
        let items = self.backlog_items();
        // Only works on top-level issues (not subtasks, not headers)
        let current_issue = match items.get(self.backlog_sel) {
            Some(BacklogItem::Issue(i, _)) => i.clone(),
            _ => return,
        };
        if matches!(items.get(self.backlog_sel), Some(BacklogItem::Issue(_, true))) {
            self.set_status("Sprint order is automatic: unfinished issues first, oldest on top.");
            return;
        }

        // Collect peer issues (same sprint_id context) in current display order.
        // display_issues follows the DB's rank DESC order, so peers are already in display order.
        let peers: Vec<Issue> = items.iter().filter_map(|bi| {
            if let BacklogItem::Issue(i, _) = bi {
                if i.sprint_id == current_issue.sprint_id {
                    return Some(i.clone());
                }
            }
            None
        }).collect();

        let Some(pos) = peers.iter().position(|i| i.id == current_issue.id) else { return };

        // dir=-1 means move up in display (Ctrl-K) = swap with the peer above (lower index)
        // dir=+1 means move down in display (Ctrl-J) = swap with the peer below (higher index)
        let swap_pos = if dir < 0 {
            if pos == 0 { return; }
            pos - 1
        } else {
            if pos + 1 >= peers.len() { return; }
            pos + 1
        };

        let other = &peers[swap_pos];
        if let Err(e) = self.db.swap_rank(current_issue.id, other.id) {
            self.set_status(format!("Error: {e}"));
            return;
        }
        self.push_undo(UndoAction::RankSwap { id_a: current_issue.id, id_b: other.id });
        let _ = self.reload();
        self.flush_display();
        // Move the selection to follow the issue in its new position
        self.focus_issue(current_issue.id);
    }

    /// Advance (+1) or regress (-1) the selected backlog item's status.
    fn backlog_advance_status(&mut self, dir: i32) {
        let items = self.backlog_items();
        let target = match items.get(self.backlog_sel).cloned() {
            Some(BacklogItem::Issue(issue, _)) => {
                if self.has_subtasks(issue.id) {
                    self.set_status("Status is managed by subtasks.");
                    return;
                }
                issue
            }
            Some(BacklogItem::Subtask(sub, _)) => sub,
            _ => return,
        };
        let new_status = if dir > 0 { target.status.next() } else { target.status.prev() };
        if new_status == target.status {
            return;
        }
        if let Err(e) = self.db.update_issue_status(target.id, &new_status) {
            self.set_status(format!("Error: {e}"));
            return;
        }
        self.push_undo(UndoAction::StatusChange { before: target.clone() });
        let _ = self.reload();
        // Patch display snapshot in-place — keeps position/visibility stable
        self.patch_display_status(target.id, &new_status);
        self.patch_parent_display(target.parent_id);
    }

    /// `f`: make the issue today's frog; on today's frog, go without one today.
    fn toggle_frog(&mut self, issue: &Issue) {
        self.frog_day = Some(Local::now().date_naive());
        if self.frog_id == Some(issue.id) {
            self.frog_id = None;
            self.save_frog();
            self.set_status("No frog today. scrumtui picks one tomorrow.");
            return;
        }
        if issue.status == Status::Done {
            self.set_status("Done issues can't be the frog.");
            return;
        }
        self.frog_id = Some(issue.id);
        self.save_frog();
        self.set_status(format!("Frog for today: {}. Do this one first.", issue.title));
    }

    /// Copy an issue's title, description and subtask checklist to the clipboard.
    fn yank(&mut self, issue: &Issue) {
        let mut text = format!("{}\n", issue.title);
        if let Some(desc) = issue.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            text.push('\n');
            text.push_str(desc);
            text.push('\n');
        }
        let subtasks = self.subtasks_of(issue.id);
        if !subtasks.is_empty() {
            text.push('\n');
            for sub in subtasks {
                let mark = match sub.status {
                    Status::Done => "x",
                    Status::InProgress => "~",
                    Status::Todo => " ",
                };
                text.push_str(&format!("- [{mark}] {}\n", sub.title));
            }
        }
        match copy_to_clipboard(&text) {
            Ok(_) => self.set_status(format!("Yanked \"{}\" to clipboard.", issue.title)),
            Err(e) => self.set_status(format!("Clipboard error: {e}")),
        }
    }

    fn handle_kanban_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl-H / Ctrl-L: move the selected issue to the previous / next column
            match key.code {
                KeyCode::Char('h') => { self.kanban_advance_status(-1); return; }
                KeyCode::Char('l') => { self.kanban_advance_status(1); return; }
                _ => {}
            }
        }
        if let Some(nav) = nav_of(&key) {
            self.kanban_nav(nav);
            return;
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                // Switch between parent and subtask panels (only if subtasks exist)
                if !self.sprint_has_any_subtasks() {
                    return;
                }
                self.kanban_panel = 1 - self.kanban_panel;
                // Entering the subtask panel shows the selected card's subtasks, if it has any.
                if self.kanban_panel == 1 {
                    if let Some(parent) = self.kanban_selected_parent() {
                        let parents = self.sprint_parents_with_subtasks();
                        if let Some(pos) = parents.iter().position(|p| p.id == parent.id) {
                            self.kanban_sub_parent_idx = pos;
                            self.kanban_sub_row = 0;
                        }
                    }
                }
                self.kanban_clamp();
            }
            // In the subtask panel, h/l (and </>) cycle through parents.
            KeyCode::Char('<') | KeyCode::Char(',') => {
                if self.kanban_panel == 1 { self.kanban_cycle_parent(-1); }
            }
            KeyCode::Char('>') | KeyCode::Char('.') => {
                if self.kanban_panel == 1 { self.kanban_cycle_parent(1); }
            }
            KeyCode::Char('h') if self.kanban_panel == 1 => self.kanban_cycle_parent(-1),
            KeyCode::Char('l') if self.kanban_panel == 1 => self.kanban_cycle_parent(1),
            // h / l: navigate between columns
            KeyCode::Char('l') => {
                if self.kanban_col < 2 {
                    self.kanban_col += 1;
                }
            }
            KeyCode::Char('h') => {
                if self.kanban_col > 0 {
                    self.kanban_col -= 1;
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.kanban_selected_issue() {
                    match issue.parent_id.and_then(|pid| self.issue_by_id(pid).cloned()) {
                        Some(parent) => self.open_edit_form(&parent, Some(issue.id)),
                        None => self.open_edit_form(&issue, None),
                    }
                }
            }
            KeyCode::Char('n') => {
                // Subtask panel: add a subtask to the focused parent.
                if self.kanban_panel == 1 {
                    if let Some(parent) = self.kanban_sub_parent() {
                        self.open_edit_form(&parent, None);
                        if let Some(Popup::EditIssue(form)) = &mut self.popup {
                            form.add_subtask();
                        }
                        return;
                    }
                }
                // Board: a new issue in the active sprint, in the focused column.
                let sprint_id = self.active_sprint.as_ref().map(|s| s.id);
                self.open_new_form(sprint_id, Status::from_index(self.kanban_col));
            }
            KeyCode::Char('y') => {
                if let Some(issue) = self.kanban_selected_issue() {
                    let target = issue.parent_id.and_then(|pid| self.issue_by_id(pid).cloned()).unwrap_or(issue);
                    self.yank(&target);
                }
            }
            KeyCode::Char('f') => {
                if let Some(issue) = self.kanban_selected_issue() {
                    let target = issue.parent_id.and_then(|pid| self.issue_by_id(pid).cloned()).unwrap_or(issue);
                    self.toggle_frog(&target);
                }
            }
            _ => {}
        }
    }

    fn kanban_advance_status(&mut self, dir: i32) {
        let Some(issue) = self.kanban_selected_issue() else { return };
        // Parent issues with subtasks: block direct status change
        if !issue.is_subtask() && self.has_subtasks(issue.id) {
            self.set_status("Status is managed by subtasks.");
            return;
        }
        let new_status = if dir > 0 { issue.status.next() } else { issue.status.prev() };
        if new_status == issue.status {
            return;
        }
        if let Err(e) = self.db.update_issue_status(issue.id, &new_status) {
            self.set_status(format!("Error: {e}"));
            return;
        }
        self.push_undo(UndoAction::StatusChange { before: issue.clone() });
        let _ = self.reload();
        // Flush display immediately so the issue moves to its new column right now,
        // and follow it there.
        self.flush_display();
        self.focus_issue(issue.id);
    }

    fn handle_gantt_key(&mut self, key: KeyEvent) {
        if let Some(nav) = nav_of(&key) {
            self.gantt_sel = nav_index(self.gantt_sel, self.epics().len(), nav);
            return;
        }
        if matches!(key.code, KeyCode::Char('e') | KeyCode::Enter) {
            if let Some(epic) = self.epics().get(self.gantt_sel).cloned() {
                self.open_epic_detail(epic, 0);
            }
        }
    }

    // ── Sprint history ─────────────────────────────────────────────────────────

    /// Load all sprints and their membership logs.
    pub fn load_sprint_history(&mut self) {
        // Fix any past sprints that are shorter than a week before displaying.
        let _ = self.db.fix_short_sprints();
        self.history_sprints = self.db.get_all_sprints().unwrap_or_default();
        self.history_entries = self.db.get_sprint_entries().unwrap_or_default();
        self.history_sel = self.history_sel.min(self.history_sprints.len().saturating_sub(1));
        let rows = self.history_rows().len();
        self.history_issue_sel = self.history_issue_sel.min(rows.saturating_sub(1));
        if rows == 0 {
            self.history_focus = 0;
        }
    }

    fn handle_history_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => {
                self.open_new_sprint_form();
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if !self.history_rows().is_empty() {
                    self.history_focus = 1 - self.history_focus;
                }
                return;
            }
            KeyCode::Esc => {
                self.history_focus = 0;
                return;
            }
            _ => {}
        }
        let len = self.history_sprints.len();
        if len == 0 {
            return;
        }
        if let Some(nav) = nav_of(&key) {
            if self.history_focus == 0 {
                self.history_sel = nav_index(self.history_sel, len, nav);
                self.history_issue_sel = 0;
                self.history_issue_offset = 0;
            } else {
                let rows = self.history_rows().len();
                self.history_issue_sel = nav_index(self.history_issue_sel, rows, nav);
            }
            return;
        }
        match key.code {
            KeyCode::Char('e') | KeyCode::Enter => {
                if self.history_focus == 1 {
                    let issue = self.history_rows().get(self.history_issue_sel).map(|e| e.issue.clone());
                    if let Some(issue) = issue {
                        self.open_edit_form(&issue, None);
                    }
                } else if let Some(sprint) = self.history_sprints.get(self.history_sel).cloned() {
                    let form = self.sprint_edit_form(&sprint);
                    self.popup = Some(Popup::SprintManager(form));
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') if self.history_focus == 0 => {
                if let Some(sprint) = self.history_sprints.get(self.history_sel) {
                    self.popup = Some(Popup::ConfirmDeleteSprint(sprint.id, sprint.name.clone()));
                }
            }
            _ => {}
        }
    }

    fn handle_popup_key(&mut self, key: KeyEvent) -> bool {
        match &mut self.popup {
            None => {}

            Some(Popup::Help) => match arrows_to_vim(key).code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => self.popup = None,
                // The renderer caps the offset at what the screen needs.
                KeyCode::Char('j') => self.help_scroll += 1,
                KeyCode::Char('k') => self.help_scroll = self.help_scroll.saturating_sub(1),
                _ => {}
            },

            Some(Popup::ConfirmDelete(id, _)) => {
                let id = *id;
                match key.code {
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        self.popup = None;
                        let next = self.backlog_neighbor(id);
                        if let Err(e) = self.db.delete_issue(id) {
                            self.set_status(format!("Error: {e}"));
                        } else {
                            self.push_undo(UndoAction::SoftDelete { issue_id: id });
                            let _ = self.reload();
                            self.flush_display();
                            if let Some(n) = next {
                                self.focus_issue(n);
                            }
                            self.set_status("Issue moved to trash. u to undo.");
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.popup = None;
                    }
                    _ => {}
                }
            }

            Some(Popup::ConfirmDeleteSprint(id, _)) => {
                let id = *id;
                match key.code {
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        self.popup = None;
                        if let Err(e) = self.db.delete_sprint(id) {
                            self.set_status(format!("Error: {e}"));
                        } else {
                            let _ = self.reload();
                            self.flush_display();
                            self.load_sprint_history();
                            self.set_status("Sprint deleted.");
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.popup = None;
                    }
                    _ => {}
                }
            }

            Some(Popup::Trash { items, sel }) => {
                let key = arrows_to_vim(key);
                if let Some(nav) = nav_of(&key) {
                    *sel = nav_index(*sel, items.len(), nav);
                    return false;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.popup = None;
                    }
                    KeyCode::Char('r') => {
                        if let Some(issue) = items.get(*sel).cloned() {
                            self.popup = None;
                            if let Err(e) = self.db.restore_issue(issue.id) {
                                self.set_status(format!("Error: {e}"));
                            } else {
                                let _ = self.reload();
                                self.flush_display();
                                self.focus_issue(issue.id);
                                self.set_status(format!("\"{}\" restored.", issue.title));
                            }
                        }
                    }
                    KeyCode::Char('D') => {
                        if let Some(issue) = items.get(*sel).cloned() {
                            let new_sel = *sel;
                            self.popup = None;
                            if let Err(e) = self.db.purge_issue(issue.id) {
                                self.set_status(format!("Error: {e}"));
                            } else {
                                // Reopen trash with updated list
                                match self.db.get_trash() {
                                    Ok(new_items) => {
                                        let clamped = new_sel.min(new_items.len().saturating_sub(1));
                                        self.popup = Some(Popup::Trash { items: new_items, sel: clamped });
                                    }
                                    Err(e) => self.set_status(format!("Error: {e}")),
                                }
                                self.set_status(format!("\"{}\" permanently deleted.", issue.title));
                            }
                        }
                    }
                    _ => {}
                }
            }

            Some(Popup::NewIssue(_)) | Some(Popup::EditIssue(_)) => {
                self.handle_issue_form_key(key);
            }

            Some(Popup::SprintManager(_)) => {
                self.handle_sprint_form_key(key);
            }

            Some(Popup::GanttEpicDetail { .. }) => {
                self.handle_epic_detail_key(key);
            }
        }
        false
    }

    fn handle_epic_detail_key(&mut self, key: KeyEvent) {
        let Some(Popup::GanttEpicDetail { epic, issues, search, search_active, sel, scroll }) = &mut self.popup else {
            return;
        };
        let count = epic_detail_filter(issues, search).len();
        if *search_active {
            match key.code {
                KeyCode::Esc => {
                    *search_active = false;
                    search.clear();
                    *sel = 0;
                    *scroll = 0;
                }
                KeyCode::Enter => *search_active = false,
                KeyCode::Down | KeyCode::Up => {
                    let nav = if key.code == KeyCode::Down { Nav::Down } else { Nav::Up };
                    *sel = nav_index(*sel, count, nav);
                }
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(search);
                    } else {
                        search.pop();
                    }
                    *sel = 0;
                    *scroll = 0;
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    delete_word(search);
                    *sel = 0;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    search.push(c);
                    *sel = 0;
                    *scroll = 0;
                }
                _ => {}
            }
            return;
        }
        if let Some(nav) = nav_of(&key) {
            *sel = nav_index(*sel, count, nav);
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = None,
            KeyCode::Char('/') => *search_active = true,
            KeyCode::Char('e') | KeyCode::Enter => {
                let selected = epic_detail_filter(issues, search).get(*sel).map(|i| (*i).clone());
                let back = Popup::GanttEpicDetail {
                    epic: epic.clone(),
                    issues: Vec::new(),
                    search: String::new(),
                    search_active: false,
                    sel: *sel,
                    scroll: 0,
                };
                if let Some(issue) = selected {
                    // Back to this epic once the form closes.
                    self.return_popup = Some(back);
                    self.open_edit_form(&issue, None);
                }
            }
            _ => {}
        }
    }

    fn issue_form(&mut self) -> Option<&mut IssueForm> {
        match &mut self.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => Some(f),
            _ => None,
        }
    }

    fn handle_issue_form_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(form) = self.issue_form() else { return };
        // Any key but Esc cancels a pending "discard changes?".
        if key.code != KeyCode::Esc && form.confirm_discard {
            form.confirm_discard = false;
            form.error = None;
        }

        // ── Subtask-list navigation (when focus is in the subtask section) ──────
        if form.in_subtask_list {
            self.handle_subtask_list_key(key);
            return;
        }

        if ctrl && key.code == KeyCode::Char('s') {
            self.submit_issue_form();
            return;
        }
        if ctrl && key.code == KeyCode::Char('n') {
            form.add_subtask();
            return;
        }

        let dropdown_open = form.epic_dropdown_open || form.due_date_dropdown_open;
        match key.code {
            KeyCode::Esc => {
                if dropdown_open {
                    form.epic_dropdown_open = false;
                    form.due_date_dropdown_open = false;
                } else if form.is_dirty() && !form.confirm_discard {
                    form.confirm_discard = true;
                    form.error = Some("Unsaved changes. Esc again to discard, Ctrl-S to save.".into());
                } else {
                    self.close_issue_form();
                }
            }
            KeyCode::Enter => {
                // With a dropdown open, Enter picks the highlighted entry; otherwise it saves.
                if dropdown_open {
                    self.commit_dropdown(true);
                } else {
                    self.submit_issue_form();
                }
            }
            KeyCode::Tab => {
                self.commit_dropdown(false);
                self.form_move_field(1, true);
            }
            KeyCode::BackTab => self.form_move_field(-1, true),
            KeyCode::Down if !dropdown_open => self.form_move_field(1, false),
            KeyCode::Up if !dropdown_open => self.form_move_field(-1, false),
            _ => self.form_input(key),
        }
    }

    /// Move to the next (+1) or previous (-1) field. Past the description is the
    /// subtask list; with `wrap`, going back from the title wraps to it as well.
    fn form_move_field(&mut self, dir: i32, wrap: bool) {
        let Some(form) = self.issue_form() else { return };
        form.epic_dropdown_open = false;
        form.due_date_dropdown_open = false;
        let to_subtasks = if dir > 0 {
            form.focused_field + 1 >= IssueForm::field_count()
        } else {
            form.focused_field == 0
        };
        if to_subtasks {
            if dir > 0 || wrap {
                form.in_subtask_list = true;
                form.subtask_sel = 0;
                form.subtask_editing = false;
            }
            return;
        }
        if dir > 0 {
            form.focused_field += 1;
        } else {
            form.focused_field -= 1;
        }
        // The due-date dropdown opens as soon as the field is focused.
        if form.focused_field == F_DUE {
            form.due_date_dropdown_open = true;
            form.due_date_dropdown_sel = 0;
        }
    }

    /// Apply the highlighted autocomplete entry and close the dropdowns. Tab
    /// (`explicit = false`) only applies it when something was typed, so tabbing past
    /// an empty due date leaves it empty.
    fn commit_dropdown(&mut self, explicit: bool) {
        let Some(form) = self.issue_form() else { return };
        let (epic, due) = (form.epic.clone(), form.due_date.clone());
        let (epic_open, due_open) = (form.epic_dropdown_open, form.due_date_dropdown_open);
        let (epic_sel, due_sel) = (form.epic_dropdown_sel, form.due_date_dropdown_sel);
        let epic_pick = (epic_open && (explicit || !epic.is_empty()))
            .then(|| self.epic_matches(&epic).into_iter().nth(epic_sel))
            .flatten();
        let due_pick = (due_open && (explicit || !due.is_empty()))
            .then(|| self.due_matches(&due).into_iter().nth(due_sel).map(|(d, _)| d))
            .flatten();
        let Some(form) = self.issue_form() else { return };
        if let Some(e) = epic_pick {
            form.epic = e;
        }
        if let Some(d) = due_pick {
            form.due_date = d;
        }
        form.epic_dropdown_open = false;
        form.due_date_dropdown_open = false;
        form.epic_dropdown_sel = 0;
        form.due_date_dropdown_sel = 0;
    }

    /// Typing into the focused field (and dropdown navigation).
    fn form_input(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let word = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
        let (epic, due) = match self.issue_form() {
            Some(f) => (f.epic.clone(), f.due_date.clone()),
            None => return,
        };
        let epic_rows = self.epic_matches(&epic).len().min(DROPDOWN_ROWS);
        let due_rows = self.due_matches(&due).len().min(DROPDOWN_ROWS);
        let Some(form) = self.issue_form() else { return };
        form.error = None;

        if form.focused_field == F_STATUS {
            // Status field: cycle with h/l (or ←/→)
            match arrows_to_vim(key).code {
                KeyCode::Char('h') => form.status_idx = form.status_idx.saturating_sub(1),
                KeyCode::Char('l') => form.status_idx = (form.status_idx + 1).min(2),
                _ => {}
            }
            return;
        }

        // Dropdown navigation: ↑/↓ or Ctrl-k/Ctrl-j (plain j/k are typed).
        let dropdown = match form.focused_field {
            F_EPIC if form.epic_dropdown_open => Some((&mut form.epic_dropdown_sel, epic_rows)),
            F_DUE if form.due_date_dropdown_open => Some((&mut form.due_date_dropdown_sel, due_rows)),
            _ => None,
        };
        if let Some((sel, rows)) = dropdown {
            let nav = match key.code {
                KeyCode::Down => Some(Nav::Down),
                KeyCode::Up => Some(Nav::Up),
                KeyCode::Char('j') if ctrl => Some(Nav::Down),
                KeyCode::Char('k') if ctrl => Some(Nav::Up),
                _ => None,
            };
            if let Some(nav) = nav {
                *sel = nav_index(*sel, rows, nav);
                return;
            }
        }

        if form.focused_field == F_DUE && key.code == KeyCode::Delete {
            form.due_date.clear();
            form.due_date_dropdown_sel = 0;
            return;
        }

        let field_idx = form.focused_field;
        let Some(field) = form.active_text_field() else { return };
        match key.code {
            KeyCode::Backspace if word => delete_word(field),
            KeyCode::Backspace => { field.pop(); }
            KeyCode::Char('w') if ctrl => delete_word(field),
            // Story points only take numbers.
            KeyCode::Char(c) if field_idx == F_SP && !(c.is_ascii_digit() || c == '.') => return,
            KeyCode::Char(c) if !ctrl => field.push(c),
            _ => return,
        }
        // Typing (re)opens the dropdown of the field and restarts its selection.
        match field_idx {
            F_EPIC => {
                form.epic_dropdown_open = !form.epic.is_empty();
                form.epic_dropdown_sel = 0;
            }
            F_DUE => {
                form.due_date_dropdown_open = true;
                form.due_date_dropdown_sel = 0;
            }
            _ => {}
        }
    }

    /// Save the issue form (create or update), including its subtasks.
    fn submit_issue_form(&mut self) {
        let Some(form) = self.issue_form() else { return };
        // Close any open dropdown without applying it.
        form.epic_dropdown_open = false;
        form.due_date_dropdown_open = false;
        if let Some(err) = form.validate() {
            form.error = Some(err);
            return;
        }
        let form = form.clone();
        let editing_id = form.editing_id;
        let title = form.title.trim().to_string();
        let sp = form.story_points.parse::<f64>().unwrap_or(1.0);
        let epic = form.epic.trim().to_string();
        let status = Status::from_index(form.status_idx);
        let due_date = NaiveDate::parse_from_str(&form.due_date, "%Y-%m-%d").ok();
        let desc = Some(form.description.clone()).filter(|d| !d.trim().is_empty());

        let frog_before = self.frog_id;
        // Snapshot for undo: the issue and its subtasks before this save.
        let before = editing_id.and_then(|id| self.db.get_issue(id).ok());
        let subtasks_before: Vec<Issue> = editing_id
            .map(|id| self.issues.iter().filter(|i| i.parent_id == Some(id)).cloned().collect())
            .unwrap_or_default();

        self.popup = None;

        // Returns the issue id (new or existing) so we can attach subtasks
        let id_result = match editing_id {
            Some(id) => self
                .db
                .update_issue(id, &title, sp, &epic, &status, due_date, desc.as_deref(), None)
                .map(|_| id),
            None => self.db.create_issue(&title, sp, &epic, &status, due_date, desc.as_deref()),
        };
        let parent_id = match id_result {
            Ok(id) => id,
            Err(e) => {
                self.set_status(format!("Error: {e}"));
                self.close_issue_form();
                return;
            }
        };
        // A new issue created from the sprint section or the kanban board joins the sprint.
        if editing_id.is_none() {
            if let Some(sid) = form.sprint_id {
                let _ = self.db.set_issue_sprint(parent_id, Some(sid));
            }
        }

        // Persist subtask drafts (works for both new and edit)
        let mut new_subtask_ids: Vec<i64> = Vec::new();
        for draft in &form.subtasks {
            let st = Status::from_index(draft.status_idx);
            match draft.id {
                Some(id) if draft.deleted => { let _ = self.db.delete_issue(id); }
                Some(id) => { let _ = self.db.update_subtask(id, draft.title.trim(), &st); }
                None if !draft.deleted && !draft.title.trim().is_empty() => {
                    if let Ok(new_id) = self.db.create_subtask(draft.title.trim(), parent_id, &st) {
                        new_subtask_ids.push(new_id);
                    }
                }
                None => {}
            }
        }
        // Recompute parent status whenever subtasks exist
        if !form.subtasks.is_empty() {
            let _ = self.db.update_parent_status_from_children(parent_id);
        }

        match (editing_id, before) {
            (None, _) => self.push_undo(UndoAction::Created { issue_id: parent_id }),
            (Some(_), Some(before)) => {
                self.push_undo(UndoAction::IssueEdit { before, subtasks_before, new_subtask_ids });
            }
            (Some(_), None) => {} // couldn't snapshot: better no undo than a wrong one
        }

        // Full edit: refresh display snapshot so the saved state
        // is immediately reflected (user consciously committed it)
        self.refresh();
        self.focus_issue(parent_id);
        // Finishing the frog already put up its own message; don't bury it.
        let frog_done = frog_before
            .and_then(|id| self.issue_by_id(id))
            .is_some_and(|i| i.status == Status::Done);
        if frog_done {
            // keep "Frog done: …"
        } else if editing_id.is_some() {
            self.set_status("Issue updated.");
        } else {
            let place = match (&form.sprint_id, &self.active_sprint) {
                (Some(_), Some(s)) => format!(" in {}", s.name),
                _ => " in the backlog".to_string(),
            };
            self.set_status(format!("Created #{parent_id}{place}."));
        }
        self.close_issue_form();
    }

    fn handle_subtask_list_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('s') {
            self.submit_issue_form();
            return;
        }
        let Some(form) = self.issue_form() else { return };

        if form.subtask_editing {
            let Some(idx) = form.selected_subtask_index() else {
                form.subtask_editing = false;
                return;
            };
            let finish = |form: &mut IssueForm| {
                form.subtask_editing = false;
                // A new subtask left without a title disappears.
                if form.subtasks[idx].id.is_none() && form.subtasks[idx].title.trim().is_empty() {
                    form.subtasks.remove(idx);
                    form.subtask_sel = form.subtask_sel.min(form.visible_subtask_count().saturating_sub(1));
                    if form.visible_subtask_count() == 0 {
                        form.in_subtask_list = false;
                        form.focused_field = F_DESC;
                    }
                }
            };
            match key.code {
                KeyCode::Esc | KeyCode::Enter => finish(&mut *form),
                KeyCode::Tab => {
                    let next = form.subtask_sel + 1;
                    let more = next < form.visible_subtask_count();
                    finish(&mut *form);
                    if more {
                        form.subtask_sel = next.min(form.visible_subtask_count().saturating_sub(1));
                        form.subtask_editing = true;
                    } else {
                        // Wrap back to top fields
                        form.in_subtask_list = false;
                        form.focused_field = F_TITLE;
                    }
                }
                KeyCode::Backspace => {
                    let title = &mut form.subtasks[idx].title;
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(title);
                    } else {
                        title.pop();
                    }
                }
                KeyCode::Char('w') if ctrl => delete_word(&mut form.subtasks[idx].title),
                KeyCode::Char(c) if !ctrl => form.subtasks[idx].title.push(c),
                _ => {}
            }
            return;
        }

        // Browse mode
        let key = arrows_to_vim(key);
        if let Some(nav) = nav_of(&key) {
            form.subtask_sel = nav_index(form.subtask_sel, form.visible_subtask_count(), nav);
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::BackTab => {
                form.in_subtask_list = false;
                form.focused_field = F_DESC;
            }
            KeyCode::Tab => {
                // Back to top fields
                form.in_subtask_list = false;
                form.focused_field = F_TITLE;
            }
            KeyCode::Enter => self.submit_issue_form(),
            KeyCode::Char('n') => form.add_subtask(),
            KeyCode::Char('e') | KeyCode::Char('i') => {
                if form.visible_subtask_count() > 0 {
                    form.subtask_editing = true;
                }
            }
            KeyCode::Char('l') | KeyCode::Char('h') => {
                if let Some(idx) = form.selected_subtask_index() {
                    let st = &mut form.subtasks[idx];
                    st.status_idx = if key.code == KeyCode::Char('l') {
                        (st.status_idx + 1).min(2)
                    } else {
                        st.status_idx.saturating_sub(1)
                    };
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                // Mark selected subtask for deletion
                if let Some(idx) = form.selected_subtask_index() {
                    form.subtasks[idx].deleted = true;
                    form.subtask_sel = form.subtask_sel.min(form.visible_subtask_count().saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    fn handle_sprint_form_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('n') {
            self.toggle_sprint_form_mode();
            return;
        }
        if key.code == KeyCode::Enter || (ctrl && key.code == KeyCode::Char('s')) {
            self.submit_sprint_form();
            return;
        }
        let Some(Popup::SprintManager(form)) = &mut self.popup else { return };
        let count = SprintForm::field_count();
        match key.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Tab | KeyCode::Down => form.focused_field = (form.focused_field + 1) % count,
            KeyCode::BackTab | KeyCode::Up => {
                form.focused_field = (form.focused_field + count - 1) % count;
            }
            _ => {
                form.error = None;
                if form.focused_field == 3 {
                    // Boolean toggle
                    if matches!(
                        arrows_to_vim(key).code,
                        KeyCode::Char(' ') | KeyCode::Char('h') | KeyCode::Char('l')
                    ) {
                        form.is_active = !form.is_active;
                    }
                } else if let Some(field) = form.active_text_field() {
                    match key.code {
                        KeyCode::Backspace if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) => {
                            delete_word(field);
                        }
                        KeyCode::Backspace => { field.pop(); }
                        KeyCode::Char('w') if ctrl => delete_word(field),
                        KeyCode::Char(c) if !ctrl => field.push(c),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Ctrl-N in the sprint manager: switch between starting a new sprint and
    /// editing the current one.
    fn toggle_sprint_form_mode(&mut self) {
        let Some(Popup::SprintManager(form)) = &self.popup else { return };
        let editing = form.editing_id.is_some();
        let active = self.active_sprint.clone();
        let new_form = match (editing, active) {
            (true, Some(a)) => SprintForm::next_after(&a),
            (true, None) => SprintForm::new(),
            (false, Some(a)) => self.sprint_edit_form(&a),
            (false, None) => return,
        };
        self.popup = Some(Popup::SprintManager(new_form));
    }

    fn submit_sprint_form(&mut self) {
        let Some(Popup::SprintManager(form)) = &mut self.popup else { return };
        if let Some(err) = form.validate() {
            form.error = Some(err);
            return;
        }
        let editing_id = form.editing_id;
        let name = form.name.trim().to_string();
        let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").unwrap_or_default();
        let end = NaiveDate::parse_from_str(&form.end_date, "%Y-%m-%d").unwrap_or_default();
        let active = form.is_active;
        self.popup = None;

        let old_active = self.active_sprint.clone();
        let result = match editing_id {
            Some(id) => self.db.update_sprint(id, &name, start, end, active).map(|_| id),
            None => self.db.create_sprint(&name, start, end, active),
        };
        let sprint_id = match result {
            Ok(id) => id,
            Err(e) => {
                self.set_status(format!("Error: {e}"));
                return;
            }
        };

        let mut msg = if editing_id.is_some() {
            "Sprint saved.".to_string()
        } else {
            format!("Created {name}.")
        };
        // When a later sprint takes over, unfinished work comes along: it gets a
        // carry-over mark (↩) and moves into the new sprint.
        if let Some(old) = old_active.filter(|o| active && o.id != sprint_id && start > o.start_date) {
            let _ = self.db.bump_carry_count_for_sprint(old.id);
            let moved = self.db.move_incomplete_issues_to_sprint(old.id, sprint_id).unwrap_or(0);
            msg = match moved {
                0 => format!("Started {name}. Nothing left over from {}.", old.name),
                1 => format!("Started {name}. 1 unfinished issue carried over from {}.", old.name),
                n => format!("Started {name}. {n} unfinished issues carried over from {}.", old.name),
            };
        }

        let _ = self.reload();
        self.flush_display();
        if self.view == View::SprintHistory {
            self.load_sprint_history();
        }
        self.set_status(msg);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Pipe `text` into the first available clipboard tool (macOS, Wayland, then X11).
/// On Linux without any of them, a background `scrumtui __clipboard` holds it.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    const TOOLS: &[(&str, &[&str])] = &[
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    let mut last_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no clipboard tool found");
    for (cmd, args) in TOOLS {
        let spawned = Command::new(cmd)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes())?;
                }
                let status = child.wait()?;
                if status.success() { return Ok(()); }
                last_err = std::io::Error::other(format!("{cmd} exited with {status}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => last_err = e,
        }
    }
    #[cfg(target_os = "linux")]
    if last_err.kind() == std::io::ErrorKind::NotFound {
        return hold_clipboard(text);
    }
    Err(last_err)
}

/// Copy `text` without a clipboard tool: start a detached `scrumtui __clipboard`
/// that owns the clipboard and answers pastes until something else is copied,
/// the way xclip does. It replies "ok" once it has the clipboard, or the error.
#[cfg(target_os = "linux")]
fn hold_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe()?)
        .arg("__clipboard")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0) // own group: Ctrl-C in the terminal doesn't reach it
        .spawn()?;
    // If the helper died early, the write fails and the reply says why.
    let _ = child.stdin.take().unwrap().write_all(text.as_bytes());
    let mut reply = String::new();
    let _ = BufReader::new(child.stdout.take().unwrap()).read_line(&mut reply);
    // Reap the helper when it exits, so it doesn't linger as a zombie.
    std::thread::spawn(move || child.wait());
    match reply.trim() {
        "ok" => Ok(()),
        "" => Err(std::io::Error::other("clipboard helper exited")),
        err => Err(std::io::Error::other(err.to_string())),
    }
}

/// `scrumtui __clipboard`: put stdin on the clipboard and keep serving it until
/// another program copies something. See `hold_clipboard`.
#[cfg(target_os = "linux")]
pub fn serve_clipboard() -> Result<()> {
    use arboard::SetExtLinux;
    use std::io::{Read, Write};
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            println!("{e}");
            return Err(e.into());
        }
    };
    println!("ok");
    std::io::stdout().flush()?;
    clipboard.set().wait().text(text)?;
    Ok(())
}

/// Translate arrow keys into their vim equivalents (←↓↑→ → h/j/k/l), keeping
/// modifiers, so Ctrl-arrows behave like Ctrl-h/j/k/l. Only use this in
/// navigation contexts — never on keys destined for a text field.
fn arrows_to_vim(mut key: KeyEvent) -> KeyEvent {
    key.code = match key.code {
        KeyCode::Left => KeyCode::Char('h'),
        KeyCode::Down => KeyCode::Char('j'),
        KeyCode::Up => KeyCode::Char('k'),
        KeyCode::Right => KeyCode::Char('l'),
        other => other,
    };
    key
}

/// Delete from end of `s` back to (and including) the last word boundary.
/// Mimics the typical Alt+Backspace / Option+Delete behaviour.
pub fn delete_word(s: &mut String) {
    // Trim trailing spaces first, then remove the word
    let trimmed_len = s.trim_end().len();
    s.truncate(trimmed_len);
    // Find last space (word boundary)
    if let Some(pos) = s.rfind(' ') {
        s.truncate(pos + 1); // keep the space before the word
    } else {
        s.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn app_with_sprint() -> (App, i64) {
        let db = Db::open(":memory:").unwrap();
        let start = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let sid = db.create_sprint("S1", start, start + Duration::days(13), true).unwrap();
        (App::new(db).unwrap(), sid)
    }

    /// An app whose active sprint runs from two days ago to two days from now.
    fn app_with_current_sprint() -> (App, i64) {
        let db = Db::open(":memory:").unwrap();
        let today = Local::now().date_naive();
        let sid = db.create_sprint("now", today - Duration::days(2), today + Duration::days(2), true).unwrap();
        (App::new(db).unwrap(), sid)
    }

    fn add(app: &App, title: &str, created: &str, status: Status, sprint: Option<i64>) -> i64 {
        let id = app.db.create_issue_full(
            title, 1.0, "", &status, None, None, None, created, created, None,
        ).unwrap();
        app.db.set_issue_sprint(id, sprint).unwrap();
        id
    }

    fn refreshed(app: &mut App) {
        app.reload().unwrap();
        app.flush_display();
    }

    fn sprint_titles(app: &App) -> Vec<String> {
        app.backlog_items().into_iter().filter_map(|bi| match bi {
            BacklogItem::Issue(i, true) => Some(i.title),
            _ => None,
        }).collect()
    }

    fn selected_title(app: &App) -> Option<String> {
        app.selected_issue().map(|i| i.title)
    }

    fn form(app: &App) -> &IssueForm {
        match &app.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
            _ => panic!("no issue form open"),
        }
    }

    #[test]
    fn arrows_map_to_vim_keeping_modifiers() {
        assert_eq!(arrows_to_vim(key(KeyCode::Left)).code, KeyCode::Char('h'));
        assert_eq!(arrows_to_vim(key(KeyCode::Down)).code, KeyCode::Char('j'));
        let k = arrows_to_vim(ctrl(KeyCode::Up));
        assert_eq!(k.code, KeyCode::Char('k'));
        assert!(k.modifiers.contains(KeyModifiers::CONTROL));
        assert_eq!(arrows_to_vim(key(KeyCode::Char('x'))).code, KeyCode::Char('x'));
    }

    #[test]
    fn unfinished_sprint_issues_sort_oldest_first() {
        let (mut app, sid) = app_with_sprint();
        add(&app, "done-old", "2025-01-01 00:00:00", Status::Done, Some(sid));
        add(&app, "carried-new", "2026-03-01 00:00:00", Status::Todo, Some(sid));
        add(&app, "carried-old", "2026-01-01 00:00:00", Status::InProgress, Some(sid));
        app.db.bump_carry_count_for_sprint(sid).unwrap();
        add(&app, "fresh-older", "2025-06-01 00:00:00", Status::Todo, Some(sid));
        add(&app, "fresh-newer", "2025-07-01 00:00:00", Status::Todo, Some(sid));
        refreshed(&mut app);
        assert_eq!(
            sprint_titles(&app),
            ["carried-old", "carried-new", "fresh-older", "fresh-newer", "done-old"]
        );
        // Kanban TODO column follows the same order.
        let todo: Vec<String> = app.sprint_parents_by_status(&Status::Todo)
            .into_iter().map(|i| i.title).collect();
        assert_eq!(todo, ["carried-new", "fresh-older", "fresh-newer"]);
    }

    #[test]
    fn status_change_keeps_position_until_view_switch() {
        let (mut app, sid) = app_with_sprint();
        add(&app, "a", "2025-01-01 00:00:00", Status::Todo, Some(sid));
        add(&app, "b", "2025-02-01 00:00:00", Status::Todo, Some(sid));
        refreshed(&mut app);
        app.backlog_sel_to_first_issue();
        app.handle_key(key(KeyCode::Right));
        app.handle_key(key(KeyCode::Right));
        assert_eq!(sprint_titles(&app), ["a", "b"]);
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Char('1')));
        assert_eq!(sprint_titles(&app), ["b", "a"]);
    }

    #[test]
    fn arrows_change_status_in_backlog() {
        let (mut app, _) = app_with_sprint();
        let id = add(&app, "a", "2025-01-01 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        app.backlog_sel_to_first_issue();
        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::InProgress);
        assert!(app.issue_by_id(id).unwrap().started_at.is_some());
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::Todo);
        assert!(app.issue_by_id(id).unwrap().started_at.is_none());
        // Undo puts the exact earlier row back.
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::InProgress);
    }

    #[test]
    fn ctrl_arrows_move_issue_between_kanban_columns() {
        let (mut app, sid) = app_with_sprint();
        let id = add(&app, "a", "2025-01-01 00:00:00", Status::Todo, Some(sid));
        refreshed(&mut app);
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(ctrl(KeyCode::Right));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::InProgress);
        assert_eq!(app.kanban_col, 1);
        app.handle_key(ctrl(KeyCode::Right));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::Done);
        app.handle_key(ctrl(KeyCode::Left));
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::InProgress);
        // Plain arrows navigate columns without changing status.
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.kanban_col, 0);
        assert_eq!(app.issue_by_id(id).unwrap().status, Status::InProgress);
    }

    #[test]
    fn ctrl_arrows_reorder_backlog() {
        let (mut app, _) = app_with_sprint();
        add(&app, "first", "2025-01-01 00:00:00", Status::Todo, None);
        add(&app, "second", "2025-01-02 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        let titles = |app: &App| -> Vec<String> {
            app.backlog_items().into_iter().filter_map(|bi| match bi {
                BacklogItem::Issue(i, false) => Some(i.title),
                _ => None,
            }).collect()
        };
        // Newest first by default.
        assert_eq!(titles(&app), ["second", "first"]);
        app.backlog_sel_to_first_issue();
        app.handle_key(ctrl(KeyCode::Down));
        assert_eq!(titles(&app), ["first", "second"]);
        app.handle_key(ctrl(KeyCode::Up));
        assert_eq!(titles(&app), ["second", "first"]);
    }

    #[test]
    fn n_and_e_open_new_and_edit() {
        let (mut app, _) = app_with_sprint();
        add(&app, "a", "2025-01-01 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        app.backlog_sel_to_first_issue();
        app.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(app.popup, Some(Popup::NewIssue(_))));
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(key(KeyCode::Char('e')));
        assert!(matches!(app.popup, Some(Popup::EditIssue(_))));
    }

    #[test]
    fn epic_field_accepts_j_and_k_while_dropdown_open() {
        let (mut app, _) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('n')));
        app.handle_key(key(KeyCode::Tab)); // → epic
        type_text(&mut app, "jk8s");
        assert_eq!(form(&app).epic, "jk8s");
    }

    #[test]
    fn right_arrow_cycles_form_status() {
        let (mut app, _) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('n')));
        for _ in 0..3 { app.handle_key(key(KeyCode::Tab)); } // → status
        app.handle_key(key(KeyCode::Right));
        assert_eq!(form(&app).status_idx, 1);
    }

    #[test]
    fn subtask_edit_after_delete_targets_visible_row() {
        let (mut app, _) = app_with_sprint();
        let mut form = IssueForm::new();
        form.title = "parent".into();
        for t in ["one", "two"] {
            form.subtasks.push(SubtaskDraft { id: None, title: t.into(), status_idx: 0, deleted: false });
        }
        form.in_subtask_list = true;
        app.popup = Some(Popup::NewIssue(form));
        app.handle_key(key(KeyCode::Char('x'))); // delete "one"; "two" is now row 0
        app.handle_key(key(KeyCode::Char('e')));
        app.handle_key(key(KeyCode::Char('!')));
        let f = self::form(&app);
        assert_eq!(f.subtasks[0].title, "one");
        assert_eq!(f.subtasks[1].title, "two!");
    }

    #[test]
    fn top_and_bottom_keys_work_in_every_list() {
        let (mut app, sid) = app_with_sprint();
        for (i, t) in ["a", "b", "c"].iter().enumerate() {
            add(&app, t, &format!("2025-01-0{} 00:00:00", i + 1), Status::Todo, None);
            add(&app, &format!("s{t}"), &format!("2025-01-0{} 00:00:00", i + 1), Status::Todo, Some(sid));
        }
        let monday = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        app.db.create_sprint("old1", monday, monday + Duration::days(6), false).unwrap();
        app.db.create_sprint("old2", monday + Duration::days(7), monday + Duration::days(13), false).unwrap();
        refreshed(&mut app);

        // Backlog: g/G and Home/End.
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(selected_title(&app).as_deref(), Some("a"));
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(selected_title(&app).as_deref(), Some("sa"));
        app.handle_key(key(KeyCode::End));
        assert_eq!(selected_title(&app).as_deref(), Some("a"));
        app.handle_key(key(KeyCode::Home));
        assert_eq!(selected_title(&app).as_deref(), Some("sa"));
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(selected_title(&app).as_deref(), Some("a"));

        // Kanban column.
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.kanban_rows[0], 2);
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.kanban_rows[0], 0);

        // Gantt has a single (empty-named) epic list; history has three sprints.
        app.handle_key(key(KeyCode::Char('4')));
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.history_sel, 2);
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.history_sel, 0);

        // Trash.
        app.handle_key(key(KeyCode::Char('1')));
        for _ in 0..3 {
            app.handle_key(key(KeyCode::Char('G')));
            app.handle_key(key(KeyCode::Char('d')));
            app.handle_key(key(KeyCode::Char('d')));
        }
        app.handle_key(key(KeyCode::Char('T')));
        app.handle_key(key(KeyCode::Char('G')));
        assert!(matches!(app.popup, Some(Popup::Trash { sel: 2, .. })));
        app.handle_key(key(KeyCode::Char('g')));
        assert!(matches!(app.popup, Some(Popup::Trash { sel: 0, .. })));
    }

    #[test]
    fn n_on_kanban_creates_issue_in_sprint_and_column() {
        let (mut app, sid) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Char('l'))); // IN PROGRESS column
        app.handle_key(key(KeyCode::Char('n')));
        assert_eq!(form(&app).status_idx, 1);
        type_text(&mut app, "from the board");
        app.handle_key(key(KeyCode::Tab));
        type_text(&mut app, "epic");
        app.handle_key(ctrl(KeyCode::Char('s')));
        assert!(app.popup.is_none(), "form should have saved");
        let issue = app.issues.iter().find(|i| i.title == "from the board").unwrap().clone();
        assert_eq!(issue.sprint_id, Some(sid));
        assert_eq!(issue.status, Status::InProgress);
        assert!(issue.started_at.is_some());
        // The cursor lands on the new card.
        assert_eq!(app.kanban_selected_issue().map(|i| i.id), Some(issue.id));
    }

    #[test]
    fn n_in_kanban_subtask_panel_adds_a_subtask() {
        let (mut app, sid) = app_with_sprint();
        let parent = app.db.create_issue("parent", 1.0, "epic", &Status::Todo, None, None).unwrap();
        app.db.set_issue_sprint(parent, Some(sid)).unwrap();
        app.db.create_subtask("first", parent, &Status::Todo).unwrap();
        refreshed(&mut app);
        app.handle_key(key(KeyCode::Char('2')));
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('n')));
        type_text(&mut app, "second");
        app.handle_key(key(KeyCode::Enter)); // finish the title
        app.handle_key(key(KeyCode::Enter)); // save
        let subs: Vec<String> = app.subtasks_of(parent).iter().map(|s| s.title.clone()).collect();
        assert_eq!(subs, ["first", "second"]);
    }

    #[test]
    fn carried_issue_counts_as_done_only_where_it_was_finished() {
        let db = Db::open(":memory:").unwrap();
        let last_monday = monday_of(Local::now().date_naive()) - Duration::days(7);
        let old = db.create_sprint("old", last_monday, last_monday + Duration::days(6), true).unwrap();
        let mut app = App::new(db).unwrap();
        let carried = add(&app, "carried", "2025-01-01 00:00:00", Status::Todo, Some(old));
        let finished = add(&app, "finished", "2025-01-01 00:00:00", Status::Done, Some(old));
        refreshed(&mut app);

        // The old sprint is over, so S offers the next one; Enter starts it.
        app.handle_key(key(KeyCode::Char('S')));
        assert!(matches!(&app.popup, Some(Popup::SprintManager(f)) if f.editing_id.is_none()));
        app.handle_key(key(KeyCode::Enter));
        let new = app.active_sprint.clone().unwrap();
        assert_ne!(new.id, old);
        let this_week = (last_monday + Duration::days(7)).max(upcoming_monday(Local::now().date_naive()));
        assert_eq!(new.start_date, this_week);
        assert_eq!(new.end_date, this_week + Duration::days(4), "Monday to Friday");
        assert_eq!(app.issue_by_id(carried).unwrap().sprint_id, Some(new.id));
        assert_eq!(app.issue_by_id(carried).unwrap().carry_count, 1);

        app.db.update_issue_status(carried, &Status::Done).unwrap();
        let entries = app.db.get_sprint_entries().unwrap();
        let entry = |sprint: i64, id: i64| entries[&sprint].iter().find(|e| e.issue.id == id).unwrap();
        assert_eq!(entry(old, carried).outcome, Outcome::Carried);
        assert_eq!(entry(new.id, carried).outcome, Outcome::Done);
        assert_eq!(entry(old, finished).outcome, Outcome::Done);
        // Carried-in work is planned scope of the new sprint, not scope creep.
        assert!(!entry(new.id, carried).added_mid_sprint(&new));
    }

    #[test]
    fn removing_from_a_running_sprint_is_logged_and_undoable() {
        let (mut app, sid) = app_with_sprint();
        let id = add(&app, "a", "2025-01-01 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        app.backlog_sel_to_first_issue();
        app.handle_key(key(KeyCode::Char('s'))); // into the sprint
        assert_eq!(app.issue_by_id(id).unwrap().sprint_id, Some(sid));
        app.handle_key(key(KeyCode::Char('s'))); // and out again
        assert_eq!(app.issue_by_id(id).unwrap().sprint_id, None);
        let outcome = |app: &App| {
            app.db.get_sprint_entries().unwrap().get(&sid)
                .and_then(|v| v.iter().find(|e| e.issue.id == id).map(|e| e.outcome))
        };
        assert_eq!(outcome(&app), Some(Outcome::Removed));

        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(app.issue_by_id(id).unwrap().sprint_id, Some(sid));
        assert_eq!(outcome(&app), Some(Outcome::Open));
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(app.issue_by_id(id).unwrap().sprint_id, None);
        assert_eq!(outcome(&app), None, "undoing the add leaves no trace in the sprint");
    }

    #[test]
    fn one_frog_a_day_that_stays_put() {
        let (mut app, sid) = app_with_sprint();
        let old = add(&app, "old", "2025-01-01 00:00:00", Status::Todo, Some(sid));
        let carried = add(&app, "carried", "2026-01-01 00:00:00", Status::Todo, Some(sid));
        let started = add(&app, "started", "2024-01-01 00:00:00", Status::InProgress, Some(sid));
        app.db.conn().execute("UPDATE issues SET carry_count = 2 WHERE id = ?1", [carried]).unwrap();
        app.db.conn().execute("UPDATE issues SET carry_count = 3 WHERE id = ?1", [started]).unwrap();
        refreshed(&mut app);
        // Already started work isn't being avoided; the twice-carried todo is.
        assert_eq!(app.frog_id, Some(carried));

        // Starting the frog keeps it, and finishing it doesn't bring a second one today.
        app.focus_issue(carried);
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.issue_by_id(carried).unwrap().status, Status::InProgress);
        assert_eq!(app.frog_id, Some(carried));
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.issue_by_id(carried).unwrap().status, Status::Done);
        assert_eq!(app.frog_id, Some(carried));
        assert!(app.current_status().unwrap().starts_with("Frog done: carried"));

        // The next day, a finished frog is replaced...
        let yesterday = Local::now().date_naive() - Duration::days(1);
        app.frog_day = Some(yesterday);
        refreshed(&mut app);
        assert_eq!(app.frog_id, Some(old));
        // ...and an unfinished one carries over.
        app.frog_day = Some(yesterday);
        refreshed(&mut app);
        assert_eq!(app.frog_id, Some(old));

        // f picks a different frog for today; f on it again means no frog today.
        app.focus_issue(started);
        app.handle_key(key(KeyCode::Char('f')));
        assert_eq!(app.frog_id, Some(started));
        assert_eq!(app.db.get_setting("frog_id").unwrap(), Some(started.to_string()));
        app.handle_key(key(KeyCode::Char('f')));
        assert_eq!(app.frog_id, None);
        refreshed(&mut app);
        assert_eq!(app.frog_id, None, "no second frog the same day");
    }

    #[test]
    fn writes_from_another_process_show_up() {
        let path = std::env::temp_dir().join(format!("scrumtui_sync_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_string_lossy().to_string();
        let mut app = App::new(Db::open(&path_str).unwrap()).unwrap();
        app.sync_external_changes();
        assert!(app.current_status().is_none(), "nothing changed yet");

        let cli = Db::open(&path_str).unwrap();
        let id = cli.create_issue("from outside", 1.0, "x", &Status::Todo, None, None).unwrap();
        app.sync_external_changes();
        assert!(app.issue_by_id(id).is_some());
        assert!(app.current_status().unwrap().contains("from outside"));
        drop(cli);
        drop(app);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsaved_form_needs_a_second_esc() {
        let (mut app, _) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('n')));
        app.handle_key(key(KeyCode::Esc));
        assert!(app.popup.is_none(), "an untouched form closes right away");

        app.handle_key(key(KeyCode::Char('n')));
        type_text(&mut app, "draft");
        app.handle_key(key(KeyCode::Esc));
        assert!(form(&app).confirm_discard);
        app.handle_key(key(KeyCode::Char('!'))); // typing on cancels the discard
        assert!(!form(&app).confirm_discard);
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(key(KeyCode::Esc));
        assert!(app.popup.is_none());
    }

    #[test]
    fn sprint_manager_switches_between_edit_and_new() {
        let (mut app, sid) = app_with_current_sprint();
        app.handle_key(key(KeyCode::Char('S')));
        assert!(matches!(&app.popup, Some(Popup::SprintManager(f)) if f.editing_id == Some(sid)));
        app.handle_key(ctrl(KeyCode::Char('n')));
        let today = Local::now().date_naive();
        let next = (monday_of(today + Duration::days(2)) + Duration::days(7)).max(upcoming_monday(today));
        assert!(matches!(&app.popup, Some(Popup::SprintManager(f))
            if f.editing_id.is_none() && f.start_date == next.format("%Y-%m-%d").to_string()));
        app.handle_key(ctrl(KeyCode::Char('n')));
        assert!(matches!(&app.popup, Some(Popup::SprintManager(f)) if f.editing_id == Some(sid)));
    }

    #[test]
    fn editing_from_epic_detail_returns_to_it() {
        let (mut app, _) = app_with_sprint();
        app.db.create_issue_full("x", 1.0, "epic", &Status::Todo, None, None, None,
            "2025-01-01 00:00:00", "2025-01-01 00:00:00", None).unwrap();
        refreshed(&mut app);
        app.handle_key(key(KeyCode::Char('3')));
        app.handle_key(key(KeyCode::Char('e')));
        assert!(matches!(app.popup, Some(Popup::GanttEpicDetail { .. })));
        app.handle_key(key(KeyCode::Char('e')));
        assert!(matches!(app.popup, Some(Popup::EditIssue(_))));
        app.handle_key(key(KeyCode::Esc));
        assert!(matches!(&app.popup, Some(Popup::GanttEpicDetail { epic, .. }) if epic == "epic"));
    }

    #[test]
    fn tab_completes_a_typed_due_date_but_skips_an_empty_one() {
        let (mut app, _) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('n')));
        for _ in 0..4 { app.handle_key(key(KeyCode::Tab)); } // → due date
        assert!(form(&app).due_date_dropdown_open);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(form(&app).due_date, "");
        assert_eq!(form(&app).focused_field, F_DESC);

        app.handle_key(key(KeyCode::BackTab));
        type_text(&mut app, "tom");
        app.handle_key(key(KeyCode::Tab));
        let tomorrow = Local::now().date_naive() + Duration::days(1);
        assert_eq!(form(&app).due_date, tomorrow.format("%Y-%m-%d").to_string());
    }

    #[test]
    fn ctrl_n_adds_a_subtask_from_any_field() {
        let (mut app, _) = app_with_sprint();
        app.handle_key(key(KeyCode::Char('n')));
        type_text(&mut app, "parent");
        app.handle_key(ctrl(KeyCode::Char('n')));
        type_text(&mut app, "step one");
        let f = form(&app);
        assert_eq!(f.title, "parent");
        assert_eq!(f.subtasks.len(), 1);
        assert_eq!(f.subtasks[0].title, "step one");
    }

    #[test]
    fn esc_clears_a_kept_search_filter() {
        let (mut app, _) = app_with_sprint();
        add(&app, "alpha", "2025-01-01 00:00:00", Status::Todo, None);
        add(&app, "beta", "2025-01-02 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        app.handle_key(key(KeyCode::Char('/')));
        type_text(&mut app, "alp");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.backlog_items().iter().filter(|i| matches!(i, BacklogItem::Issue(..))).count(), 1);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.search_query.is_empty());
        assert_eq!(selected_title(&app).as_deref(), Some("alpha"));
    }

    #[test]
    fn search_finds_parents_by_subtask_title() {
        let (mut app, _) = app_with_sprint();
        let parent = add(&app, "parent", "2025-01-01 00:00:00", Status::Todo, None);
        app.db.create_subtask("needle", parent, &Status::Todo).unwrap();
        add(&app, "other", "2025-01-02 00:00:00", Status::Todo, None);
        refreshed(&mut app);
        app.search_query = "needle".into();
        let shown: Vec<String> = app.backlog_items().into_iter().filter_map(|bi| match bi {
            BacklogItem::Issue(i, _) => Some(i.title),
            _ => None,
        }).collect();
        assert_eq!(shown, ["parent"]);
    }

    #[test]
    fn demo_data_tells_a_consistent_story() {
        let db = Db::open(":memory:").unwrap();
        crate::seed::seed(&db).unwrap();
        let app = App::new(db).unwrap();
        let frog = app.frog().unwrap();
        assert_eq!(frog.title, "Fellowship proposal: research statement");
        assert_eq!(frog.carry_count, 5);
        let sprints = app.db.get_all_sprints().unwrap();
        assert_eq!(sprints.len(), 6);
        let entries = app.db.get_sprint_entries().unwrap();
        // Carried out of every past sprint, still open in the current one.
        for s in &sprints {
            let e = entries[&s.id].iter().find(|e| e.issue.id == frog.id).unwrap();
            let expected = if s.is_active { Outcome::Open } else { Outcome::Carried };
            assert_eq!(e.outcome, expected, "{}", s.name);
        }
        // Every sprint has finished work and each issue's history adds up.
        for s in sprints.iter().filter(|s| !s.is_active) {
            assert!(entries[&s.id].iter().any(|e| e.outcome == Outcome::Done), "{}", s.name);
        }
        assert!(entries.values().flatten().any(|e| e.outcome == Outcome::Removed));
    }
}
