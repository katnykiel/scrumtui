use std::collections::HashMap;

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{Issue, Outcome, Sprint, SprintEntry, Status};

pub struct Db {
    conn: Connection,
}

/// Columns read by `row_to_issue`, in order. Queries alias `issues` as `i`.
const ISSUE_COLS: &str = "i.id, i.title, i.story_points, i.epic, i.status, i.due_date, i.description,
     i.sprint_id, i.created_at, i.updated_at, i.completed_at, i.parent_id, i.rank, i.carry_count,
     i.started_at";

/// Number of columns in `ISSUE_COLS`; extra selected columns start at this index.
const ISSUE_COL_COUNT: usize = 15;

fn parse_dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .unwrap_or_else(|_| Local::now().naive_local())
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_else(|_| Local::now().date_naive())
}

fn now_str() -> String {
    Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn row_to_issue(row: &rusqlite::Row) -> rusqlite::Result<Issue> {
    let status_str: String = row.get(4)?;
    let due_str: Option<String> = row.get(5)?;
    let completed_str: Option<String> = row.get(10)?;
    let started_str: Option<String> = row.get(14)?;
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
        carry_count: row.get(13).unwrap_or(0),
        started_at: started_str.as_deref().map(parse_dt),
    })
}

fn row_to_sprint(row: &rusqlite::Row) -> rusqlite::Result<Sprint> {
    Ok(Sprint {
        id: row.get(0)?,
        name: row.get(1)?,
        start_date: parse_date(&row.get::<_, String>(2)?),
        end_date: parse_date(&row.get::<_, String>(3)?),
        is_active: row.get::<_, i32>(4)? != 0,
        created_at: parse_dt(&row.get::<_, String>(5)?),
    })
}

