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
    pub parent_id: Option<i64>,
    /// Display rank within the backlog/sprint (lower = higher priority).
    #[allow(dead_code)]
    pub rank: i64,
    /// Number of sprints this issue has been carried over from (not completed).
    pub carry_count: i64,
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
    #[allow(dead_code)]
    pub created_at: NaiveDateTime,
}
