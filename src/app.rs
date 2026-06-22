use anyhow::Result;
use chrono::{Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::time::Instant;

use crate::db::Db;
use crate::models::{Issue, Sprint, Status};

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

// ── Issue form ────────────────────────────────────────────────────────────────

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
    pub focused_field: usize,  // 0=title 1=epic 2=sp 3=status 4=due 5=desc
    pub error: Option<String>,
    /// Whether the epic autocomplete dropdown is visible.
    pub epic_dropdown_open: bool,
    /// Selected row in the epic dropdown.
    pub epic_dropdown_sel: usize,
    /// Whether the due-date autocomplete dropdown is visible.
    pub due_date_dropdown_open: bool,
    /// Selected row in the due-date dropdown.
    pub due_date_dropdown_sel: usize,
    /// Whether the status picker dropdown is open.
    pub status_dropdown_open: bool,
    /// Selected row in the status dropdown (0=Todo 1=InProgress 2=Done).
    pub status_dropdown_sel: usize,
    /// Subtasks (only populated when editing an existing issue).
    pub subtasks: Vec<SubtaskDraft>,
    /// Selected row in the subtask list.
    pub subtask_sel: usize,
    /// Whether keyboard focus is in the subtask list.
    pub in_subtask_list: bool,
    /// Whether the currently selected subtask title is being edited.
    pub subtask_editing: bool,
}

impl IssueForm {
    pub fn new() -> Self {
        IssueForm {
            editing_id: None,
            sprint_id: None,
            title: String::new(),
            story_points: String::from("1"),
            epic: String::new(),
            status_idx: 0,
            due_date: String::new(),
            description: String::new(),
            focused_field: 0,
            error: None,
            epic_dropdown_open: false,
            epic_dropdown_sel: 0,
            due_date_dropdown_open: false,
            due_date_dropdown_sel: 0,
            status_dropdown_open: false,
            status_dropdown_sel: 0,
            subtasks: Vec::new(),
            subtask_sel: 0,
            in_subtask_list: false,
            subtask_editing: false,
        }
    }

    pub fn from_issue(issue: &Issue) -> Self {
        IssueForm {
            editing_id: Some(issue.id),
            sprint_id: None,
            title: issue.title.clone(),
            story_points: issue.story_points.to_string(),
            epic: issue.epic.clone(),
            status_idx: issue.status.index(),
            due_date: issue.due_date_str(),
            description: issue.description.clone().unwrap_or_default(),
            focused_field: 0,
            error: None,
            epic_dropdown_open: false,
            epic_dropdown_sel: 0,
            due_date_dropdown_open: false,
            due_date_dropdown_sel: 0,
            status_dropdown_open: false,
            status_dropdown_sel: 0,
            subtasks: Vec::new(),
            subtask_sel: 0,
            in_subtask_list: false,
            subtask_editing: false,
        }
    }

    /// Returns a mutable reference to the string buffer of the currently focused text field,
    /// or None if the focused field is not a text field (e.g. status).
    pub fn active_text_field(&mut self) -> Option<&mut String> {
        match self.focused_field {
            0 => Some(&mut self.title),
            1 => Some(&mut self.epic),
            2 => Some(&mut self.story_points),
            3 => None, // status cycles with h/l
            4 => Some(&mut self.due_date),
            5 => Some(&mut self.description),
            _ => None,
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
        if !self.due_date.is_empty() {
            if NaiveDate::parse_from_str(&self.due_date, "%Y-%m-%d").is_err() {
                return Some("Due date must be YYYY-MM-DD.".into());
            }
        }
        None
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
}

impl SprintForm {
    pub fn new() -> Self {
        let today = Local::now().date_naive();
        // Default to a full Monday–Sunday week containing today.
        use chrono::Datelike;
        let days_from_monday = today.weekday().num_days_from_monday();
        let monday = today - chrono::Duration::days(days_from_monday as i64);
        let sunday = monday + chrono::Duration::days(6);
        SprintForm {
            editing_id: None,
            name: String::from("Sprint"),
            start_date: monday.format("%Y-%m-%d").to_string(),
            end_date: sunday.format("%Y-%m-%d").to_string(),
            is_active: true,
            focused_field: 0,
            error: None,
        }
    }

    pub fn from_sprint(s: &Sprint) -> Self {
        SprintForm {
            editing_id: Some(s.id),
            name: s.name.clone(),
            start_date: s.start_date.format("%Y-%m-%d").to_string(),
            end_date: s.end_date.format("%Y-%m-%d").to_string(),
            is_active: s.is_active,
            focused_field: 0,
            error: None,
        }
    }

    pub fn active_text_field(&mut self) -> Option<&mut String> {
        match self.focused_field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.start_date),
            2 => Some(&mut self.end_date),
            3 => None, // boolean toggle
            _ => None,
        }
    }

    pub fn field_count() -> usize {
        4
    }

    pub fn validate(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("Name is required.".into());
        }
        if NaiveDate::parse_from_str(&self.start_date, "%Y-%m-%d").is_err() {
            return Some("Start date must be YYYY-MM-DD.".into());
        }
        if NaiveDate::parse_from_str(&self.end_date, "%Y-%m-%d").is_err() {
            return Some("End date must be YYYY-MM-DD.".into());
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
    /// Gantt epic detail: epic name, issues list, search query, scroll offset
    GanttEpicDetail {
        epic: String,
        issues: Vec<Issue>,
        search: String,
        search_active: bool,
        scroll: usize,
    },
}

// ── Undo ─────────────────────────────────────────────────────────────────────

/// Captures enough state to reverse a single user action.
#[derive(Clone)]
pub enum UndoAction {
    /// A status-only change: restore the old status (and completed_at) on one issue.
    StatusChange {
        issue_id: i64,
        old_status: Status,
        /// Parent id — if set, cascade update_parent_status_from_children after undo.
        parent_id: Option<i64>,
    },
    /// Full issue edit or create — restore the previous Issue snapshot (and its subtasks).
    IssueSnapshot {
        /// The full Issue row before the edit.  None = issue was newly created (undo = soft-delete).
        before: Option<Issue>,
        /// Subtask snapshots before the edit (empty for creates).
        subtasks_before: Vec<Issue>,
        /// Newly created subtask ids that should be soft-deleted on undo.
        new_subtask_ids: Vec<i64>,
    },
    /// Issue was soft-deleted — undo by restoring from trash.
    SoftDelete { issue_id: i64 },
    /// Sprint membership toggled — undo by toggling back.
    SprintToggle { issue_id: i64, old_sprint_id: Option<i64> },
    /// Two issues had their ranks swapped.
    RankSwap { id_a: i64, id_b: i64 },
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
    /// Kanban: current column (0=Todo 1=InProgress 2=Done)
    pub kanban_col: usize,
    /// Kanban: selected row within each column (parent issues panel)
    pub kanban_rows: [usize; 3],
    /// Kanban: 0=parent panel focused, 1=subtask panel focused
    pub kanban_panel: usize,
    /// Kanban: selected subtask row per column (subtask panel)
    pub kanban_sub_rows: [usize; 3],
    /// Kanban: index into the list of parents-with-subtasks for the subtask panel
    pub kanban_sub_parent_idx: usize,
    /// Gantt scroll offset (rows)
    pub gantt_scroll: usize,
    /// Gantt selected epic index (for Enter to open detail popup)
    pub gantt_sel: usize,
    pub popup: Option<Popup>,
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
    pub history_sprints: Vec<crate::models::Sprint>,
    /// Sprint history: selected sprint index.
    pub history_sel: usize,
    /// Sprint history: issues for the selected sprint (loaded on selection change).
    pub history_issues: Vec<Issue>,
    /// Undo stack — up to 50 entries, most-recent last (pop from end).
    pub undo_stack: Vec<UndoAction>,
    /// When set, the next kanban column switch should follow this issue (id, target_col).
    pub pending_kanban_follow: Option<(i64, usize)>,
    /// Cached sorted unique epic names — refreshed on reload().
    epics_cache: Vec<String>,
    /// Issue IDs that were just marked Done in the backlog this session.
    /// These are kept visible until flush_display() is called (i.e., view switch).
    recently_completed: std::collections::HashSet<i64>,
}

impl App {
    pub fn new(db: Db) -> Result<Self> {
        let issues = db.get_all_issues()?;
        let active_sprint = db.get_active_sprint()?;
        // Restore persisted show_completed preference (defaults to true)
        let show_completed = db
            .get_setting("show_completed")
            .ok()
            .flatten()
            .map(|v| v != "false")
            .unwrap_or(true);
        let display_issues = issues.clone();
        let epics_cache = Self::build_epics_cache(&issues);
        Ok(App {
            view: View::Backlog,
            db,
            display_issues,
            issues,
            active_sprint,
            backlog_sel: 0,
            kanban_col: 0,
            kanban_rows: [0, 0, 0],
            kanban_panel: 0,
            kanban_sub_rows: [0, 0, 0],
            kanban_sub_parent_idx: 0,
            gantt_scroll: 0,
            gantt_sel: 0,
            popup: None,
            status_msg: None,
            status_msg_at: None,
            show_completed,
            search_query: String::new(),
            search_active: false,
            history_sprints: Vec::new(),
            history_sel: 0,
            history_issues: Vec::new(),
            undo_stack: Vec::new(),
            pending_kanban_follow: None,
            epics_cache,
            recently_completed: std::collections::HashSet::new(),
        })
    }

