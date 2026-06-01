use anyhow::Result;
use chrono::{Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::db::Db;
use crate::models::{Issue, Sprint, Status};

// ── View ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Backlog,
    Kanban,
    Gantt,
}

// ── Backlog display items ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BacklogItem {
    SprintHeader(Sprint),
    SprintFooter,
    BacklogHeader,
    Issue(Issue, bool), // (issue, is_in_sprint)
}

// ── Issue form ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IssueForm {
    pub editing_id: Option<i64>,
    pub title: String,
    pub story_points: String,
    pub epic: String,
    pub status_idx: usize,
    pub due_date: String,
    pub description: String,
    pub focused_field: usize, // 0=title 1=sp 2=epic 3=status 4=due 5=desc
    pub error: Option<String>,
}

impl IssueForm {
    pub fn new() -> Self {
        IssueForm {
            editing_id: None,
            title: String::new(),
            story_points: String::from("1"),
            epic: String::new(),
            status_idx: 0,
            due_date: String::new(),
            description: String::new(),
            focused_field: 0,
            error: None,
        }
    }

    pub fn from_issue(issue: &Issue) -> Self {
        IssueForm {
            editing_id: Some(issue.id),
            title: issue.title.clone(),
            story_points: issue.story_points.to_string(),
            epic: issue.epic.clone(),
            status_idx: issue.status.index(),
            due_date: issue.due_date_str(),
            description: issue.description.clone().unwrap_or_default(),
            focused_field: 0,
            error: None,
        }
    }

