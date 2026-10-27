use anyhow::{Context, Result};
use std::io::Write;

use crate::db::Db;
use crate::models::{format_sp, Issue, Status};

pub fn export_markdown(db: &Db, path: &str) -> Result<()> {
    let issues = db.get_all_issues()?;
    let sprint = db.get_active_sprint()?;

    let mut out = std::fs::File::create(path)
        .with_context(|| format!("Cannot create {path}"))?;

    let sym = |s: &Status| match s {
        Status::Todo => "○",
        Status::InProgress => "◉",
        Status::Done => "✓",
    };

    let due_str = |due: &Option<chrono::NaiveDate>| -> String {
        due.map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "—".into())
    };

    // A `|` inside a cell would end it early.
    let cell = |s: &str| s.replace('|', "\\|");

    let header = "| Status | Title | SP | Epic | Due |\n\
                  |--------|-------|----|------|-----|\n";

    // One issue row followed by its subtasks (in creation order).
    let write_issue = |out: &mut std::fs::File, issue: &Issue| -> Result<()> {
        writeln!(
            out,
            "| {} {} | {} | {} | {} | {} |",
            sym(&issue.status),
            issue.status.label(),
            cell(&issue.title),
            format_sp(issue.story_points),
            cell(&issue.epic),
            due_str(&issue.due_date),
        )?;
        // DB order is newest first; reverse for creation order.
        for child in issues.iter().rev().filter(|i| i.parent_id == Some(issue.id)) {
            writeln!(
                out,
                "| {} {} | ↳ {} |  |  | {} |",
                sym(&child.status),
                child.status.label(),
                cell(&child.title),
                due_str(&child.due_date),
            )?;
        }
        Ok(())
    };

    // ── Sprint section ─────────────────────────────────────────────────────────
    if let Some(ref s) = sprint {
        writeln!(
            out,
            "# {} · {} → {}\n",
            s.name,
            s.start_date.format("%b %d, %Y"),
            s.end_date.format("%b %d, %Y")
        )?;

        let sprint_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.sprint_id == Some(s.id) && i.parent_id.is_none())
            .collect();

        if !sprint_issues.is_empty() {
            write!(out, "{}", header)?;
            for issue in &sprint_issues {
                write_issue(&mut out, issue)?;
            }
            writeln!(out)?;

            // Totals
            let total_sp: f64 = sprint_issues.iter().map(|i| i.story_points).sum();
            let done_sp: f64 = sprint_issues
                .iter()
                .filter(|i| i.status == Status::Done)
                .map(|i| i.story_points)
                .sum();
            writeln!(
                out,
                "_{}sp total · {}sp done · {}sp remaining_\n",
                format_sp(total_sp),
                format_sp(done_sp),
                format_sp(total_sp - done_sp),
            )?;
        }
    }

    // ── Backlog section ────────────────────────────────────────────────────────
    let backlog: Vec<_> = issues
        .iter()
        .filter(|i| {
            let in_sprint = sprint.as_ref().map(|s| i.sprint_id == Some(s.id)).unwrap_or(false);
            !in_sprint && i.parent_id.is_none()
        })
        .collect();

    if !backlog.is_empty() {
        writeln!(out, "# Backlog\n")?;
        write!(out, "{}", header)?;
        for issue in &backlog {
            write_issue(&mut out, issue)?;
        }
    }

    Ok(())
}
