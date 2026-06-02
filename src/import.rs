/// Jira CSV importer — column indices are hardcoded to Kat's Jira export format.
///
/// Only imports "Story" type issues that have a non-empty parent key
/// (i.e., stories that live under an Epic). Sub-tasks are excluded.
///
/// Header reference (0-indexed):
///  0  Summary
///  3  Issue Type        "Story" | "Epic" | "Sub-task" …
///  4  Status
///  5  Project key
/// 19  Created           "11/Aug/25 10:13 AM"
/// 20  Updated
/// 22  Resolved
/// 23  Due date
/// 26  Description
/// 64  Custom field (Story point estimate)
/// 68  Parent key        non-empty → has a parent
/// 69  Parent summary    used as epic label
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};

use crate::db::Db;
use crate::models::Status;

const COL_TITLE:          usize = 0;
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

pub fn import_jira_csv(db: &Db, path: &str) -> Result<ImportReport> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("Cannot open {path}"))?;

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for result in rdr.records() {
        let record = match result {
            Ok(r) => r,
            Err(_) => { skipped += 1; continue; }
        };

        let title = get(&record, COL_TITLE).trim().to_string();
        if title.is_empty() { skipped += 1; continue; }

        // Only import Stories (or Tasks) that have a parent Epic
        let issue_type = get(&record, COL_ISSUE_TYPE).trim();
        let parent_key = get(&record, COL_PARENT_KEY).trim();
        if !matches!(issue_type, "Story" | "Task" | "Bug") || parent_key.is_empty() {
            skipped += 1;
            continue;
        }

        let status = parse_status(get(&record, COL_STATUS));
        let project_key = get(&record, COL_PROJECT).to_lowercase();
        let parent_summary = get(&record, COL_PARENT_SUMMARY).trim().to_string();
        let epic = if parent_summary.is_empty() { project_key } else { parent_summary.to_lowercase() };

        let due_date = parse_due(get(&record, COL_DUE));
        let description = {
            let d = get(&record, COL_DESC).trim().to_string();
            if d.is_empty() { None } else { Some(d) }
        };
        let story_points = get(&record, COL_SP)
            .trim().parse::<f64>().unwrap_or(1.0).max(0.5);

        // Timestamps
        let created_at = parse_jira_dt(get(&record, COL_CREATED))
            .unwrap_or_else(|| "2000-01-01 00:00:00".to_string());
        let updated_at = parse_jira_dt(get(&record, COL_UPDATED))
            .unwrap_or_else(|| created_at.clone());
        let completed_at: Option<String> = match status {
            Status::Done => parse_jira_dt(get(&record, COL_RESOLVED))
                .or_else(|| parse_jira_dt(get(&record, COL_UPDATED))),
            _ => None,
        };

        db.create_issue_full(
            &title,
            story_points,
            &epic,
            &status,
            due_date,
            description.as_deref(),
            None, // parent_id — we don't link epics into the local issue graph
            &created_at,
            &updated_at,
            completed_at.as_deref(),
        )
        .with_context(|| format!("Failed inserting: {title}"))?;

        imported += 1;
    }

    Ok(ImportReport { imported, skipped })
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

/// Parse "11/Aug/25 10:13 AM" → "2025-08-11 10:13:00"
fn parse_jira_dt(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() { return None; }
    NaiveDateTime::parse_from_str(s, "%d/%b/%y %I:%M %p")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn parse_due(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() { return None; }
    // "11/Aug/25 10:13 AM" — take date part only
    NaiveDate::parse_from_str(s.split_whitespace().next().unwrap_or(""), "%d/%b/%y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
}

pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
}