    /// Returns a mutable reference to the string buffer of the currently focused text field,
    /// or None if the focused field is not a text field (e.g. status).
    pub fn active_text_field(&mut self) -> Option<&mut String> {
        match self.focused_field {
            0 => Some(&mut self.title),
            1 => Some(&mut self.story_points),
            2 => Some(&mut self.epic),
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
        let end = today + chrono::Duration::days(6);
        SprintForm {
            editing_id: None,
            name: String::from("Sprint"),
            start_date: today.format("%Y-%m-%d").to_string(),
            end_date: end.format("%Y-%m-%d").to_string(),
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
    Help,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub view: View,
    pub db: Db,
    pub issues: Vec<Issue>,
    pub active_sprint: Option<Sprint>,
    /// Index into the flat display list produced by `backlog_items()`
    pub backlog_sel: usize,
    /// Kanban: current column (0=Todo 1=InProgress 2=Done)
    pub kanban_col: usize,
    /// Kanban: selected row within each column
    pub kanban_rows: [usize; 3],
    /// Gantt scroll offset (rows)
    pub gantt_scroll: usize,
    pub popup: Option<Popup>,
    pub status_msg: Option<String>,
}

impl App {
    pub fn new(db: Db) -> Result<Self> {
        let issues = db.get_all_issues()?;
        let active_sprint = db.get_active_sprint()?;
        Ok(App {
            view: View::Backlog,
            db,
            issues,
            active_sprint,
            backlog_sel: 0,
            kanban_col: 0,
            kanban_rows: [0, 0, 0],
            gantt_scroll: 0,
            popup: None,
            status_msg: None,
        })
    }

    pub fn reload(&mut self) -> Result<()> {
        self.issues = self.db.get_all_issues()?;
        self.active_sprint = self.db.get_active_sprint()?;
        // Clamp selection to valid range
        let len = self.backlog_items().len();
        if self.backlog_sel >= len && len > 0 {
            self.backlog_sel = len - 1;
        }
        Ok(())
    }

    // ── Derived data ───────────────────────────────────────────────────────────

    /// Build the flat display list for the backlog view.
    pub fn backlog_items(&self) -> Vec<BacklogItem> {
        let mut items: Vec<BacklogItem> = Vec::new();

        if let Some(sprint) = &self.active_sprint {
            let sprint_issues: Vec<Issue> = self
                .issues
                .iter()
                .filter(|i| i.sprint_id == Some(sprint.id))
                .cloned()
                .collect();
            if !sprint_issues.is_empty() {
                items.push(BacklogItem::SprintHeader(sprint.clone()));
                for issue in sprint_issues {
                    items.push(BacklogItem::Issue(issue, true));
                }
                items.push(BacklogItem::SprintFooter);
            }
        }

        let backlog: Vec<Issue> = self
            .issues
            .iter()
            .filter(|i| match &self.active_sprint {
                Some(s) => i.sprint_id != Some(s.id),
                None => true,
            })
            .cloned()
            .collect();

        if !backlog.is_empty() {
            items.push(BacklogItem::BacklogHeader);
            for issue in backlog {
                items.push(BacklogItem::Issue(issue, false));
            }
        }

        items
    }

    /// The issue currently selected in the backlog (if any).
    pub fn selected_issue(&self) -> Option<Issue> {
        let items = self.backlog_items();
        match items.get(self.backlog_sel) {
            Some(BacklogItem::Issue(issue, _)) => Some(issue.clone()),
            _ => None,
        }
    }

    pub fn sprint_issues_by_status(&self, status: &Status) -> Vec<Issue> {
        match &self.active_sprint {
            Some(s) => self
                .issues
                .iter()
                .filter(|i| i.sprint_id == Some(s.id) && &i.status == status)
                .cloned()
                .collect(),
            None => vec![],
        }
    }

    /// Unique epics across all issues, sorted alphabetically.
    pub fn epics(&self) -> Vec<String> {
        let mut epics: Vec<String> = self
            .issues
            .iter()
            .map(|i| i.epic.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        epics.sort();
        epics
    }

    // ── Navigation helpers ─────────────────────────────────────────────────────

    fn is_selectable(item: &BacklogItem) -> bool {
        matches!(item, BacklogItem::Issue(_, _))
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
        let len = self.sprint_issues_by_status(&Status::from_index(self.kanban_col)).len();
        if len == 0 {
            return;
        }
        let row = &mut self.kanban_rows[self.kanban_col];
        if *row + 1 < len {
            *row += 1;
        }
    }

    pub fn kanban_up(&mut self) {
        let row = &mut self.kanban_rows[self.kanban_col];
        if *row > 0 {
            *row -= 1;
        }
    }

    pub fn kanban_selected_issue(&self) -> Option<Issue> {
        let status = Status::from_index(self.kanban_col);
        let issues = self.sprint_issues_by_status(&status);
        let row = self.kanban_rows[self.kanban_col];
        issues.into_iter().nth(row)
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

        // Global keys
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => {
                self.popup = Some(Popup::Help);
                return false;
            }
            KeyCode::Char('1') => {
                self.view = View::Backlog;
                return false;
            }
            KeyCode::Char('2') => {
                self.view = View::Kanban;
                return false;
            }
            KeyCode::Char('3') => {
                self.view = View::Gantt;
                return false;
            }
            _ => {}
        }

        match self.view {
            View::Backlog => self.handle_backlog_key(key),
            View::Kanban => self.handle_kanban_key(key),
            View::Gantt => self.handle_gantt_key(key),
        }

        false
    }

    fn handle_backlog_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.backlog_down(),
            KeyCode::Char('k') | KeyCode::Up => self.backlog_up(),
            KeyCode::Char('g') => self.backlog_sel_to_first_issue(),
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
                self.popup = Some(Popup::NewIssue(IssueForm::new()));
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.selected_issue() {
                    self.popup = Some(Popup::EditIssue(IssueForm::from_issue(&issue)));
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
                        if let Err(e) = self.db.set_issue_sprint(issue.id, new_sprint_id) {
                            self.status_msg = Some(format!("Error: {e}"));
                        } else {
                            let _ = self.reload();
                            self.status_msg = if new_sprint_id.is_some() {
                                Some("Moved to sprint.".into())
                            } else {
                                Some("Moved to backlog.".into())
                            };
                        }
                    } else {
                        self.status_msg = Some("No active sprint. Press S to create one.".into());
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
            _ => {}
        }
    }

    fn handle_kanban_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if self.kanban_col > 0 {
                    self.kanban_col -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.kanban_col < 2 {
                    self.kanban_col += 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.kanban_down(),
            KeyCode::Char('k') | KeyCode::Up => self.kanban_up(),
            KeyCode::Char('>') | KeyCode::Char('.') => {
                if let Some(issue) = self.kanban_selected_issue() {
                    let new_status = issue.status.next();
                    if new_status != issue.status {
                        let _ = self.db.update_issue_status(issue.id, &new_status);
                        let _ = self.reload();
                        // Try to keep selection in the same column
                        let col = new_status.index();
                        self.kanban_col = col;
                        let len = self.sprint_issues_by_status(&new_status).len();
                        if self.kanban_rows[col] >= len && len > 0 {
                            self.kanban_rows[col] = len - 1;
                        }
                    }
                }
            }
            KeyCode::Char('<') | KeyCode::Char(',') => {
                if let Some(issue) = self.kanban_selected_issue() {
                    let new_status = issue.status.prev();
                    if new_status != issue.status {
                        let _ = self.db.update_issue_status(issue.id, &new_status);
                        let _ = self.reload();
                        let col = new_status.index();
                        self.kanban_col = col;
                        let len = self.sprint_issues_by_status(&new_status).len();
                        if self.kanban_rows[col] >= len && len > 0 {
                            self.kanban_rows[col] = len - 1;
                        }
                    }
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(issue) = self.kanban_selected_issue() {
                    self.popup = Some(Popup::EditIssue(IssueForm::from_issue(&issue)));
                }
            }
            _ => {}
        }
    }

    fn handle_gantt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.gantt_scroll = self.gantt_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.gantt_scroll = self.gantt_scroll.saturating_sub(1);
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
                        if let Err(e) = self.db.delete_issue(id) {
                            self.status_msg = Some(format!("Error: {e}"));
                        } else {
                            let _ = self.reload();
                            self.status_msg = Some("Issue deleted.".into());
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.popup = None;
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
        }
        false
    }

    fn handle_issue_form_key(&mut self, key: KeyEvent) {
        let popup = match &mut self.popup {
            Some(Popup::NewIssue(f)) | Some(Popup::EditIssue(f)) => f,
            _ => return,
        };

        match key.code {
            KeyCode::Esc => {
                self.popup = None;
                return;
            }
            KeyCode::Tab => {
                popup.focused_field = (popup.focused_field + 1) % IssueForm::field_count();
                return;
            }
            KeyCode::BackTab => {
                popup.focused_field =
                    (popup.focused_field + IssueForm::field_count() - 1) % IssueForm::field_count();
                return;
            }
            KeyCode::Enter => {
                // Submit form – need to extract form data first
                if let Some(err) = popup.validate() {
                    popup.error = Some(err);
                    return;
                }
                // Extract needed data before borrow ends
                let editing_id = popup.editing_id;
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
                    Some(popup.description.trim().to_string())
                };

                self.popup = None;

                let result = if let Some(id) = editing_id {
                    self.db.update_issue(id, &title, sp, &epic, &status, due_date, desc.as_deref())
                } else {
                    self.db
                        .create_issue(&title, sp, &epic, &status, due_date, desc.as_deref())
                        .map(|_| ())
                };

                match result {
                    Err(e) => self.status_msg = Some(format!("Error: {e}")),
                    Ok(_) => {
                        let _ = self.reload();
                        self.status_msg = Some(if editing_id.is_some() {
                            "Issue updated.".into()
                        } else {
                            "Issue created.".into()
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
            // Status field: h/l or left/right to cycle
            match key.code {
                KeyCode::Char('h') | KeyCode::Left => {
                    if popup.status_idx > 0 {
                        popup.status_idx -= 1;
                    }
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if popup.status_idx < 2 {
                        popup.status_idx += 1;
                    }
                }
                _ => {}
            }
        } else if let Some(field) = popup.active_text_field() {
            match key.code {
                KeyCode::Backspace => {
                    field.pop();
                }
                KeyCode::Char(c) => {
                    field.push(c);
                }
                _ => {}
            }
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

                let result = if let Some(id) = editing_id {
                    self.db.update_sprint(id, &name, start, end, active)
                } else {
                    self.db.create_sprint(&name, start, end, active).map(|_| ())
                };

                match result {
                    Err(e) => self.status_msg = Some(format!("Error: {e}")),
                    Ok(_) => {
                        let _ = self.reload();
                        self.status_msg = Some("Sprint saved.".into());
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
                KeyCode::Char(' ') | KeyCode::Char('h') | KeyCode::Char('l')
                | KeyCode::Left | KeyCode::Right => {
                    popup.is_active = !popup.is_active;
                }
                _ => {}
            }
        } else if let Some(field) = popup.active_text_field() {
            match key.code {
                KeyCode::Backspace => {
                    field.pop();
                }
                KeyCode::Char(c) => {
                    field.push(c);
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
