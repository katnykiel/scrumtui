use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection};

use crate::models::{Issue, Sprint, Status};

pub struct Db {
    conn: Connection,
}

fn parse_dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .unwrap_or_else(|_| Local::now().naive_local())
}

fn now_str() -> String {
    Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string()
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Db { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sprints (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT    NOT NULL,
                start_date TEXT    NOT NULL,
                end_date   TEXT    NOT NULL,
                is_active  INTEGER NOT NULL DEFAULT 0,
                created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
            );

            CREATE TABLE IF NOT EXISTS issues (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                title        TEXT    NOT NULL,
                story_points INTEGER NOT NULL DEFAULT 1,
                epic         TEXT    NOT NULL DEFAULT '',
                status       TEXT    NOT NULL DEFAULT 'TODO',
                due_date     TEXT,
                description  TEXT,
                sprint_id    INTEGER REFERENCES sprints(id) ON DELETE SET NULL,
                created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
                updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
                completed_at TEXT
            );
            ",
        )?;
        Ok(())
    }

    // ── Issues ────────────────────────────────────────────────────────────────

    pub fn get_all_issues(&self) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at
             FROM issues
             ORDER BY (sprint_id IS NULL), id",
        )?;
        let issues = stmt
            .query_map([], |row| {
                let status_str: String = row.get(4)?;
                let due_str: Option<String> = row.get(5)?;
                let completed_str: Option<String> = row.get(10)?;
                Ok(Issue {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    story_points: row.get(2)?,
                    epic: row.get(3)?,
                    status: Status::from_db(&status_str),
                    due_date: due_str.as_deref().and_then(|s| {
                        NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
                    }),
                    description: row.get(6)?,
                    sprint_id: row.get(7)?,
                    created_at: parse_dt(&row.get::<_, String>(8)?),
                    updated_at: parse_dt(&row.get::<_, String>(9)?),
                    completed_at: completed_str.as_deref().map(parse_dt),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(issues)
    }

    pub fn get_issue(&self, id: i64) -> Result<Issue> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at
             FROM issues WHERE id = ?1",
        )?;
        let issue = stmt.query_row(params![id], |row| {
            let status_str: String = row.get(4)?;
            let due_str: Option<String> = row.get(5)?;
            let completed_str: Option<String> = row.get(10)?;
            Ok(Issue {
                id: row.get(0)?,
                title: row.get(1)?,
                story_points: row.get(2)?,
                epic: row.get(3)?,
                status: Status::from_db(&status_str),
                due_date: due_str
                    .as_deref()
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
                description: row.get(6)?,
                sprint_id: row.get(7)?,
                created_at: parse_dt(&row.get::<_, String>(8)?),
                updated_at: parse_dt(&row.get::<_, String>(9)?),
                completed_at: completed_str.as_deref().map(parse_dt),
            })
        })?;
        Ok(issue)
    }

    pub fn create_issue(
        &self,
        title: &str,
        story_points: f64,
        epic: &str,
        status: &Status,
        due_date: Option<NaiveDate>,
        description: Option<&str>,
    ) -> Result<i64> {
        let now = now_str();
        let due_str = due_date.map(|d| d.format("%Y-%m-%d").to_string());
        let completed_at: Option<String> = if status == &Status::Done {
            Some(now.clone())
        } else {
            None
        };
        self.conn.execute(
            "INSERT INTO issues
             (title, story_points, epic, status, due_date, description, created_at, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            params![title, story_points, epic, status.to_db(), due_str, description, now, completed_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_issue(
        &self,
        id: i64,
        title: &str,
        story_points: f64,
        epic: &str,
        status: &Status,
        due_date: Option<NaiveDate>,
        description: Option<&str>,
    ) -> Result<()> {
        let now = now_str();
        let due_str = due_date.map(|d| d.format("%Y-%m-%d").to_string());
        // Preserve existing completed_at if already done, set if newly done, clear if not done
        let prev = self.get_issue(id)?;
        let completed_at: Option<String> = match status {
            Status::Done => {
                if prev.status == Status::Done {
                    prev.completed_at
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                } else {
                    Some(now.clone())
                }
            }
            _ => None,
        };
        self.conn.execute(
            "UPDATE issues
             SET title=?1, story_points=?2, epic=?3, status=?4, due_date=?5,
                 description=?6, updated_at=?7, completed_at=?8
             WHERE id=?9",
            params![title, story_points, epic, status.to_db(), due_str, description, now, completed_at, id],
        )?;
        Ok(())
    }

    pub fn delete_issue(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM issues WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Override completed_at directly – used by seed data to backdate completions.
    pub fn set_completed_at(&self, id: i64, ts: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET completed_at=?1, updated_at=?1 WHERE id=?2",
            params![ts, id],
        )?;
        Ok(())
    }

    pub fn set_issue_sprint(&self, issue_id: i64, sprint_id: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET sprint_id=?1, updated_at=?2 WHERE id=?3",
            params![sprint_id, now_str(), issue_id],
        )?;
        Ok(())
    }

    pub fn update_issue_status(&self, issue_id: i64, status: &Status) -> Result<()> {
        let now = now_str();
        let completed_at: Option<String> = if status == &Status::Done {
            Some(now.clone())
        } else {
            None
        };
        self.conn.execute(
            "UPDATE issues SET status=?1, updated_at=?2, completed_at=?3 WHERE id=?4",
            params![status.to_db(), now, completed_at, issue_id],
        )?;
        Ok(())
    }

    // ── Sprints ───────────────────────────────────────────────────────────────

    pub fn get_active_sprint(&self) -> Result<Option<Sprint>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, start_date, end_date, is_active, created_at
             FROM sprints WHERE is_active = 1 LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Sprint {
                id: row.get(0)?,
                name: row.get(1)?,
                start_date: NaiveDate::parse_from_str(&row.get::<_, String>(2)?, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive()),
                end_date: NaiveDate::parse_from_str(&row.get::<_, String>(3)?, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive()),
                is_active: row.get::<_, i32>(4)? != 0,
                created_at: parse_dt(&row.get::<_, String>(5)?),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn create_sprint(
        &self,
        name: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        is_active: bool,
    ) -> Result<i64> {
        if is_active {
            self.conn.execute("UPDATE sprints SET is_active = 0", [])?;
        }
        self.conn.execute(
            "INSERT INTO sprints (name, start_date, end_date, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                name,
                start_date.format("%Y-%m-%d").to_string(),
                end_date.format("%Y-%m-%d").to_string(),
                is_active as i32,
                now_str(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_sprint(
        &self,
        id: i64,
        name: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        is_active: bool,
    ) -> Result<()> {
        if is_active {
            self.conn
                .execute("UPDATE sprints SET is_active = 0 WHERE id != ?1", params![id])?;
        }
        self.conn.execute(
            "UPDATE sprints SET name=?1, start_date=?2, end_date=?3, is_active=?4 WHERE id=?5",
            params![
                name,
                start_date.format("%Y-%m-%d").to_string(),
                end_date.format("%Y-%m-%d").to_string(),
                is_active as i32,
                id,
            ],
        )?;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
        Ok(count == 0)
    }
}