/// One raw `issue_sprint_log` row, kept verbatim so undo can restore it exactly.
#[derive(Debug, Clone)]
pub struct SprintLogRow {
    pub sprint_id: i64,
    pub added_at: Option<String>,
    pub removed_at: Option<String>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        // The TUI and the CLI (e.g. `scrumtui add` run by a script or an AI assistant)
        // may write at the same time; wait for the lock instead of failing.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let db = Db { conn };
        db.init()?;
        Ok(db)
    }

    /// Direct access for the demo seeder, which needs to back-date timestamps.
    pub fn conn(&self) -> &Connection {
        &self.conn
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
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN carry_count INTEGER NOT NULL DEFAULT 0;",
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN started_at TEXT;",
        );
        // primary_sprint_id kept for backwards compat with existing databases (ignored)
        let _ = self.conn.execute_batch(
            "ALTER TABLE issues ADD COLUMN primary_sprint_id INTEGER;",
        );
        // Sprint membership log: every (issue, sprint) pairing that has ever existed.
        // Used by the history view so carried issues appear in every sprint they touched.
        let _ = self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS issue_sprint_log (
                issue_id  INTEGER NOT NULL,
                sprint_id INTEGER NOT NULL,
                PRIMARY KEY (issue_id, sprint_id)
            );",
        );
        // When the issue joined the sprint, and when it was taken out again by hand
        // (NULL = still in, or carried forward unfinished). NULL added_at = legacy row.
        let _ = self.conn.execute_batch(
            "ALTER TABLE issue_sprint_log ADD COLUMN added_at TEXT;",
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE issue_sprint_log ADD COLUMN removed_at TEXT;",
        );
        // Backfill log from issues.sprint_id for databases that predate this table.
        let _ = self.conn.execute_batch(
            "INSERT OR IGNORE INTO issue_sprint_log (issue_id, sprint_id)
             SELECT id, sprint_id FROM issues WHERE sprint_id IS NOT NULL;",
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

    /// Changes whenever another connection (e.g. the CLI) commits to the database.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    // ── Issues ────────────────────────────────────────────────────────────────

    pub fn get_all_issues(&self) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ISSUE_COLS}
             FROM issues i
             WHERE i.deleted_at IS NULL
             ORDER BY (i.sprint_id IS NULL), i.rank DESC, i.id DESC"
        ))?;
        let issues = stmt
            .query_map([], row_to_issue)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(issues)
    }

    pub fn get_issue(&self, id: i64) -> Result<Issue> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ISSUE_COLS} FROM issues i WHERE i.id = ?1"))?;
        Ok(stmt.query_row(params![id], row_to_issue)?)
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
        let id = self.create_issue_full(
            title, story_points, epic, status, due_date, description,
            None, &now, &now, None,
        )?;
        // Issues created straight into IN PROGRESS start their clock now.
        if status != &Status::Todo {
            self.conn.execute(
                "UPDATE issues SET started_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        if status == &Status::Done {
            self.conn.execute(
                "UPDATE issues SET completed_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        Ok(id)
    }

    /// Full insert with explicit timestamps and optional parent. Used by import.
    #[allow(clippy::too_many_arguments)]
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
        // rank = max(rank) + 1: new issues sort first under `rank DESC`.
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

    #[allow(clippy::too_many_arguments)]
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
        // Preserve started_at if already set; set it if transitioning to InProgress/Done;
        // clear it if going back to Todo
        let started_at: Option<String> = match status {
            Status::Todo => None,
            Status::InProgress | Status::Done => {
                prev.started_at
                    .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                    .or_else(|| Some(now.clone()))
            }
        };
        self.conn.execute(
            "UPDATE issues
             SET title=?1, story_points=?2, epic=?3, status=?4, due_date=?5,
                 description=?6, updated_at=?7, completed_at=?8, parent_id=?9, started_at=?10
             WHERE id=?11",
            params![title, story_points, epic, status.to_db(), due_str, description, now, completed_at, parent_id, started_at, id],
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
        self.conn.execute("DELETE FROM issue_sprint_log WHERE issue_id=?1", params![id])?;
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
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ISSUE_COLS}
             FROM issues i
             WHERE i.deleted_at IS NOT NULL AND i.parent_id IS NULL
             ORDER BY i.deleted_at DESC"
        ))?;
        let items = stmt
            .query_map([], row_to_issue)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    // ── Subtasks ──────────────────────────────────────────────────────────────

    /// Create a new subtask under `parent_id`. Subtasks have no story points or epic.
    pub fn create_subtask(&self, title: &str, parent_id: i64, status: &Status) -> Result<i64> {
        let now = now_str();
        let completed_at: Option<&str> = if status == &Status::Done { Some(&now) } else { None };
        let started_at: Option<&str> = if status == &Status::Todo { None } else { Some(&now) };
        // Rank like any other issue so siblings keep creation order under `rank DESC`.
        let next_rank: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(rank), 0) + 1 FROM issues", [], |r| r.get(0)
        ).unwrap_or(1);
        self.conn.execute(
            "INSERT INTO issues
             (title, story_points, epic, status, parent_id, created_at, updated_at, completed_at,
              started_at, rank)
             VALUES (?1, 0, '', ?2, ?3, ?4, ?4, ?5, ?6, ?7)",
            params![title, status.to_db(), parent_id, now, completed_at, started_at, next_rank],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Update a subtask's title and status.
    pub fn update_subtask(&self, id: i64, title: &str, status: &Status) -> Result<()> {
        let prev = self.get_issue(id)?;
        if prev.title == title && &prev.status == status {
            return Ok(());
        }
        if prev.title != title {
            self.conn.execute(
                "UPDATE issues SET title=?1, updated_at=?2 WHERE id=?3",
                params![title, now_str(), id],
            )?;
        }
        if &prev.status != status {
            self.set_status_only(id, status)?;
        }
        Ok(())
    }

    /// Recompute and persist a parent issue's status from its subtasks:
    /// all DONE → Done; all TODO → Todo; any mix (incl. TODO+DONE) → InProgress.
    /// No-op when the parent has no subtasks or its status already matches.
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

        let new_status = if statuses.iter().all(|s| s == "DONE") {
            Status::Done
        } else if statuses.iter().all(|s| s == "TODO") {
            Status::Todo
        } else {
            Status::InProgress
        };

        let current: String = self.conn.query_row(
            "SELECT status FROM issues WHERE id = ?1",
            params![parent_id],
            |r| r.get(0),
        )?;
        if current == new_status.to_db() {
            return Ok(());
        }
        self.set_status_only(parent_id, &new_status)
    }

    /// Write a status change and keep the timestamps consistent:
    /// completed_at is set when an issue becomes DONE (kept if it already was) and cleared
    /// otherwise; started_at is set on the first move out of TODO and cleared on a
    /// return to TODO, so "actual time" covers the whole stretch of work.
    fn set_status_only(&self, issue_id: i64, status: &Status) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET
                completed_at = CASE WHEN ?1 = 'DONE'
                                    THEN COALESCE(CASE WHEN status = 'DONE' THEN completed_at END, ?2)
                                    ELSE NULL END,
                started_at   = CASE WHEN ?1 = 'TODO' THEN NULL
                                    ELSE COALESCE(started_at, ?2) END,
                status = ?1,
                updated_at = ?2
             WHERE id = ?3",
            params![status.to_db(), now_str(), issue_id],
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
        let started_str = snap.started_at.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string());
        self.conn.execute(
            "UPDATE issues
             SET title=?1, story_points=?2, epic=?3, status=?4, due_date=?5,
                 description=?6, sprint_id=?7, updated_at=?8, completed_at=?9,
                 parent_id=?10, rank=?11, started_at=?12, deleted_at=NULL
             WHERE id=?13",
            params![
                snap.title, snap.story_points, snap.epic, snap.status.to_db(),
                due_str, snap.description, snap.sprint_id,
                updated_str, completed_str,
                snap.parent_id, snap.rank, started_str,
                snap.id,
            ],
        )?;
        Ok(())
    }

    /// Swap the rank values of two issues (used for reordering in the backlog).
    pub fn swap_rank(&self, id_a: i64, id_b: i64) -> Result<()> {
        let rank_a: i64 = self.conn.query_row(
            "SELECT rank FROM issues WHERE id = ?1", params![id_a], |r| r.get(0))?;
        let rank_b: i64 = self.conn.query_row(
            "SELECT rank FROM issues WHERE id = ?1", params![id_b], |r| r.get(0))?;
        // If ranks are equal (legacy data), assign distinct values first
        if rank_a == rank_b {
            self.conn.execute(
                "UPDATE issues SET rank = ?1 WHERE id = ?2",
                params![rank_a + 1, id_a])?;
            self.conn.execute(
                "UPDATE issues SET rank = ?1 WHERE id = ?2",
                params![rank_b, id_b])?;
            return Ok(());
        }
        self.conn.execute(
            "UPDATE issues SET rank = ?1 WHERE id = ?2",
            params![rank_b, id_a])?;
        self.conn.execute(
            "UPDATE issues SET rank = ?1 WHERE id = ?2",
            params![rank_a, id_b])?;
        Ok(())
    }

    pub fn set_issue_due_date(&self, issue_id: i64, due_date: NaiveDate) -> Result<()> {
        let due_str = due_date.format("%Y-%m-%d").to_string();
        self.conn.execute(
            "UPDATE issues SET due_date=?1 WHERE id=?2",
            params![due_str, issue_id],
        )?;
        Ok(())
    }

    /// Move an issue into a sprint (or back to the backlog with `None`) and log it.
    ///
    /// Leaving a sprint that is still running (the active one, or one whose end date
    /// hasn't passed) marks the membership as removed, so the history view can tell
    /// de-scoped work from work that was carried forward unfinished.
    pub fn set_issue_sprint(&self, issue_id: i64, sprint_id: Option<i64>) -> Result<()> {
        let now = now_str();
        let old: Option<i64> = self
            .conn
            .query_row("SELECT sprint_id FROM issues WHERE id=?1", params![issue_id], |r| r.get(0))
            .optional()?
            .flatten();
        self.conn.execute(
            "UPDATE issues SET sprint_id=?1, updated_at=?2 WHERE id=?3",
            params![sprint_id, now, issue_id],
        )?;
        if let Some(old_id) = old.filter(|o| Some(*o) != sprint_id) {
            self.conn.execute(
                "UPDATE issue_sprint_log SET removed_at = ?1
                 WHERE issue_id = ?2 AND sprint_id = ?3 AND removed_at IS NULL
                   AND EXISTS (SELECT 1 FROM sprints s WHERE s.id = ?3
                               AND (s.is_active = 1 OR s.end_date >= date(?1)))",
                params![now, issue_id, old_id],
            )?;
        }
        if let Some(sid) = sprint_id {
            self.conn.execute(
                "INSERT INTO issue_sprint_log (issue_id, sprint_id, added_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(issue_id, sprint_id) DO UPDATE SET removed_at = NULL",
                params![issue_id, sid, now],
            )?;
        }
        Ok(())
    }

    /// All sprint-log rows of one issue (snapshot for undo).
    pub fn sprint_log_for_issue(&self, issue_id: i64) -> Result<Vec<SprintLogRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT sprint_id, added_at, removed_at FROM issue_sprint_log WHERE issue_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![issue_id], |r| {
                Ok(SprintLogRow { sprint_id: r.get(0)?, added_at: r.get(1)?, removed_at: r.get(2)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Put an issue's sprint assignment and sprint-log rows back exactly as they were.
    pub fn restore_sprint_membership(
        &self,
        issue_id: i64,
        sprint_id: Option<i64>,
        log: &[SprintLogRow],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE issues SET sprint_id=?1 WHERE id=?2", params![sprint_id, issue_id])?;
        tx.execute("DELETE FROM issue_sprint_log WHERE issue_id=?1", params![issue_id])?;
        for row in log {
            tx.execute(
                "INSERT INTO issue_sprint_log (issue_id, sprint_id, added_at, removed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![issue_id, row.sprint_id, row.added_at, row.removed_at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Change an issue's status. Cascades to the parent when the issue is a subtask.
    pub fn update_issue_status(&self, issue_id: i64, status: &Status) -> Result<()> {
        self.set_status_only(issue_id, status)?;
        let parent_id: Option<i64> = self.conn.query_row(
            "SELECT parent_id FROM issues WHERE id = ?1",
            params![issue_id],
            |row| row.get(0),
        ).optional()?.flatten();
        if let Some(pid) = parent_id {
            self.update_parent_status_from_children(pid)?;
        }
        Ok(())
    }

    // ── Sprints ───────────────────────────────────────────────────────────────

    /// Return all sprints ordered by end_date descending (most recently completed first).
    pub fn get_all_sprints(&self) -> Result<Vec<Sprint>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, start_date, end_date, is_active, created_at
             FROM sprints ORDER BY end_date DESC, id DESC",
        )?;
        let sprints = stmt.query_map([], row_to_sprint)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sprints)
    }

    /// Every sprint's membership log, keyed by sprint id, with each entry's outcome:
    /// done (finished while this was its sprint), open (still assigned, unfinished),
    /// carried (moved on to a later sprint unfinished) or removed (taken out by hand).
    /// Subtasks and trashed issues are left out.
    pub fn get_sprint_entries(&self) -> Result<HashMap<i64, Vec<SprintEntry>>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ISSUE_COLS}, l.sprint_id, l.added_at, l.removed_at
             FROM issue_sprint_log l
             JOIN issues i ON i.id = l.issue_id
             WHERE i.parent_id IS NULL AND i.deleted_at IS NULL
             ORDER BY i.rank DESC, i.id DESC"
        ))?;
        let rows = stmt.query_map([], |row| {
            let issue = row_to_issue(row)?;
            let sprint_id: i64 = row.get(ISSUE_COL_COUNT)?;
            let added: Option<String> = row.get(ISSUE_COL_COUNT + 1)?;
            let removed: Option<String> = row.get(ISSUE_COL_COUNT + 2)?;
            Ok((sprint_id, issue, added, removed))
        })?;
        let mut map: HashMap<i64, Vec<SprintEntry>> = HashMap::new();
        for row in rows {
            let (sprint_id, issue, added, removed) = row?;
            let removed_at = removed.as_deref().map(parse_dt);
            let outcome = if removed_at.is_some() {
                Outcome::Removed
            } else if issue.sprint_id != Some(sprint_id) {
                Outcome::Carried
            } else if issue.status == Status::Done {
                Outcome::Done
            } else {
                Outcome::Open
            };
            map.entry(sprint_id).or_default().push(SprintEntry {
                issue,
                added_at: added.as_deref().map(parse_dt),
                removed_at,
                outcome,
            });
        }
        Ok(map)
    }

    /// Increment carry_count for all TODO/IN_PROGRESS issues in a sprint.
    /// Call this when completing a sprint / activating a new one.
    pub fn bump_carry_count_for_sprint(&self, sprint_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET carry_count = carry_count + 1
             WHERE sprint_id = ?1 AND status IN ('TODO', 'IN_PROGRESS') AND deleted_at IS NULL",
            params![sprint_id],
        )?;
        Ok(())
    }

    /// Move all TODO/IN_PROGRESS issues from one sprint to another.
    /// Returns the number of issues moved.
    pub fn move_incomplete_issues_to_sprint(&self, from_sprint_id: i64, to_sprint_id: i64) -> Result<usize> {
        let now = now_str();
        // Carried issues are planned scope of the new sprint, so log them as joining no
        // later than its first morning, even when the sprint is created a day or two late.
        let to_start: Option<String> = self
            .conn
            .query_row(
                "SELECT start_date || ' 00:00:00' FROM sprints WHERE id = ?1",
                params![to_sprint_id],
                |r| r.get(0),
            )
            .optional()?;
        let added_at = match to_start {
            Some(start) if start < now => start,
            _ => now.clone(),
        };
        self.conn.execute(
            "INSERT INTO issue_sprint_log (issue_id, sprint_id, added_at)
             SELECT id, ?1, ?2 FROM issues
             WHERE sprint_id = ?3 AND status IN ('TODO', 'IN_PROGRESS') AND deleted_at IS NULL
             ON CONFLICT(issue_id, sprint_id) DO UPDATE SET removed_at = NULL",
            params![to_sprint_id, added_at, from_sprint_id],
        )?;
        let n = self.conn.execute(
            "UPDATE issues SET sprint_id = ?1, updated_at = ?2
             WHERE sprint_id = ?3 AND status IN ('TODO', 'IN_PROGRESS') AND deleted_at IS NULL",
            params![to_sprint_id, now, from_sprint_id],
        )?;
        Ok(n)
    }

    pub fn get_active_sprint(&self) -> Result<Option<Sprint>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, start_date, end_date, is_active, created_at
                 FROM sprints WHERE is_active = 1 LIMIT 1",
                [],
                row_to_sprint,
            )
            .optional()?)
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

    /// For every past (non-active) sprint shorter than a Monday–Friday week, back-date
    /// its start_date so it spans 5 days (start = end - 4). Sprints are processed in
    /// end_date ASC order; if extending a sprint would cause it to overlap the
    /// previous sprint, its start is clamped to prev_end + 1 day.
    /// A start date is only ever moved earlier, never later.
    pub fn fix_short_sprints(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_date, end_date FROM sprints WHERE is_active = 0 ORDER BY end_date ASC, id ASC",
        )?;
        let rows: Vec<(i64, NaiveDate, NaiveDate)> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                parse_date(&row.get::<_, String>(1)?),
                parse_date(&row.get::<_, String>(2)?),
            ))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;

        let mut prev_end: Option<NaiveDate> = None;
        for (id, start, end) in rows {
            let duration = (end - start).num_days() + 1;
            if duration < 5 {
                // Ideal: back-date to a full Monday–Friday length
                let mut new_start = end - chrono::Duration::days(4);
                // Clamp so we don't overlap the previous sprint
                if let Some(pe) = prev_end {
                    if new_start <= pe {
                        new_start = pe + chrono::Duration::days(1);
                    }
                }
                if new_start < start {
                    self.conn.execute(
                        "UPDATE sprints SET start_date = ?1 WHERE id = ?2",
                        params![new_start.format("%Y-%m-%d").to_string(), id],
                    )?;
                }
            }
            prev_end = Some(end);
        }
        Ok(())
    }

    /// Delete a sprint and unlink all its issues (sets sprint_id = NULL).
    pub fn delete_sprint(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE issues SET sprint_id = NULL WHERE sprint_id = ?1",
            params![id],
        )?;
        self.conn.execute("DELETE FROM issue_sprint_log WHERE sprint_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM sprints WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
        Ok(count == 0)
    }

    /// Re-assign rank values for all top-level issues so that newer created_at
    /// = higher rank, making them appear first in the `rank DESC` sort order.
    /// Subtasks keep their own ranks (only their order among siblings matters).
    /// Call this after a bulk import to fix ordering.
    pub fn rerank_by_created_at(&self) -> Result<()> {
        // Collect ids ordered oldest-first so we can assign rank 1, 2, 3 ...
        // (oldest gets the lowest rank; newest gets the highest → appears first
        //  in ORDER BY rank DESC).
        let mut stmt = self.conn.prepare(
            "SELECT id FROM issues
             WHERE parent_id IS NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let tx = self.conn.unchecked_transaction()?;
        for (rank, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE issues SET rank = ?1 WHERE id = ?2",
                params![(rank as i64) + 1, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Create a sprint, but only if no sprint with the same name already exists.
    /// Returns the id of the existing or newly-created sprint.
    pub fn get_or_create_sprint(
        &self,
        name: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<i64> {
        let existing: Option<i64> = self.conn
            .query_row(
                "SELECT id FROM sprints WHERE name = ?1 LIMIT 1",
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO sprints (name, start_date, end_date, is_active, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                name,
                start_date.format("%Y-%m-%d").to_string(),
                end_date.format("%Y-%m-%d").to_string(),
                now_str(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}
