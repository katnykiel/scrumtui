use chrono::{NaiveDate, NaiveDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Todo,
    InProgress,
    Done,
}

impl Status {
    pub fn from_db(s: &str) -> Self {
        match s {
            "IN_PROGRESS" => Status::InProgress,
            "DONE" => Status::Done,
            _ => Status::Todo,
        }
    }

    pub fn to_db(&self) -> &'static str {
        match self {
            Status::Todo => "TODO",
            Status::InProgress => "IN_PROGRESS",
            Status::Done => "DONE",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Status::Todo => "TODO",
            Status::InProgress => "IN PROGRESS",
            Status::Done => "DONE",
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            Status::Todo => "TODO",
            Status::InProgress => "IP  ",
            Status::Done => "DONE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Status::Todo => Status::InProgress,
            Status::InProgress => Status::Done,
            Status::Done => Status::Done,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Status::Todo => Status::Todo,
            Status::InProgress => Status::Todo,
            Status::Done => Status::InProgress,
        }
    }

    #[allow(dead_code)]
    pub fn all() -> &'static [Status; 3] {
        &[Status::Todo, Status::InProgress, Status::Done]
    }

    pub fn index(&self) -> usize {
        match self {
            Status::Todo => 0,
            Status::InProgress => 1,
            Status::Done => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Status::InProgress,
            2 => Status::Done,
            _ => Status::Todo,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub id: i64,
    pub title: String,
    pub story_points: f64,
    pub epic: String,
    pub status: Status,
    pub due_date: Option<NaiveDate>,
    pub description: Option<String>,
    pub sprint_id: Option<i64>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    pub parent_id: Option<i64>,
    /// Display rank within the backlog/sprint (lower = higher priority).
    #[allow(dead_code)]
    pub rank: i64,
    /// Number of sprints this issue has been carried over from (not completed).
    pub carry_count: i64,
}

/// Format the elapsed duration between two timestamps as a human-readable string.
/// e.g. "3d 2h", "45m", "< 1m"
pub fn format_duration(start: NaiveDateTime, end: NaiveDateTime) -> String {
    let secs = (end - start).num_seconds().max(0);
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else if mins > 0 {
        format!("{}m", mins)
    } else {
        "< 1m".to_string()
    }
}

/// Display story points without unnecessary trailing zeros.
pub fn format_sp(sp: f64) -> String {
    if sp.fract() == 0.0 {
        format!("{}", sp as i64)
    } else {
        format!("{}", sp)
    }
}

impl Issue {
    pub fn due_date_str(&self) -> String {
        match &self.due_date {
            Some(d) => d.format("%Y-%m-%d").to_string(),
            None => String::new(),
        }
    }

    /// Returns true if this issue is a subtask (has a parent).
    pub fn is_subtask(&self) -> bool {
        self.parent_id.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct Sprint {
    pub id: i64,
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
}

impl Sprint {
    /// Length in days, counting both ends. Sprints recorded with start == end (an
    /// artifact of never setting an end date) count as a Monday–Friday week.
    pub fn duration_days(&self) -> i64 {
        let raw = (self.end_date - self.start_date).num_days() + 1;
        if raw <= 1 { 5 } else { raw }
    }

    /// Issues that join on or before this day count as planned scope; later ones are
    /// scope creep. It's the start date, or the day the sprint was set up if that was
    /// later (planning often happens a day into the week).
    pub fn planning_day(&self) -> NaiveDate {
        self.start_date.max(self.created_at.date())
    }
}

/// What became of an issue in one sprint it was part of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Finished while this was its sprint.
    Done,
    /// Still assigned to this sprint and not finished.
    Open,
    /// Moved on to a later sprint unfinished (carry-forward).
    Carried,
    /// Taken out of the sprint by hand while it was running.
    Removed,
}

/// One issue's membership in one sprint, as recorded in `issue_sprint_log`.
#[derive(Debug, Clone)]
pub struct SprintEntry {
    pub issue: Issue,
    /// When the issue joined the sprint (None for rows logged before 2.3).
    pub added_at: Option<NaiveDateTime>,
    pub removed_at: Option<NaiveDateTime>,
    pub outcome: Outcome,
}

impl SprintEntry {
    /// When the issue joined the sprint, falling back to its creation time for old rows
    /// (or rows logged long after the sprint, e.g. by a Jira import).
    pub fn joined_at(&self, sprint: &Sprint) -> NaiveDateTime {
        self.added_at
            .filter(|a| a.date() <= sprint.end_date)
            .unwrap_or(self.issue.created_at)
    }

    /// Whether the issue joined after planning, i.e. counts as scope creep.
    pub fn added_mid_sprint(&self, sprint: &Sprint) -> bool {
        self.joined_at(sprint).date() > sprint.planning_day()
    }
}
