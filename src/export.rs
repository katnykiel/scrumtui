use anyhow::{Context, Result};
use std::io::Write;

use crate::db::Db;
use crate::models::{format_sp, Status};

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

    let header = "| Status | Title | SP | Epic | Due |\n\
                  |--------|-------|----|------|-----|\n";

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
                writeln!(
                    out,
                    "| {} {} | {} | {} | {} | {} |",
                    sym(&issue.status),
                    issue.status.label(),
                    issue.title,
                    format_sp(issue.story_points),
                    issue.epic,
                    due_str(&issue.due_date),
                )?;
                // Children
                let children: Vec<_> = issues.iter().filter(|i| i.parent_id == Some(issue.id)).collect();
                for child in children {
                    writeln!(
                        out,
                        "| {} {} | ↳ {} | {} | {} | {} |",
                        sym(&child.status),
                        child.status.label(),
                        child.title,
                        format_sp(child.story_points),
                        child.epic,
                        due_str(&child.due_date),
                    )?;
                }
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
            writeln!(
                out,
                "| {} {} | {} | {} | {} | {} |",
                sym(&issue.status),
                issue.status.label(),
                issue.title,
                format_sp(issue.story_points),
                issue.epic,
                due_str(&issue.due_date),
            )?;
            let children: Vec<_> = issues.iter().filter(|i| i.parent_id == Some(issue.id)).collect();
            for child in children {
                writeln!(
                    out,
                    "| {} {} | ↳ {} | {} | {} | {} |",
                    sym(&child.status),
                    child.status.label(),
                    child.title,
                    format_sp(child.story_points),
                    child.epic,
                    due_str(&child.due_date),
                )?;
            }
        }
    }

    Ok(())
}