    pub fn reload(&mut self) -> Result<()> {
        self.issues = self.db.get_all_issues()?;
        self.active_sprint = self.db.get_active_sprint()?;
        // Refresh epics cache
        self.epics_cache = Self::build_epics_cache(&self.issues);
        // Clamp selection to valid range
        let n = self.backlog_items().len();
        if self.backlog_sel >= n && n > 0 {
            self.backlog_sel = n - 1;
        }
        Ok(())
    }

    fn build_epics_cache(issues: &[Issue]) -> Vec<String> {
        use chrono::NaiveDateTime;
        // Compute the earliest created_at across all top-level issues per epic.
        let mut epic_starts: std::collections::HashMap<String, NaiveDateTime> =
            std::collections::HashMap::new();
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

    /// Refresh display_issues from the live issues list.  Called on view switch
    /// so that order/visibility changes take effect when leaving a view.
    pub fn flush_display(&mut self) {
        self.display_issues = self.issues.clone();
        self.recently_completed.clear();
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

    /// Set a status message that will display for ~3 seconds.
    pub fn set_status(&mut self, msg: impl Into<String> + std::fmt::Display) {
        self.status_msg = Some(msg.to_string());
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
        let action = match self.undo_stack.pop() {
            Some(a) => a,
            None => {
                self.set_status("Nothing to undo.");
                return;
            }
        };
        match action {
            UndoAction::StatusChange { issue_id, old_status, parent_id } => {
                if let Err(e) = self.db.update_issue_status(issue_id, &old_status) {
                    self.set_status(format!("Undo failed: {e}"));
                    return;
                }
                if let Some(pid) = parent_id {
                    let _ = self.db.update_parent_status_from_children(pid);
                }
                let _ = self.reload();
                // Patch display so the status symbol reverts immediately in-view
                self.patch_display_status(issue_id, &old_status);
                if let Some(pid) = parent_id {
                    if let Some(parent) = self.issues.iter().find(|i| i.id == pid).cloned() {
                        self.patch_display_status(pid, &parent.status);
                    }
                }
                self.set_status(format!("Undid status change → {}", old_status.label()));
            }
            UndoAction::IssueSnapshot { before, subtasks_before, new_subtask_ids } => {
                match before {
                    None => {
                        // Issue was newly created — find it by checking the most recently
                        // inserted id that matches. We track via new_subtask_ids and the
                        // snapshot: the issue id is encoded as the first element of new_subtask_ids
                        // with a sentinel, or we look at the last created issue.
                        // Actually simpler: we embedded the id in new_subtask_ids[0] using
                        // a convention — see push sites. For "create" undos we store the
                        // new issue id in new_subtask_ids[0] with a negative sign as sentinel.
                        // Let's handle it: the first entry is the new issue id.
                        if let Some(&new_id) = new_subtask_ids.first() {
                            if let Err(e) = self.db.delete_issue(new_id) {
                                self.set_status(format!("Undo failed: {e}"));
                                return;
                            }
                        }
                        let _ = self.reload();
                        self.flush_display();
                        self.set_status("Undid issue creation (moved to trash).");
                    }
                    Some(snapshot) => {
                        // Restore the issue row to its prior state
                        let due = snapshot.due_date;
                        let desc = snapshot.description.as_deref();
                        if let Err(e) = self.db.restore_issue_to_snapshot(&snapshot) {
                            self.set_status(format!("Undo failed: {e}"));
                            return;
                        }
                        // Restore each previous subtask
                        for sub in &subtasks_before {
                            let _ = self.db.restore_issue_to_snapshot(sub);
                        }
                        // Soft-delete any subtasks that were newly created in that edit
                        for id in &new_subtask_ids {
                            let _ = self.db.delete_issue(*id);
                        }
                        let _ = self.reload();
                        self.flush_display();
                        let _ = due; let _ = desc; // suppress unused warnings
                        self.set_status("Undid issue edit.");
                    }
                }
            }
            UndoAction::SoftDelete { issue_id } => {
                if let Err(e) = self.db.restore_issue(issue_id) {
                    self.set_status(format!("Undo failed: {e}"));
                    return;
                }
                let _ = self.reload();
                self.flush_display();
                self.set_status("Undid delete (issue restored).");
            }
            UndoAction::SprintToggle { issue_id, old_sprint_id } => {
                if let Err(e) = self.db.set_issue_sprint(issue_id, old_sprint_id) {
                    self.set_status(format!("Undo failed: {e}"));
                    return;
                }
                let _ = self.reload();
                self.flush_display();
                self.set_status(if old_sprint_id.is_some() {
                    "Undid sprint removal (back in sprint)."
                } else {
                    "Undid sprint add (back in backlog)."
                });
            }
            UndoAction::RankSwap { id_a, id_b } => {
                if let Err(e) = self.db.swap_rank(id_a, id_b) {
                    self.set_status(format!("Undo failed: {e}"));
                    return;
                }
                let _ = self.reload();
                self.flush_display();
                self.set_status("Undid rank change.");
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
        let mut children_map: std::collections::HashMap<i64, Vec<Issue>> =
            std::collections::HashMap::new();
        for issue in &self.display_issues {
            if let Some(pid) = issue.parent_id {
                children_map.entry(pid).or_default().push(issue.clone());
            }
        }
        for subs in children_map.values_mut() {
            subs.reverse();
        }

        let search_matches = |issue: &Issue| -> bool {
            if q.is_empty() { return true; }
            let haystack = format!(
                "{} {} {}",
                issue.title.to_lowercase(),
                issue.epic.to_lowercase(),
                issue.description.as_deref().unwrap_or("").to_lowercase()
            );
            haystack.contains(&q)
        };

        let emit_with_subtasks = |items: &mut Vec<BacklogItem>,
                                   issue: Issue,
                                   in_sprint: bool,
                                   subtasks_map: &std::collections::HashMap<i64, Vec<Issue>>| {
            let id = issue.id;
            items.push(BacklogItem::Issue(issue, in_sprint));
            if let Some(subs) = subtasks_map.get(&id) {
                for sub in subs {
                    items.push(BacklogItem::Subtask(sub.clone(), in_sprint));
                }
            }
        };

        if let Some(sprint) = &self.active_sprint {
            // Sprint issues: always show Done; search filter still applies.
            // display_issues is already sorted by rank DESC so order is preserved.
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
                    emit_with_subtasks(&mut items, issue, true, &children_map);
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
                emit_with_subtasks(&mut items, issue, false, &children_map);
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

    /// Look up an issue by id from the loaded cache.
    pub fn issue_by_id(&self, id: i64) -> Option<&Issue> {
        self.issues.iter().find(|i| i.id == id)
    }

    /// All sprint parent issues that have at least one subtask, in order.
    pub fn sprint_parents_with_subtasks(&self) -> Vec<Issue> {
        match &self.active_sprint {
            Some(s) => {
                let parents: Vec<Issue> = self
                    .display_issues
                    .iter()
                    .filter(|i| i.sprint_id == Some(s.id) && i.parent_id.is_none())
                    .cloned()
                    .collect();
                parents
                    .into_iter()
                    .filter(|p| self.display_issues.iter().any(|i| i.parent_id == Some(p.id)))
                    .collect()
            }
            None => vec![],
        }
    }

    /// The currently focused parent for the subtask panel (based on kanban_sub_parent_idx).
    pub fn kanban_sub_parent(&self) -> Option<Issue> {
        let parents = self.sprint_parents_with_subtasks();
        let idx = self.kanban_sub_parent_idx.min(parents.len().saturating_sub(1));
        parents.into_iter().nth(idx)
    }

    /// All subtasks of the currently focused sub-panel parent, by status.
    pub fn sprint_subtasks_by_status(&self, status: &Status) -> Vec<Issue> {
        match self.kanban_sub_parent() {
            Some(parent) => self
                .display_issues
                .iter()
                .filter(|i| i.parent_id == Some(parent.id) && &i.status == status)
                .cloned()
                .collect(),
            None => vec![],
        }
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
        match &self.active_sprint {
            Some(s) => {
                let parent_ids: std::collections::HashSet<i64> = self
                    .display_issues
                    .iter()
                    .filter(|i| i.sprint_id == Some(s.id) && i.parent_id.is_none())
                    .map(|i| i.id)
                    .collect();
                self.display_issues
                    .iter()
                    .any(|i| i.parent_id.map(|pid| parent_ids.contains(&pid)).unwrap_or(false))
            }
            None => false,
        }
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
        let subs = self.sprint_subtasks_flat();
        subs.into_iter().nth(self.kanban_sub_rows[0])
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

    /// Build SubtaskDraft list from loaded issues for a given parent.
    pub fn subtask_drafts_for(&self, parent_id: i64) -> Vec<SubtaskDraft> {
        let mut drafts: Vec<SubtaskDraft> = self.issues
            .iter()
            .filter(|i| i.parent_id == Some(parent_id))
            .map(|i| SubtaskDraft {
                id: Some(i.id),
                title: i.title.clone(),
                status_idx: i.status.index(),
                deleted: false,
            })
            .collect();
        // DB returns newest-first; reverse to show subtasks in creation order.
        drafts.reverse();
        drafts
    }

    /// Unique epics across all issues, sorted alphabetically (cached).
    pub fn epics(&self) -> &[String] {
        &self.epics_cache
    }

    /// Today's date as "YYYY-MM-DD" for the due-date autocomplete.
    pub fn today_str(&self) -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// Unique due dates across all issues, sorted ascending, with today always first.
    pub fn due_dates(&self) -> Vec<String> {
        let today = self.today_str();
        let mut dates: Vec<String> = self
            .issues
            .iter()
            .filter_map(|i| i.due_date.map(|d| d.format("%Y-%m-%d").to_string()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        // Sort dates after removing today (we always push it first)
        dates.retain(|d| d != &today);
        dates.sort();
        let mut result = vec![today];
        result.extend(dates);
        result
    }

    // ── Navigation helpers ─────────────────────────────────────────────────────

    fn is_selectable(item: &BacklogItem) -> bool {
        matches!(item, BacklogItem::Issue(_, _) | BacklogItem::Subtask(_, _))
    }

    pub fn backlog_down(&mut self) {
        let items = self.backlog_items();
        let start = self.backlog_sel + 1;
        for i in start..items.len() {
            if Self::is_selectable(&items[i]) {
                self.backlog_sel = i;
                return;
            }
        }
    }

    pub fn backlog_up(&mut self) {
        if self.backlog_sel == 0 {
            return;
        }
        let items = self.backlog_items();
        let mut i = self.backlog_sel - 1;
        loop {
            if Self::is_selectable(&items[i]) {
                self.backlog_sel = i;
                return;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }

    pub fn backlog_sel_to_first_issue(&mut self) {
        let items = self.backlog_items();
        for (i, item) in items.iter().enumerate() {
            if Self::is_selectable(item) {
                self.backlog_sel = i;
                return;
            }
        }
    }

    pub fn kanban_down(&mut self) {
        let col = self.kanban_col;
        if self.kanban_panel == 0 {
            let len = self.sprint_parents_by_status(&Status::from_index(col)).len();
            if len == 0 { return; }
            let row = &mut self.kanban_rows[col];
            if *row + 1 < len { *row += 1; }
        } else {
            // Flat sub panel
            let len = self.sprint_subtasks_flat().len();
            if len == 0 { return; }
            if self.kanban_sub_rows[0] + 1 < len { self.kanban_sub_rows[0] += 1; }
        }
    }

    pub fn kanban_up(&mut self) {
        let col = self.kanban_col;
        if self.kanban_panel == 0 {
            let row = &mut self.kanban_rows[col];
            if *row > 0 { *row -= 1; }
        } else {
            // Flat sub panel
            if self.kanban_sub_rows[0] > 0 { self.kanban_sub_rows[0] -= 1; }
        }
    }

    /// The currently focused issue (parent or subtask depending on kanban_panel).
    pub fn kanban_selected_issue(&self) -> Option<Issue> {
        if self.kanban_panel == 0 {
            self.kanban_selected_parent()
        } else {
            self.kanban_selected_subtask()
        }
    }

    /// After switching kanban columns (or after a status change that moves an issue),
    /// try to keep focus on the same issue id. Clamps rows for both panels.
    fn kanban_clamp_and_follow(&mut self, old_id: Option<i64>) {
        let col = self.kanban_col;
        let status = Status::from_index(col);
        let parents = self.sprint_parents_by_status(&status);
        let subs = self.sprint_subtasks_by_status(&status);

        // Clamp parent row
        let plen = parents.len();
        if plen == 0 {
            self.kanban_rows[col] = 0;
        } else if self.kanban_rows[col] >= plen {
            self.kanban_rows[col] = plen - 1;
        }

        // Clamp flat subtask row
        let _ = subs; // no longer used per-column
        let flat_len = self.sprint_subtasks_flat().len();
        if flat_len == 0 {
            self.kanban_sub_rows[0] = 0;
        } else if self.kanban_sub_rows[0] >= flat_len {
            self.kanban_sub_rows[0] = flat_len - 1;
        }

        // Check if a pending follow lands in this column
        if let Some((follow_id, target_col)) = self.pending_kanban_follow {
            if target_col == col {
                // Search parents first
                if let Some(pos) = parents.iter().position(|i| i.id == follow_id) {
                    self.kanban_rows[col] = pos;
                    self.kanban_panel = 0;
                    self.pending_kanban_follow = None;
                    return;
                }
                // Then subtasks
                if let Some(pos) = subs.iter().position(|i| i.id == follow_id) {
                    self.kanban_sub_rows[col] = pos;
                    self.kanban_panel = 1;
                    self.pending_kanban_follow = None;
                    return;
                }
            }
            self.pending_kanban_follow = None;
        }

        // Try to find old focused issue id in parents
        if let Some(id) = old_id {
            if let Some(pos) = parents.iter().position(|i| i.id == id) {
                self.kanban_rows[col] = pos;
                self.kanban_panel = 0;
                return;
            }
            if let Some(pos) = subs.iter().position(|i| i.id == id) {
                self.kanban_sub_rows[col] = pos;
                self.kanban_panel = 1;
            }
        }
    }

    // ── Key handling ───────────────────────────────────────────────────────────

    /// Returns true when the app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Ctrl-C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
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
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                        delete_word(&mut self.search_query);
                    } else {
                        self.search_query.push(c);
                    }
                    self.backlog_sel_to_first_issue();
                }
                _ => {}
            }
            return false;
        }

        // Global keys
        match key.code {
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.undo();
                return false;
            }
            KeyCode::Char('u') => {
                self.undo();
                return false;
            }
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => {
                self.popup = Some(Popup::Help);
                return false;
            }
            KeyCode::Char('1') => {
                if self.view != View::Backlog { self.flush_display(); }
                self.view = View::Backlog;
                return false;
            }
            KeyCode::Char('2') => {
                if self.view != View::Kanban { self.flush_display(); }
                self.view = View::Kanban;
                return false;
            }
            KeyCode::Char('3') => {
                if self.view != View::Gantt { self.flush_display(); }
                self.view = View::Gantt;
                return false;
            }
            KeyCode::Char('4') => {
                self.flush_display();
                self.load_sprint_history();
                self.view = View::SprintHistory;
                return false;
            }
            _ => {}
        }

        match self.view {
            View::Backlog => self.handle_backlog_key(key),
            View::Kanban => self.handle_kanban_key(key),
            View::Gantt => self.handle_gantt_key(key),
            View::SprintHistory => self.handle_history_key(key),
        }

        false
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
        match key.code {
            KeyCode::Char('j') => self.backlog_down(),
            KeyCode::Char('k') => self.backlog_up(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                for _ in 0..10 { self.backlog_down(); }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                for _ in 0..10 { self.backlog_up(); }
            }
            KeyCode::Char('g') => self.backlog_sel_to_first_issue(),
            KeyCode::Char('/') => {
                self.search_active = true;
            }
            KeyCode::Char('c') => {
                self.show_completed = !self.show_completed;
                let _ = self.db.set_setting(
                    "show_completed",
                    if self.show_completed { "true" } else { "false" },
                );
                let len = self.backlog_items().len();
                if self.backlog_sel >= len && len > 0 {
                    self.backlog_sel = len - 1;
                }
                self.backlog_sel_to_first_issue();
            }
            KeyCode::Char('G') => {
                // jump to last issue
                let items = self.backlog_items();
                for i in (0..items.len()).rev() {
                    if Self::is_selectable(&items[i]) {
                        self.backlog_sel = i;
                        break;
                    }
                }
            }
            KeyCode::Char('n') => {
                let mut form = IssueForm::new();
                // If currently focused on a sprint item, pre-assign the new issue to that sprint
                let items = self.backlog_items();
                let in_sprint = matches!(
                    items.get(self.backlog_sel),
                    Some(BacklogItem::Issue(_, true)) | Some(BacklogItem::Subtask(_, true))
                        | Some(BacklogItem::SprintHeader(_)) | Some(BacklogItem::SprintFooter)
                );
                if in_sprint {
                    if let Some(sprint) = &self.active_sprint {
                        form.sprint_id = Some(sprint.id);
                    }
                }
                self.popup = Some(Popup::NewIssue(form));
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.selected_issue() {
                    let mut form = IssueForm::from_issue(&issue);
                    form.subtasks = self.subtask_drafts_for(issue.id);
                    self.popup = Some(Popup::EditIssue(form));
                }
            }
            KeyCode::Char('d') => {
                if let Some(issue) = self.selected_issue() {
                    self.popup = Some(Popup::ConfirmDelete(issue.id, issue.title.clone()));
                }
            }
            KeyCode::Char('s') => {
                // Toggle sprint membership
                if let Some(issue) = self.selected_issue() {
                    if let Some(sprint) = &self.active_sprint {
                        let new_sprint_id = if issue.sprint_id == Some(sprint.id) {
                            None
                        } else {
                            Some(sprint.id)
                        };
                        self.push_undo(UndoAction::SprintToggle {
                            issue_id: issue.id,
                            old_sprint_id: issue.sprint_id,
                        });
                        if let Err(e) = self.db.set_issue_sprint(issue.id, new_sprint_id) {
                            self.undo_stack.pop(); // rollback the push on error
                            self.set_status(format!("Error: {e}"));
                        } else {
                            let _ = self.reload();
                            self.flush_display();
                            self.set_status(if new_sprint_id.is_some() {
                                "Moved to sprint."
                            } else {
                                "Moved to backlog."
                            });
                        }
                    } else {
                        self.set_status("No active sprint. Press S to create one.");
                    }
                }
            }
            KeyCode::Char('S') => {
                let form = match &self.active_sprint {
                    Some(s) => SprintForm::from_sprint(s),
                    None => SprintForm::new(),
                };
                self.popup = Some(Popup::SprintManager(form));
            }
            KeyCode::Char('T') => {
                match self.db.get_trash() {
                    Ok(items) => self.popup = Some(Popup::Trash { items, sel: 0 }),
                    Err(e) => self.set_status(format!("Error: {e}")),
                }
            }
            KeyCode::Char(']') => {
                self.backlog_advance_status(1);
            }
            KeyCode::Char('[') => {
                self.backlog_advance_status(-1);
            }
            _ => {}
        }
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

        // Collect peer issues (same sprint_id context) in current display order.
        // display_issues is rank DESC sorted, so peers are already in display order.
        let peers: Vec<Issue> = items.iter().filter_map(|bi| {
            if let BacklogItem::Issue(i, _) = bi {
                if i.sprint_id == current_issue.sprint_id {
                    return Some(i.clone());
                }
            }
            None
        }).collect();

        let pos = match peers.iter().position(|i| i.id == current_issue.id) {
            Some(p) => p,
            None => return,
        };

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
        self.push_undo(UndoAction::RankSwap { id_a: current_issue.id, id_b: other.id });
        if let Err(e) = self.db.swap_rank(current_issue.id, other.id) {
            self.undo_stack.pop();
            self.set_status(format!("Error: {e}"));
            return;
        }
        let _ = self.reload();
        self.flush_display();
        // Move the selection to follow the issue in its new position
        let new_items = self.backlog_items();
        if let Some(new_pos) = new_items.iter().position(|bi| {
            matches!(bi, BacklogItem::Issue(i, _) if i.id == current_issue.id)
        }) {
            self.backlog_sel = new_pos;
        }
    }

    /// Advance (+1) or regress (-1) the selected backlog item's status.
    fn backlog_advance_status(&mut self, dir: i32) {
        let items = self.backlog_items();
        match items.get(self.backlog_sel).cloned() {
            Some(BacklogItem::Issue(issue, _)) => {
                if self.has_subtasks(issue.id) {
                    self.set_status("Status is managed by subtasks.");
                    return;
                }
                let new_status = if dir > 0 { issue.status.next() } else { issue.status.prev() };
                if new_status != issue.status {
                    self.push_undo(UndoAction::StatusChange {
                        issue_id: issue.id,
                        old_status: issue.status.clone(),
                        parent_id: None,
                    });
                    let _ = self.db.update_issue_status(issue.id, &new_status);
                    let _ = self.reload();
                    // Patch display snapshot in-place — keeps position/visibility stable
                    self.patch_display_status(issue.id, &new_status);
                }
            }
            Some(BacklogItem::Subtask(sub, _)) => {
                let new_status = if dir > 0 { sub.status.next() } else { sub.status.prev() };
                if new_status != sub.status {
                    self.push_undo(UndoAction::StatusChange {
                        issue_id: sub.id,
                        old_status: sub.status.clone(),
                        parent_id: sub.parent_id,
                    });
                    let _ = self.db.update_issue_status(sub.id, &new_status);
                    let _ = self.reload();
                    self.patch_display_status(sub.id, &new_status);
                    // Also patch parent so its derived status badge updates
                    if let Some(pid) = sub.parent_id {
                        if let Some(parent) = self.issues.iter().find(|i| i.id == pid).cloned() {
                            self.patch_display_status(pid, &parent.status);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_kanban_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') => {
                if self.kanban_col > 0 {
                    let focused_id = self.kanban_selected_issue().map(|i| i.id);
                    self.kanban_col -= 1;
                    self.kanban_clamp_and_follow(focused_id);
                }
            }
            KeyCode::Char('l') => {
                if self.kanban_col < 2 {
                    let focused_id = self.kanban_selected_issue().map(|i| i.id);
                    self.kanban_col += 1;
                    self.kanban_clamp_and_follow(focused_id);
                }
            }
            KeyCode::Tab => {
                // Switch between parent and subtask panels (only if subtasks exist)
                if self.sprint_has_any_subtasks() {
                    self.kanban_panel = 1 - self.kanban_panel;
                    // When entering the subtask panel, sync sub_parent_idx to the
                    // currently selected parent (if it has subtasks).
                    if self.kanban_panel == 1 {
                        if let Some(parent) = self.kanban_selected_parent() {
                            let parents_with_subs = self.sprint_parents_with_subtasks();
                            if let Some(pos) = parents_with_subs.iter().position(|p| p.id == parent.id) {
                                self.kanban_sub_parent_idx = pos;
                            }
                        }
                        // Clamp flat subtask row
                        let len = self.sprint_subtasks_flat().len();
                        if self.kanban_sub_rows[0] >= len.max(1) {
                            self.kanban_sub_rows[0] = len.saturating_sub(1);
                        }
                    } else {
                        let col = self.kanban_col;
                        let status = Status::from_index(col);
                        let len = self.sprint_parents_by_status(&status).len();
                        if self.kanban_rows[col] >= len.max(1) {
                            self.kanban_rows[col] = len.saturating_sub(1);
                        }
                    }
                }
            }
            // Cycle through parents in the subtask panel with < and >
            KeyCode::Char('<') | KeyCode::Char(',') => {
                if self.kanban_panel == 1 {
                    let len = self.sprint_parents_with_subtasks().len();
                    if self.kanban_sub_parent_idx > 0 {
                        self.kanban_sub_parent_idx -= 1;
                    } else if len > 0 {
                        self.kanban_sub_parent_idx = len - 1;
                    }
                    let sub_len = self.sprint_subtasks_flat().len();
                    if self.kanban_sub_rows[0] >= sub_len.max(1) {
                        self.kanban_sub_rows[0] = sub_len.saturating_sub(1);
                    }
                }
            }
            KeyCode::Char('>') | KeyCode::Char('.') => {
                if self.kanban_panel == 1 {
                    let len = self.sprint_parents_with_subtasks().len();
                    if len > 0 {
                        self.kanban_sub_parent_idx = (self.kanban_sub_parent_idx + 1) % len;
                    }
                    let sub_len = self.sprint_subtasks_flat().len();
                    if self.kanban_sub_rows[0] >= sub_len.max(1) {
                        self.kanban_sub_rows[0] = sub_len.saturating_sub(1);
                    }
                }
            }
            KeyCode::Char('j') => self.kanban_down(),
            KeyCode::Char('k') => self.kanban_up(),
            KeyCode::Char(']') => self.kanban_advance_status(1),
            KeyCode::Char('[') => self.kanban_advance_status(-1),
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.kanban_selected_issue() {
                    let target = if issue.is_subtask() {
                        issue.parent_id.and_then(|pid| self.issue_by_id(pid).cloned())
                    } else {
                        Some(issue)
                    };
                    if let Some(parent) = target {
                        let mut form = IssueForm::from_issue(&parent);
                        form.subtasks = self.subtask_drafts_for(parent.id);
                        self.popup = Some(Popup::EditIssue(form));
                    }
                }
            }
            _ => {}
        }
    }

    fn kanban_advance_status(&mut self, dir: i32) {
        let issue = match self.kanban_selected_issue() {
            Some(i) => i,
            None => return,
        };
        // Parent issues with subtasks: block direct status change
        if !issue.is_subtask() && self.has_subtasks(issue.id) {
            self.set_status("Status is managed by subtasks.");
            return;
        }
        let new_status = if dir > 0 { issue.status.next() } else { issue.status.prev() };
        if new_status == issue.status { return; }

        self.push_undo(UndoAction::StatusChange {
            issue_id: issue.id,
            old_status: issue.status.clone(),
            parent_id: issue.parent_id,
        });
        let _ = self.db.update_issue_status(issue.id, &new_status);
        let _ = self.reload();
        // Flush display immediately so the issue moves to its new column right now
        self.flush_display();
        // Follow the issue: switch column and find it
        let new_col = new_status.index();
        self.kanban_col = new_col;
        let is_sub = issue.is_subtask();
        if is_sub {
            self.kanban_panel = 1;
            // Flat sub panel: find position in all-status flat list, keep col unchanged
            let subs = self.sprint_subtasks_flat();
            if let Some(pos) = subs.iter().position(|i| i.id == issue.id) {
                self.kanban_sub_rows[0] = pos;
            } else {
                self.kanban_sub_rows[0] = subs.len().saturating_sub(1);
            }
            // Also update parent status display
            if let Some(pid) = issue.parent_id {
                if let Some(parent) = self.issues.iter().find(|i| i.id == pid).cloned() {
                    self.patch_display_status(pid, &parent.status);
                }
            }
        } else {
            self.kanban_panel = 0;
            let parents = self.sprint_parents_by_status(&new_status);
            if let Some(pos) = parents.iter().position(|i| i.id == issue.id) {
                self.kanban_rows[new_col] = pos;
            } else {
                self.kanban_rows[new_col] = parents.len().saturating_sub(1);
            }
        }
    }

    fn handle_gantt_key(&mut self, key: KeyEvent) {
        let epic_count = self.epics().len();
        match key.code {
            KeyCode::Char('j') => {
                if epic_count > 0 && self.gantt_sel + 1 < epic_count {
                    self.gantt_sel += 1;
                }
            }
            KeyCode::Char('k') => {
                if self.gantt_sel > 0 {
                    self.gantt_sel -= 1;
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                let epics = self.epics();
                if let Some(epic) = epics.get(self.gantt_sel) {
                    let epic_issues: Vec<Issue> = self.issues.iter()
                        .filter(|i| i.parent_id.is_none() && &i.epic == epic)
                        .cloned()
                        .collect();
                    self.popup = Some(Popup::GanttEpicDetail {
                        epic: epic.clone(),
                        issues: epic_issues,
                        search: String::new(),
                        search_active: false,
                        scroll: 0,
                    });
                }
            }
            _ => {}
        }
    }

    // ── Sprint history ─────────────────────────────────────────────────────────

    /// Load all sprints and issues for the currently selected one.
    pub fn load_sprint_history(&mut self) {
        self.history_sprints = self.db.get_all_sprints().unwrap_or_default();
        self.history_sel = self.history_sel.min(self.history_sprints.len().saturating_sub(1));
        self.load_history_issues();
    }

    fn load_history_issues(&mut self) {
        if let Some(sprint) = self.history_sprints.get(self.history_sel) {
            self.history_issues = self.db.get_sprint_issues(sprint.id).unwrap_or_default();
        } else {
            self.history_issues = Vec::new();
        }
    }

    fn handle_history_key(&mut self, key: KeyEvent) {
        let len = self.history_sprints.len();
        if len == 0 { return; }
        match key.code {
            KeyCode::Char('j') => {
                if self.history_sel + 1 < len {
                    self.history_sel += 1;
                    self.load_history_issues();
                }
            }
            KeyCode::Char('k') => {
                if self.history_sel > 0 {
                    self.history_sel -= 1;
                    self.load_history_issues();
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(sprint) = self.history_sprints.get(self.history_sel) {
                    self.popup = Some(Popup::SprintManager(SprintForm::from_sprint(sprint)));
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
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

            Some(Popup::Help) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                    self.popup = None;
                }
            }

            Some(Popup::ConfirmDelete(id, _)) => {
                let id = *id;
                match key.code {
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        self.popup = None;
                        self.push_undo(UndoAction::SoftDelete { issue_id: id });
                        if let Err(e) = self.db.delete_issue(id) {
                            self.set_status(format!("Error: {e}"));
                        } else {
                            let _ = self.reload();
                            self.flush_display();
                            self.set_status("Issue moved to trash.");
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
                            // Reload sprint history
                            self.history_sprints = self.db.get_all_sprints().unwrap_or_default();
                            self.history_sel = self.history_sel.min(self.history_sprints.len().saturating_sub(1));
                            self.history_issues = Vec::new();
                            if let Some(sprint) = self.history_sprints.get(self.history_sel) {
                                self.history_issues = self.db.get_sprint_issues(sprint.id).unwrap_or_default();
                            }
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
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.popup = None;
                    }
                    KeyCode::Char('j') => {
                        let len = items.len();
                        if len > 0 && *sel + 1 < len { *sel += 1; }
                    }
                    KeyCode::Char('k') => {
                        if *sel > 0 { *sel -= 1; }
                    }
                    KeyCode::Char('r') => {
                        if let Some(issue) = items.get(*sel).cloned() {
                            self.popup = None;
                            if let Err(e) = self.db.restore_issue(issue.id) {
                                self.set_status(format!("Error: {e}"));
                            } else {
                                let _ = self.reload();
                                self.flush_display();
                                self.set_status(format!("\"{}\" restored.", issue.title));
                            }
                        }
                    }
                    KeyCode::Char('D') => {
                        if let Some(issue) = items.get(*sel).cloned() {
                            let new_sel = sel.saturating_sub(1);
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

            Some(Popup::GanttEpicDetail { search, search_active, scroll, issues, .. }) => {
                let issue_count = issues.len();
                match key.code {
                    KeyCode::Esc => {
                        if *search_active {
                            *search_active = false;
                            search.clear();
                            *scroll = 0;
                        } else {
                            self.popup = None;
                        }
                    }
                    KeyCode::Char('q') if !*search_active => {
                        self.popup = None;
                    }
                    KeyCode::Char('/') if !*search_active => {
                        *search_active = true;
                    }
                    KeyCode::Enter if *search_active => {
                        *search_active = false;
                    }
                    KeyCode::Backspace if *search_active => {
                        search.pop();
                        *scroll = 0;
                    }
                    KeyCode::Char(c) if *search_active => {
                        search.push(c);
                        *scroll = 0;
                    }
                    KeyCode::Char('j') => {
                        if issue_count > 0 && *scroll + 1 < issue_count {
                            *scroll += 1;
                        }
                    }
                    KeyCode::Char('k') => {
                        *scroll = scroll.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }
        false
    }

    fn handle_issue_form_key(&mut self, key: KeyEvent) {
        let popup = match &mut self.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
            _ => return,
        };

        // ── Subtask-list navigation (when focus is in the subtask section) ──────
        if popup.in_subtask_list {
            return self.handle_subtask_list_key(key);
        }

        // Ctrl+S saves the form from any field (including description)
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(err) = popup.validate() {
                popup.error = Some(err);
                return;
            }
            // Reuse Enter logic by temporarily pretending we pressed Enter without a dropdown open
            let was_epic_open = popup.epic_dropdown_open;
            let was_due_open = popup.due_date_dropdown_open;
            popup.epic_dropdown_open = false;
            popup.due_date_dropdown_open = false;
            // save by synthesising a regular Enter key path — but popup.focused_field must not be 5
            // so the newline guard doesn't fire. We'll temporarily set it to 0.
            let saved_field = popup.focused_field;
            popup.focused_field = 0;
            self.handle_issue_form_key(crossterm::event::KeyEvent::new(
                KeyCode::Enter, KeyModifiers::NONE,
            ));
            // Restore in case save was blocked by validation (popup still open)
            if let Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) = &mut self.popup {
                f.focused_field = saved_field;
                f.epic_dropdown_open = was_epic_open;
                f.due_date_dropdown_open = was_due_open;
            }
            return;
        }

        match key.code {
            KeyCode::Esc => {
                if popup.epic_dropdown_open {
                    popup.epic_dropdown_open = false;
                    return;
                }
                if popup.due_date_dropdown_open {
                    popup.due_date_dropdown_open = false;
                    return;
                }
                if popup.status_dropdown_open {
                    popup.status_dropdown_open = false;
                    return;
                }
                self.popup = None;
                return;
            }
            KeyCode::Tab => {
                // Close status dropdown on Tab (commits current selection)
                if popup.focused_field == 3 && popup.status_dropdown_open {
                    let popup2 = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    popup2.status_idx = popup2.status_dropdown_sel;
                    popup2.status_dropdown_open = false;
                    let next = popup2.focused_field + 1;
                    if next >= IssueForm::field_count() {
                        popup2.in_subtask_list = true;
                        popup2.subtask_sel = 0;
                        popup2.subtask_editing = false;
                    } else {
                        popup2.focused_field = next;
                        if popup2.focused_field == 4 {
                            popup2.due_date_dropdown_open = true;
                            popup2.due_date_dropdown_sel = 0;
                        }
                    }
                    return;
                }
                // If epic dropdown is open, commit selected item then advance
                if popup.focused_field == 1 && popup.epic_dropdown_open {
                    let q = popup.epic.to_lowercase();
                    let sorted: Vec<String> = self.epics_cache.iter()
                        .filter(|e| e.to_lowercase().contains(&q))
                        .cloned()
                        .collect();
                    let popup2 = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    let sel = popup2.epic_dropdown_sel.min(sorted.len().saturating_sub(1));
                    if let Some(chosen) = sorted.into_iter().nth(sel) {
                        popup2.epic = chosen;
                    }
                    popup2.epic_dropdown_open = false;
                    popup2.epic_dropdown_sel = 0;
                    let next = popup2.focused_field + 1;
                    if next >= IssueForm::field_count() {
                        popup2.in_subtask_list = true;
                        popup2.subtask_sel = 0;
                        popup2.subtask_editing = false;
                    } else {
                        popup2.focused_field = next;
                        if popup2.focused_field == 4 {
                            popup2.due_date_dropdown_open = true;
                            popup2.due_date_dropdown_sel = 0;
                        }
                    }
                    return;
                }
                let popup2 = match &mut self.popup {
                    Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                    _ => return,
                };
                popup2.epic_dropdown_open = false;
                popup2.due_date_dropdown_open = false;
                let next = popup2.focused_field + 1;
                if next >= IssueForm::field_count() {
                    // Move focus into subtask list
                    popup2.in_subtask_list = true;
                    popup2.subtask_sel = 0;
                    popup2.subtask_editing = false;
                } else {
                    popup2.focused_field = next % IssueForm::field_count();
                    // Auto-open due-date dropdown when focusing the due field
                    if popup2.focused_field == 4 {
                        popup2.due_date_dropdown_open = true;
                        popup2.due_date_dropdown_sel = 0;
                    }
                }
                return;
            }
            KeyCode::BackTab => {
                popup.epic_dropdown_open = false;
                popup.due_date_dropdown_open = false;
                if popup.focused_field == 0 {
                    // At first field — shift-tab wraps into the subtask list
                    popup.in_subtask_list = true;
                    popup.subtask_sel = 0;
                    popup.subtask_editing = false;
                } else {
                    popup.focused_field -= 1;
                    // Auto-open due-date dropdown when focusing the due field
                    if popup.focused_field == 4 {
                        popup.due_date_dropdown_open = true;
                        popup.due_date_dropdown_sel = 0;
                    }
                }
                return;
            }
            KeyCode::Enter => {
                // If dropdown open, commit selected epic and close instead of saving form
                if popup.epic_dropdown_open {
                    let q = popup.epic.to_lowercase();
                    let sorted: Vec<String> = self.epics_cache.iter()
                        .filter(|e| e.to_lowercase().contains(&q))
                        .cloned()
                        .collect();
                    let sel = popup.epic_dropdown_sel.min(sorted.len().saturating_sub(1));
                    let popup = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    if let Some(chosen) = sorted.into_iter().nth(sel) {
                        popup.epic = chosen;
                    }
                    popup.epic_dropdown_open = false;
                    popup.epic_dropdown_sel = 0;
                    return;
                }

                if popup.due_date_dropdown_open {
                    let q = popup.due_date.to_lowercase();
                    let today = Local::now().format("%Y-%m-%d").to_string();
                    let dates: std::collections::HashSet<String> = self
                        .issues
                        .iter()
                        .filter_map(|i| i.due_date.map(|d| d.format("%Y-%m-%d").to_string()))
                        .collect();
                    let mut dates: Vec<String> = dates.into_iter().collect();
                    dates.retain(|d| d != &today);
                    dates.sort();
                    let mut matches = vec![today];
                    matches.extend(dates);
                    matches.retain(|d| d.contains(&q));
                    let sel = popup.due_date_dropdown_sel.min(matches.len().saturating_sub(1));
                    let popup = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    if let Some(chosen) = matches.into_iter().nth(sel) {
                        popup.due_date = chosen;
                    }
                    popup.due_date_dropdown_open = false;
                    popup.due_date_dropdown_sel = 0;
                    return;
                }
                // Submit form – need to extract form data first
                if let Some(err) = popup.validate() {
                    popup.error = Some(err);
                    return;
                }
                // Extract needed data before borrow ends
                let editing_id = popup.editing_id;
                let new_issue_sprint_id = popup.sprint_id;
                let title = popup.title.trim().to_string();
                let sp = popup.story_points.parse::<f64>().unwrap_or(1.0);
                let epic = popup.epic.trim().to_string();
                let status = Status::from_index(popup.status_idx);
                let due_date = if popup.due_date.is_empty() {
                    None
                } else {
                    NaiveDate::parse_from_str(&popup.due_date, "%Y-%m-%d").ok()
                };
                let desc: Option<String> = if popup.description.is_empty() {
                    None
                } else {
                    Some(popup.description.clone())
                };

                // Capture subtask data before closing popup
                let subtask_drafts = {
                    let popup = match &self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    popup.subtasks.clone()
                };

                // ── Snapshot for undo ─────────────────────────────────────────
                // For edits: snapshot the current issue + its existing subtasks.
                // For creates: we'll record the new id after insert (stored in new_subtask_ids[0]).
                let undo_snapshot: Option<UndoAction> = if let Some(id) = editing_id {
                    let before = self.db.get_issue(id).ok();
                    let subtasks_before: Vec<Issue> = self.issues.iter()
                        .filter(|i| i.parent_id == Some(id))
                        .cloned()
                        .collect();
                    // IDs of subtasks that exist now (any not in this list after save = newly created)
                    Some(UndoAction::IssueSnapshot {
                        before,
                        subtasks_before,
                        new_subtask_ids: Vec::new(), // filled after save
                    })
                } else {
                    None // create: handled after we have the new id
                };

                self.popup = None;

                // Returns the issue id (new or existing) so we can attach subtasks
                let id_result: Result<i64, _> = if let Some(id) = editing_id {
                    self.db.update_issue(id, &title, sp, &epic, &status, due_date, desc.as_deref(), None)
                        .map(|_| id)
                } else {
                    self.db.create_issue(&title, sp, &epic, &status, due_date, desc.as_deref())
                };

                // If this is a new issue with a pre-assigned sprint, add it now
                if editing_id.is_none() {
                    if let (Ok(new_id), Some(sid)) = (&id_result, new_issue_sprint_id) {
                        let _ = self.db.set_issue_sprint(*new_id, Some(sid));
                    }
                }

                match id_result {
                    Err(e) => self.set_status(format!("Error: {e}")),
                    Ok(parent_id) => {
                        let mut newly_created_subtask_ids: Vec<i64> = Vec::new();

                        // Persist subtask drafts (works for both new and edit)
                        for draft in &subtask_drafts {
                            if draft.deleted {
                                if let Some(id) = draft.id {
                                    let _ = self.db.delete_issue(id);
                                }
                            } else if let Some(id) = draft.id {
                                let st = Status::from_index(draft.status_idx);
                                let _ = self.db.update_subtask(id, &draft.title, &st);
                            } else if !draft.title.trim().is_empty() {
                                let st = Status::from_index(draft.status_idx);
                                if let Ok(new_id) = self.db.create_subtask(&draft.title, parent_id, &st) {
                                    newly_created_subtask_ids.push(new_id);
                                }
                            }
                        }
                        // Recompute parent status whenever subtasks exist
                        if !subtask_drafts.is_empty() {
                            let _ = self.db.update_parent_status_from_children(parent_id);
                        }

                        // Push undo entry now that we have all the info
                        if let Some(UndoAction::IssueSnapshot { before, subtasks_before, .. }) = undo_snapshot {
                            self.push_undo(UndoAction::IssueSnapshot {
                                before,
                                subtasks_before,
                                new_subtask_ids: newly_created_subtask_ids,
                            });
                        } else {
                            // New issue created: store its id so undo can soft-delete it
                            self.push_undo(UndoAction::IssueSnapshot {
                                before: None,
                                subtasks_before: Vec::new(),
                                new_subtask_ids: vec![parent_id],
                            });
                        }

                        let _ = self.reload();
                        // Full edit: refresh display snapshot so the saved state
                        // is immediately reflected (user consciously committed it)
                        self.flush_display();
                        self.set_status(if editing_id.is_some() {
                            "Issue updated."
                        } else {
                            "Issue created."
                        });
                    }
                }
                return;
            }
            _ => {}
        }

        // Route character input to the active field
        let popup = match &mut self.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
            _ => return,
        };
        popup.error = None;

        if popup.focused_field == 3 {
            if popup.status_dropdown_open {
                // Dropdown navigation
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if popup.status_dropdown_sel < 2 { popup.status_dropdown_sel += 1; }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if popup.status_dropdown_sel > 0 { popup.status_dropdown_sel -= 1; }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        popup.status_idx = popup.status_dropdown_sel;
                        popup.status_dropdown_open = false;
                    }
                    KeyCode::Esc => {
                        popup.status_dropdown_open = false;
                    }
                    _ => {}
                }
                return;
            }
            // Status field not open: any printable key or Enter/Space opens the dropdown
            match key.code {
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('[') | KeyCode::Char(']') => {
                    popup.status_dropdown_sel = popup.status_idx;
                    popup.status_dropdown_open = true;
                }
                _ => {}
            }
        } else if popup.focused_field == 1 && popup.epic_dropdown_open {
            // Epic dropdown navigation — commit selection on Enter/Tab
            match key.code {
                KeyCode::Char('j') => {
                    popup.epic_dropdown_sel = popup.epic_dropdown_sel.saturating_add(1);
                }
                KeyCode::Char('k') => {
                    popup.epic_dropdown_sel = popup.epic_dropdown_sel.saturating_sub(1);
                }
                KeyCode::Tab => {
                    // Commit then advance field
                    let q = popup.epic.to_lowercase();
                    let sorted: Vec<String> = self.epics_cache.iter()
                        .filter(|e| e.to_lowercase().contains(&q))
                        .cloned()
                        .collect();
                    let sel = popup.epic_dropdown_sel.min(sorted.len().saturating_sub(1));
                    let popup = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    if let Some(chosen) = sorted.into_iter().nth(sel) {
                        popup.epic = chosen;
                    }
                    popup.epic_dropdown_open = false;
                    popup.epic_dropdown_sel = 0;
                    popup.focused_field = (popup.focused_field + 1) % IssueForm::field_count();
                }
                KeyCode::Esc => {
                    popup.epic_dropdown_open = false;
                }
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(&mut popup.epic);
                    } else {
                        popup.epic.pop();
                    }
                    popup.epic_dropdown_open = !popup.epic.is_empty();
                    popup.epic_dropdown_sel = 0;
                }
                KeyCode::Char(c) => {
                    popup.epic.push(c);
                    popup.epic_dropdown_open = true;
                    popup.epic_dropdown_sel = 0;
                }
                _ => {}
            }
        } else if popup.focused_field == 4 && popup.due_date_dropdown_open {
            // Due-date dropdown navigation
            match key.code {
                KeyCode::Char('j') => {
                    popup.due_date_dropdown_sel = popup.due_date_dropdown_sel.saturating_add(1);
                }
                KeyCode::Char('k') => {
                    popup.due_date_dropdown_sel = popup.due_date_dropdown_sel.saturating_sub(1);
                }
                KeyCode::Tab => {
                    // Commit then advance field
                    let q = popup.due_date.to_lowercase();
                    let today = Local::now().format("%Y-%m-%d").to_string();
                    let dates: std::collections::HashSet<String> = self
                        .issues
                        .iter()
                        .filter_map(|i| i.due_date.map(|d| d.format("%Y-%m-%d").to_string()))
                        .collect();
                    let mut dates: Vec<String> = dates.into_iter().collect();
                    dates.retain(|d| d != &today);
                    dates.sort();
                    let mut matches = vec![today];
                    matches.extend(dates);
                    matches.retain(|d| d.contains(&q));
                    let sel = popup.due_date_dropdown_sel.min(matches.len().saturating_sub(1));
                    let popup = match &mut self.popup {
                        Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
                        _ => return,
                    };
                    if let Some(chosen) = matches.into_iter().nth(sel) {
                        popup.due_date = chosen;
                    }
                    popup.due_date_dropdown_open = false;
                    popup.due_date_dropdown_sel = 0;
                    popup.focused_field = (popup.focused_field + 1) % IssueForm::field_count();
                }
                KeyCode::Esc => {
                    popup.due_date_dropdown_open = false;
                }
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(&mut popup.due_date);
                    } else {
                        popup.due_date.pop();
                    }
                    // Keep dropdown open — it always shows when field 4 is focused
                    popup.due_date_dropdown_open = true;
                    popup.due_date_dropdown_sel = 0;
                }
                KeyCode::Char(c) => {
                    popup.due_date.push(c);
                    popup.due_date_dropdown_open = true;
                    popup.due_date_dropdown_sel = 0;
                }
                _ => {}
            }
        } else if let Some(field) = popup.active_text_field() {
            match key.code {
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(field);
                    } else {
                        field.pop();
                    }
                    // Open dropdown if we're on the epic field
                    if popup.focused_field == 1 {
                        popup.epic_dropdown_open = !popup.epic.is_empty();
                        popup.epic_dropdown_sel = 0;
                    }
                    if popup.focused_field == 4 {
                        // Keep dropdown open — it always shows when field 4 is focused
                        popup.due_date_dropdown_open = true;
                        popup.due_date_dropdown_sel = 0;
                    }
                }
                                 KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                        delete_word(field);
                        if popup.focused_field == 1 {
                            popup.epic_dropdown_open = !popup.epic.is_empty();
                            popup.epic_dropdown_sel = 0;
                        }
                        if popup.focused_field == 4 {
                            popup.due_date_dropdown_open = true;
                            popup.due_date_dropdown_sel = 0;
                        }
                    } else {
                        field.push(c);
                        // Open epic dropdown on any char input in epic field
                        if popup.focused_field == 1 {
                            popup.epic_dropdown_open = true;
                            popup.epic_dropdown_sel = 0;
                        }
                        // Open due-date dropdown on any char input in due-date field
                        if popup.focused_field == 4 {
                            popup.due_date_dropdown_open = true;
                            popup.due_date_dropdown_sel = 0;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_subtask_list_key(&mut self, key: KeyEvent) {
        let popup = match &mut self.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
            _ => return,
        };

        if popup.subtask_editing {
            // Title-edit mode for selected subtask
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    popup.subtask_editing = false;
                }
                KeyCode::Tab => {
                    popup.subtask_editing = false;
                    if !popup.subtasks.is_empty() {
                        let next = popup.subtask_sel + 1;
                        if next < popup.subtasks.len() {
                            popup.subtask_sel = next;
                            popup.subtask_editing = true;
                        } else {
                            // Wrap back to top fields
                            popup.in_subtask_list = false;
                            popup.focused_field = 0;
                        }
                    }
                }
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        if let Some(st) = popup.subtasks.get_mut(popup.subtask_sel) {
                            delete_word(&mut st.title);
                        }
                    } else if let Some(st) = popup.subtasks.get_mut(popup.subtask_sel) {
                        st.title.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                        if let Some(st) = popup.subtasks.get_mut(popup.subtask_sel) {
                            delete_word(&mut st.title);
                        }
                    } else if let Some(st) = popup.subtasks.get_mut(popup.subtask_sel) {
                        st.title.push(c);
                    }
                }
                _ => {}
            }
            return;
        }

        // Browse mode
        match key.code {
            KeyCode::Esc => {
                popup.in_subtask_list = false;
                popup.focused_field = IssueForm::field_count() - 1;
                return;
            }
            KeyCode::Tab => {
                // Back to top fields
                popup.in_subtask_list = false;
                popup.focused_field = 0;
                return;
            }
            KeyCode::BackTab => {
                popup.in_subtask_list = false;
                popup.focused_field = IssueForm::field_count() - 1;
                return;
            }
            KeyCode::Enter => {
                // Submit form (same as main form Enter)
                if let Some(err) = popup.validate() {
                    popup.error = Some(err);
                    return;
                }
                // Fall through to form-submit by exiting subtask list and re-handling Enter
                popup.in_subtask_list = false;
                self.handle_issue_form_key(key);
                return;
            }
            KeyCode::Char('j') => {
                let visible_len = popup.subtasks.iter().filter(|s| !s.deleted).count();
                if visible_len > 0 && popup.subtask_sel + 1 < visible_len {
                    popup.subtask_sel += 1;
                }
                return;
            }
            KeyCode::Char('k') => {
                if popup.subtask_sel > 0 {
                    popup.subtask_sel -= 1;
                }
                return;
            }
            KeyCode::Char('e') | KeyCode::Char('i') => {
                if !popup.subtasks.is_empty() {
                    popup.subtask_editing = true;
                }
                return;
            }
            KeyCode::Char(']') => {
                // Advance subtask status
                let vis_idx = popup.subtask_sel;
                if let Some(st) = popup.subtasks.iter_mut().filter(|s| !s.deleted).nth(vis_idx) {
                    if st.status_idx < 2 {
                        st.status_idx += 1;
                    }
                }
                return;
            }
            KeyCode::Char('[') => {
                // Regress subtask status
                let vis_idx = popup.subtask_sel;
                if let Some(st) = popup.subtasks.iter_mut().filter(|s| !s.deleted).nth(vis_idx) {
                    if st.status_idx > 0 {
                        st.status_idx -= 1;
                    }
                }
                return;
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                // Mark selected subtask for deletion
                let vis_idx = popup.subtask_sel;
                let target = popup.subtasks.iter_mut().filter(|s| !s.deleted).nth(vis_idx);
                if let Some(st) = target {
                    st.deleted = true;
                    let visible_len = popup.subtasks.iter().filter(|s| !s.deleted).count();
                    if popup.subtask_sel >= visible_len && popup.subtask_sel > 0 {
                        popup.subtask_sel -= 1;
                    }
                }
                return;
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Add new subtask
                popup.subtasks.push(SubtaskDraft {
                    id: None,
                    title: String::new(),
                    status_idx: 0,
                    deleted: false,
                });
                let visible_len = popup.subtasks.iter().filter(|s| !s.deleted).count();
                popup.subtask_sel = visible_len.saturating_sub(1);
                popup.subtask_editing = true;
                return;
            }
            _ => {}
        }
    }

    fn handle_sprint_form_key(&mut self, key: KeyEvent) {
        let popup = match &mut self.popup {
            Some(Popup::SprintManager(f)) => f,
            _ => return,
        };

        match key.code {
            KeyCode::Esc => {
                self.popup = None;
                return;
            }
            KeyCode::Tab => {
                popup.focused_field = (popup.focused_field + 1) % SprintForm::field_count();
                return;
            }
            KeyCode::BackTab => {
                popup.focused_field = (popup.focused_field + SprintForm::field_count() - 1)
                    % SprintForm::field_count();
                return;
            }
            KeyCode::Enter => {
                if let Some(err) = popup.validate() {
                    popup.error = Some(err);
                    return;
                }
                let editing_id = popup.editing_id;
                let name = popup.name.trim().to_string();
                let start = NaiveDate::parse_from_str(&popup.start_date, "%Y-%m-%d").unwrap();
                let end = NaiveDate::parse_from_str(&popup.end_date, "%Y-%m-%d").unwrap();
                let active = popup.is_active;

                self.popup = None;

                // If activating a new sprint, bump carry_count for the old sprint's
                // incomplete issues before we deactivate it.
                if active {
                    if let Some(ref old_sprint) = self.active_sprint {
                        // Don't bump for the sprint being edited (it's staying active)
                        if editing_id != Some(old_sprint.id) {
                            let _ = self.db.bump_carry_count_for_sprint(old_sprint.id);
                        }
                    }
                }

                let result = if let Some(id) = editing_id {
                    self.db.update_sprint(id, &name, start, end, active)
                } else {
                    self.db.create_sprint(&name, start, end, active).map(|_| ())
                };

                match result {
                    Err(e) => self.set_status(format!("Error: {e}")),
                    Ok(_) => {
                        let _ = self.reload();
                        self.flush_display();
                        // Refresh history list if we're in that view
                        if self.view == View::SprintHistory {
                            self.load_sprint_history();
                        }
                        self.set_status("Sprint saved.");
                    }
                }
                return;
            }
            _ => {}
        }

        let popup = match &mut self.popup {
            Some(Popup::SprintManager(f)) => f,
            _ => return,
        };
        popup.error = None;

        if popup.focused_field == 3 {
            // Boolean toggle
            match key.code {
                KeyCode::Char(' ') | KeyCode::Char('h') | KeyCode::Char('l') => {
                    popup.is_active = !popup.is_active;
                }
                _ => {}
            }
        } else if let Some(field) = popup.active_text_field() {
            match key.code {
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        delete_word(field);
                    } else {
                        field.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                        delete_word(field);
                    } else {
                        field.push(c);
                    }
                }
                _ => {}
            }
        }
    }

    // ── List state for ratatui ─────────────────────────────────────────────────

    pub fn backlog_list_state(&self) -> ListState {
        let mut state = ListState::default();
        state.select(Some(self.backlog_sel));
        state
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
