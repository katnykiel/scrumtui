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
        // Schema migrations — ignore errors if column already exists
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN parent_id INTEGER;",
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN deleted_at TEXT;",
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN rank INTEGER NOT NULL DEFAULT 0;",
        );
        // Back-fill rank for existing rows so each issue gets a unique rank
        // equal to its rowid (no-op if already set).
        let _ = self.conn.execute_batch(
            "UPDATE issues SET rank = id WHERE rank = 0;",
        );
        // Settings table for persisting UI preferences
        let _ = self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        );
        Ok(())
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT value FROM settings WHERE key = ?1",
        )?;
        let result = stmt.query_row(params![key], |row| row.get(0));
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ── Issues ────────────────────────────────────────────────────────────────

    pub fn get_all_issues(&self) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at, parent_id, rank
             FROM issues
             WHERE deleted_at IS NULL
             ORDER BY (sprint_id IS NULL),
                      parent_id IS NOT NULL,
                      updated_at DESC,
                      id DESC",
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
                    parent_id: row.get(11)?,
                    rank: row.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(issues)
    }

    pub fn get_issue(&self, id: i64) -> Result<Issue> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at, parent_id, rank
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
                parent_id: row.get(11)?,
                rank: row.get(12)?,
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
        self.create_issue_full(
            title, story_points, epic, status, due_date, description,
            None, &now_str(), &now_str(), None,
        )
    }

    /// Full insert with explicit timestamps and optional parent. Used by import.
    pub fn create_issue_full(
        &self,
        title: &str,
        story_points: f64,
        epic: &str,
        status: &Status,
        due_date: Option<NaiveDate>,
        description: Option<&str>,
        parent_id: Option<i64>,
        created_at: &str,
        updated_at: &str,
        completed_at: Option<&str>,
    ) -> Result<i64> {
        let due_str = due_date.map(|d| d.format("%Y-%m-%d").to_string());
        // Assign rank = max(rank) + 1 so new issues go to the bottom
        let next_rank: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(rank), 0) + 1 FROM issues", [], |r| r.get(0)
        ).unwrap_or(1);
        self.conn.execute(
            "INSERT INTO issues
             (title, story_points, epic, status, due_date, description, parent_id,
              created_at, updated_at, completed_at, rank)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                title, story_points, epic, status.to_db(), due_str, description,
                parent_id, created_at, updated_at, completed_at, next_rank
            ],
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
        parent_id: Option<i64>,
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
                 description=?6, updated_at=?7, completed_at=?8, parent_id=?9
             WHERE id=?10",
            params![title, story_points, epic, status.to_db(), due_str, description, now, completed_at, parent_id, id],
        )?;
        Ok(())
    }

    /// Soft-delete an issue (moves to trash). Subtasks are soft-deleted too.
    pub fn delete_issue(&self, id: i64) -> Result<()> {
        let now = now_str();
        self.conn.execute(
            "UPDATE issues SET deleted_at=?1 WHERE parent_id=?2",
            params![now, id],
        )?;
        self.conn.execute(
            "UPDATE issues SET deleted_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Permanently delete an issue and its subtasks from the database.
    pub fn purge_issue(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM issues WHERE parent_id=?1", params![id])?;
        self.conn.execute("DELETE FROM issues WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Restore a soft-deleted issue (and its subtasks) from the trash.
    pub fn restore_issue(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET deleted_at=NULL WHERE parent_id=?1",
            params![id],
        )?;
        self.conn.execute(
            "UPDATE issues SET deleted_at=NULL WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    /// Return all soft-deleted top-level issues (trash), newest first.
    pub fn get_trash(&self) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at, parent_id, rank
             FROM issues
             WHERE deleted_at IS NOT NULL AND parent_id IS NULL
             ORDER BY deleted_at DESC",
        )?;
        let items = stmt
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
                        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
                    }),
                    description: row.get(6)?,
                    sprint_id: row.get(7)?,
                    created_at: parse_dt(&row.get::<_, String>(8)?),
                    updated_at: parse_dt(&row.get::<_, String>(9)?),
                    completed_at: completed_str.as_deref().map(parse_dt),
                    parent_id: row.get(11)?,
                    rank: row.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    // ── Subtasks ──────────────────────────────────────────────────────────────

    /// Create a new subtask under `parent_id`. Subtasks have no story points or epic.
    pub fn create_subtask(&self, title: &str, parent_id: i64, status: &Status) -> Result<i64> {
        let now = now_str();
        let completed_at: Option<&str> = if status == &Status::Done { Some(&now) } else { None };
        self.conn.execute(
            "INSERT INTO issues
             (title, story_points, epic, status, parent_id, created_at, updated_at, completed_at)
             VALUES (?1, 0, '', ?2, ?3, ?4, ?4, ?5)",
            params![title, status.to_db(), parent_id, now, completed_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Update a subtask's title and status.
    pub fn update_subtask(&self, id: i64, title: &str, status: &Status) -> Result<()> {
        let now = now_str();
        let prev = self.get_issue(id)?;
        let completed_at: Option<String> = match status {
            Status::Done => {
                if prev.status == Status::Done {
                    prev.completed_at.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                } else {
                    Some(now.clone())
                }
            }
            _ => None,
        };
        self.conn.execute(
            "UPDATE issues SET title=?1, status=?2, updated_at=?3, completed_at=?4 WHERE id=?5",
            params![title, status.to_db(), now, completed_at, id],
        )?;
        Ok(())
    }

    /// Recompute and persist a parent issue's status from its subtasks:
    /// - any subtask InProgress → InProgress
    /// - all subtasks Done → Done
    /// - otherwise → Todo
    pub fn update_parent_status_from_children(&self, parent_id: i64) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM issues WHERE parent_id = ?1 AND deleted_at IS NULL",
        )?;
        let statuses: Vec<String> = stmt
            .query_map(params![parent_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        if statuses.is_empty() {
            return Ok(());
        }

        // all DONE → Done; all TODO → Todo; any mix (incl. TODO+DONE) → InProgress
        let new_status = if statuses.iter().all(|s| s == "DONE") {
            Status::Done
        } else if statuses.iter().all(|s| s == "TODO") {
            Status::Todo
        } else {
            Status::InProgress
        };

        let now = now_str();
        let completed_at: Option<String> = if new_status == Status::Done {
            // Only set completed_at if it wasn't already done
            let prev = self.get_issue(parent_id)?;
            if prev.status == Status::Done {
                prev.completed_at.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            } else {
                Some(now.clone())
            }
        } else {
            None
        };

        self.conn.execute(
            "UPDATE issues SET status=?1, updated_at=?2, completed_at=?3 WHERE id=?4",
            params![new_status.to_db(), now, completed_at, parent_id],
        )?;
        Ok(())
    }

    /// Restore an issue row to an exact prior snapshot (used by undo).
    /// Restores all mutable fields: title, story_points, epic, status, due_date,
    /// description, sprint_id, updated_at, completed_at, parent_id, rank, deleted_at.
    pub fn restore_issue_to_snapshot(&self, snap: &Issue) -> Result<()> {
        let due_str = snap.due_date.map(|d| d.format("%Y-%m-%d").to_string());
        let updated_str = snap.updated_at.format("%Y-%m-%d %H:%M:%S").to_string();
        let completed_str = snap.completed_at.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string());
        self.conn.execute(
            "UPDATE issues
             SET title=?1, story_points=?2, epic=?3, status=?4, due_date=?5,
                 description=?6, sprint_id=?7, updated_at=?8, completed_at=?9,
                 parent_id=?10, rank=?11, deleted_at=NULL
             WHERE id=?12",
            params![
                snap.title, snap.story_points, snap.epic, snap.status.to_db(),
                due_str, snap.description, snap.sprint_id,
                updated_str, completed_str,
                snap.parent_id, snap.rank,
                snap.id,
            ],
        )?;
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

    /// Swap the rank values of two issues (used for reordering in the backlog).
    /// Also equalises their updated_at so they sort adjacently in the updated_at DESC order.
    pub fn swap_rank(&self, id_a: i64, id_b: i64) -> Result<()> {
        let rank_a: i64 = self.conn.query_row(
            "SELECT rank FROM issues WHERE id = ?1", params![id_a], |r| r.get(0))?;
        let rank_b: i64 = self.conn.query_row(
            "SELECT rank FROM issues WHERE id = ?1", params![id_b], |r| r.get(0))?;
        // Use the same timestamp for both so they share an updated_at bucket,
        // letting the rank tiebreaker determine their relative order.
        let now = now_str();
        self.conn.execute(
            "UPDATE issues SET rank = ?1, updated_at = ?2 WHERE id = ?3",
            params![rank_b, now, id_a])?;
        self.conn.execute(
            "UPDATE issues SET rank = ?1, updated_at = ?2 WHERE id = ?3",
            params![rank_a, now, id_b])?;
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
        // If this issue is a subtask, cascade status update to its parent
        let parent_id: Option<i64> = self.conn.query_row(
            "SELECT parent_id FROM issues WHERE id = ?1",
            params![issue_id],
            |row| row.get(0),
        ).ok().flatten();
        if let Some(pid) = parent_id {
            let _ = self.update_parent_status_from_children(pid);
        }
        Ok(())
    }

    // ── Sprints ───────────────────────────────────────────────────────────────

    /// Return all sprints ordered newest first (by id desc).
    pub fn get_all_sprints(&self) -> Result<Vec<Sprint>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, start_date, end_date, is_active, created_at
             FROM sprints ORDER BY id DESC",
        )?;
        let sprints = stmt.query_map([], |row| {
            Ok(Sprint {
                id: row.get(0)?,
                name: row.get(1)?,
                start_date: NaiveDate::parse_from_str(&row.get::<_, String>(2)?, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive()),
                end_date: NaiveDate::parse_from_str(&row.get::<_, String>(3)?, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive()),
                is_active: row.get::<_, i32>(4)? != 0,
                created_at: parse_dt(&row.get::<_, String>(5)?),
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sprints)
    }

    /// Return all issues that were ever in a given sprint (including deleted), ordered by rank.
    pub fn get_sprint_issues(&self, sprint_id: i64) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, story_points, epic, status, due_date, description,
                    sprint_id, created_at, updated_at, completed_at, parent_id, rank
             FROM issues
             WHERE sprint_id = ?1 AND parent_id IS NULL
             ORDER BY rank, id",
        )?;
        let issues = stmt.query_map(params![sprint_id], |row| {
            let status_str: String = row.get(4)?;
            let due_str: Option<String> = row.get(5)?;
            let completed_str: Option<String> = row.get(10)?;
            Ok(Issue {
                id: row.get(0)?,
                title: row.get(1)?,
                story_points: row.get(2)?,
                epic: row.get(3)?,
                status: Status::from_db(&status_str),
                due_date: due_str.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
                description: row.get(6)?,
                sprint_id: row.get(7)?,
                created_at: parse_dt(&row.get::<_, String>(8)?),
                updated_at: parse_dt(&row.get::<_, String>(9)?),
                completed_at: completed_str.as_deref().map(parse_dt),
                parent_id: row.get(11)?,
                rank: row.get(12)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(issues)
    }

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
