/// Jira CSV importer.
///
/// Two-pass import:
///   Pass 1 — import Story / Task / Bug rows that have an Epic parent → creates
///             top-level scrumtui issues.  Builds a jira_key → local_id map.
///   Pass 2 — import Subtask rows whose parent key is in that map → creates
///             scrumtui subtasks linked to their parent issue.
///
/// Header columns (0-indexed):
///   0   Summary
///   1   Issue key       (e.g. KAT-1962)
///   3   Issue Type      "Story" | "Task" | "Bug" | "Subtask" | "Epic"
///   4   Status
///   5   Project key
///  19   Created         "21/Nov/25 3:28 PM"
///  20   Updated
///  22   Resolved
///  23   Due date
///  26   Description
///  64   Custom field (Story point estimate)
///  67   Parent          (numeric Jira issue id — unused)
///  68   Parent key      (e.g. KAT-1693)  ← parent Epic for Stories; parent Story for Subtasks
///  69   Parent summary  ← parent Epic name (used as local epic label)
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use std::collections::HashMap;

use crate::db::Db;
use crate::models::Status;

const COL_TITLE:          usize = 0;
const COL_KEY:            usize = 1;
const COL_ISSUE_TYPE:     usize = 3;
const COL_STATUS:         usize = 4;
const COL_PROJECT:        usize = 5;
const COL_CREATED:        usize = 19;
const COL_UPDATED:        usize = 20;
const COL_RESOLVED:       usize = 22;
const COL_DUE:            usize = 23;
const COL_DESC:           usize = 26;
const COL_SP:             usize = 64;
const COL_PARENT_KEY:     usize = 68;
const COL_PARENT_SUMMARY: usize = 69;

pub struct ImportReport {
    pub imported: usize,
    pub subtasks_imported: usize,
    pub skipped: usize,
}

pub fn import_jira_csv(db: &Db, path: &str) -> Result<ImportReport> {
    // ── Pass 1: Stories / Tasks / Bugs ────────────────────────────────────────
    // Read all records into memory so we can do two passes without seeking.
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("Cannot open {path}"))?;

    let all_records: Vec<csv::StringRecord> = rdr
        .records()
        .filter_map(|r| r.ok())
        .collect();

    let mut imported = 0usize;
    let mut subtasks_imported = 0usize;
    let mut skipped = 0usize;

    // jira_key (e.g. "KAT-1962") → local scrumtui issue id
    let mut key_to_id: HashMap<String, i64> = HashMap::new();

    for record in &all_records {
        let title = get(record, COL_TITLE).trim().to_string();
        if title.is_empty() { skipped += 1; continue; }

        let issue_type = get(record, COL_ISSUE_TYPE).trim();
        let parent_key = get(record, COL_PARENT_KEY).trim();
        let jira_key = get(record, COL_KEY).trim().to_string();

        // Only Stories / Tasks / Bugs with an Epic parent
        if !matches!(issue_type, "Story" | "Task" | "Bug") || parent_key.is_empty() {
            skipped += 1;
            continue;
        }

        let status = parse_status(get(record, COL_STATUS));
        let project_key = get(record, COL_PROJECT).to_lowercase();
        let parent_summary = get(record, COL_PARENT_SUMMARY).trim().to_string();
        let epic = if parent_summary.is_empty() { project_key } else { parent_summary.to_lowercase() };

        let due_date = parse_due(get(record, COL_DUE));
        let description = {
            let d = get(record, COL_DESC).trim().to_string();
            if d.is_empty() { None } else { Some(d) }
        };
        let story_points = get(record, COL_SP)
            .trim().parse::<f64>().unwrap_or(1.0).max(0.5);

        let created_at = parse_jira_dt(get(record, COL_CREATED))
            .unwrap_or_else(|| "2000-01-01 00:00:00".to_string());
        let updated_at = parse_jira_dt(get(record, COL_UPDATED))
            .unwrap_or_else(|| created_at.clone());
        let completed_at: Option<String> = match status {
            Status::Done => parse_jira_dt(get(record, COL_RESOLVED))
                .or_else(|| parse_jira_dt(get(record, COL_UPDATED))),
            _ => None,
        };

        let local_id = db.create_issue_full(
            &title,
            story_points,
            &epic,
            &status,
            due_date,
            description.as_deref(),
            None,
            &created_at,
            &updated_at,
            completed_at.as_deref(),
        )
        .with_context(|| format!("Failed inserting story: {title}"))?;

        if !jira_key.is_empty() {
            key_to_id.insert(jira_key, local_id);
        }
        imported += 1;
    }

    // ── Pass 2: Subtasks ───────────────────────────────────────────────────────
    for record in &all_records {
        let title = get(record, COL_TITLE).trim().to_string();
        if title.is_empty() { continue; }

        let issue_type = get(record, COL_ISSUE_TYPE).trim();
        if issue_type != "Subtask" { continue; }

        let parent_key = get(record, COL_PARENT_KEY).trim();
        if parent_key.is_empty() { skipped += 1; continue; }

        // Only import if parent story was imported in pass 1
        let parent_local_id = match key_to_id.get(parent_key) {
            Some(&id) => id,
            None => { skipped += 1; continue; }
        };

        let status = parse_status(get(record, COL_STATUS));
        let created_at = parse_jira_dt(get(record, COL_CREATED))
            .unwrap_or_else(|| "2000-01-01 00:00:00".to_string());
        let updated_at = parse_jira_dt(get(record, COL_UPDATED))
            .unwrap_or_else(|| created_at.clone());
        let completed_at: Option<String> = match status {
            Status::Done => parse_jira_dt(get(record, COL_RESOLVED))
                .or_else(|| parse_jira_dt(get(record, COL_UPDATED))),
            _ => None,
        };

        db.create_issue_full(
            &title,
            0.0,           // subtasks carry no story points
            "",            // no epic
            &status,
            None,          // no due date on subtasks
            None,          // no description
            Some(parent_local_id),
            &created_at,
            &updated_at,
            completed_at.as_deref(),
        )
        .with_context(|| format!("Failed inserting subtask: {title}"))?;

        subtasks_imported += 1;
    }

    // After importing, recompute parent statuses from their subtasks
    for &parent_id in key_to_id.values() {
        let _ = db.update_parent_status_from_children(parent_id);
    }

    Ok(ImportReport { imported, subtasks_imported, skipped })
}

fn get<'a>(record: &'a csv::StringRecord, idx: usize) -> &'a str {
    record.get(idx).unwrap_or("").trim()
}

fn parse_status(s: &str) -> Status {
    match s.trim() {
        "In Progress" | "In Review" | "In Development" => Status::InProgress,
        "Done" | "Closed" | "Resolved" | "Won't Do" => Status::Done,
        _ => Status::Todo,
    }
}

/// Parse Jira datetime "21/Nov/25 3:28 PM" → "2025-11-21 15:28:00"
fn parse_jira_dt(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() { return None; }
    NaiveDateTime::parse_from_str(s, "%d/%b/%y %I:%M %p")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%d/%b/%y %l:%M %p"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn parse_due(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() { return None; }
    NaiveDate::parse_from_str(s.split_whitespace().next().unwrap_or(""), "%d/%b/%y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
}
