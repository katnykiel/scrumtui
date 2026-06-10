/// Jira CSV importer — two-pass with sprint support.
///
/// Pass 1: Story / Task / Bug rows that have an Epic parent  →  top-level issues.
///         Collects sprint membership per jira_key.
///         Collects per-sprint date ranges (min created, max resolved/updated).
/// Pass 2: Subtask rows whose parent key was imported in pass 1  →  child issues.
///
/// After both passes:
///   • Sprints are created (or reused if already present by name).
///   • Issues are assigned to their sprint.
///   • All ranks are re-assigned by created_at so newest appears first.
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
///  41‥62 Sprint         (multiple columns with the same header; take first non-empty)
///  64   Custom field (Story point estimate)
///  68   Parent key      parent Epic key for Stories; parent Story key for Subtasks
///  69   Parent summary  Epic name → used as local epic label
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
// Columns 41-62 are all named "Sprint" in the Jira export.
const COL_SPRINT_FIRST:   usize = 41;
const COL_SPRINT_LAST:    usize = 62;
const COL_SP:             usize = 64;
const COL_PARENT_KEY:     usize = 68;
const COL_PARENT_SUMMARY: usize = 69;

pub struct ImportReport {
    pub imported: usize,
    pub subtasks_imported: usize,
    pub sprints_created: usize,
    pub skipped: usize,
}

pub fn import_jira_csv(db: &Db, path: &str) -> Result<ImportReport> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("Cannot open {path}"))?;

    // Read all records into memory for the two-pass approach.
    let all_records: Vec<csv::StringRecord> = rdr
        .records()
        .filter_map(|r| r.ok())
        .collect();

    let mut imported        = 0usize;
    let mut subtasks_imported = 0usize;
    let mut skipped         = 0usize;

    // jira_key → local scrumtui issue id
    let mut key_to_id: HashMap<String, i64> = HashMap::new();
    // jira_key → sprint name (take the last/most-recent sprint listed for the issue)
    let mut key_to_sprint: HashMap<String, String> = HashMap::new();
    // sprint_name → (min_created_date, max_end_date)
    let mut sprint_dates: HashMap<String, (NaiveDate, NaiveDate)> = HashMap::new();

    // ── Pass 1: Stories / Tasks / Bugs ────────────────────────────────────────
    for record in &all_records {
        let title = get(record, COL_TITLE).trim().to_string();
        if title.is_empty() { skipped += 1; continue; }

        let issue_type = get(record, COL_ISSUE_TYPE).trim();
        let parent_key = get(record, COL_PARENT_KEY).trim();
        let jira_key   = get(record, COL_KEY).trim().to_string();

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
            key_to_id.insert(jira_key.clone(), local_id);
        }
        imported += 1;

        // Collect sprint membership — use the last non-empty sprint column value
        // (the last sprint is the most recent one the issue was in).
        let sprint_name = (COL_SPRINT_FIRST..=COL_SPRINT_LAST)
            .filter_map(|c| {
                let v = record.get(c).unwrap_or("").trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            })
            .last();

        if let Some(name) = sprint_name {
            // Track date range for this sprint
            if let Some(created_date) = parse_date_from_dt(&created_at) {
                let end_date = completed_at.as_deref()
                    .and_then(parse_date_from_dt)
                    .or_else(|| parse_date_from_dt(&updated_at))
                    .unwrap_or(created_date);

                let entry = sprint_dates.entry(name.clone()).or_insert((created_date, end_date));
                if created_date < entry.0 { entry.0 = created_date; }
                if end_date    > entry.1 { entry.1 = end_date; }
            }
            key_to_sprint.insert(jira_key, name);
        }
    }

    // ── Pass 2: Subtasks ───────────────────────────────────────────────────────
    for record in &all_records {
        let title = get(record, COL_TITLE).trim().to_string();
        if title.is_empty() { continue; }
        if get(record, COL_ISSUE_TYPE).trim() != "Subtask" { continue; }

        let parent_key = get(record, COL_PARENT_KEY).trim();
        if parent_key.is_empty() { skipped += 1; continue; }

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
            &title, 0.0, "", &status,
            None, None,
            Some(parent_local_id),
            &created_at, &updated_at,
            completed_at.as_deref(),
        )
        .with_context(|| format!("Failed inserting subtask: {title}"))?;

        subtasks_imported += 1;
    }

    // Recompute parent statuses from subtasks
    for &parent_id in key_to_id.values() {
        let _ = db.update_parent_status_from_children(parent_id);
    }

    // ── Create sprints and assign issues ──────────────────────────────────────
    let mut sprints_created = 0usize;
    // sprint_name → local sprint id
    let mut sprint_name_to_id: HashMap<String, i64> = HashMap::new();

    for (sprint_name, (start, end)) in &sprint_dates {
        // Ensure end >= start
        let end = (*end).max(*start);
        let sprint_id = db.get_or_create_sprint(sprint_name, *start, end)
            .with_context(|| format!("Failed creating sprint: {sprint_name}"))?;
        // Count as "created" only for new ones (get_or_create returns existing silently)
        sprints_created += 1;
        sprint_name_to_id.insert(sprint_name.clone(), sprint_id);
    }

    // Assign sprint_id to issues
    for (jira_key, sprint_name) in &key_to_sprint {
        if let (Some(&local_id), Some(&sprint_id)) = (
            key_to_id.get(jira_key),
            sprint_name_to_id.get(sprint_name),
        ) {
            let _ = db.set_issue_sprint(local_id, Some(sprint_id));
        }
    }

    // ── Re-rank all issues by created_at so newest appears first ─────────────
    db.rerank_by_created_at()?;

    Ok(ImportReport { imported, subtasks_imported, sprints_created, skipped })
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

fn parse_date_from_dt(s: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| dt.date())
}
