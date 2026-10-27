mod app;
mod db;
mod export;
mod import;
mod models;
mod seed;
mod ui;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;
use db::Db;
use models::{format_sp, Issue, Status};

/// Resolve the database path using this priority order:
///   1. `--db <path>` (or `--db=<path>`) CLI flag
///   2. `SCRUMTUI_DB` environment variable
///   3. `db_path` key in `$XDG_CONFIG_HOME/scrumtui/config` (or `~/.config/scrumtui/config`)
///   4. Legacy default: `~/.scrumtui.db`
fn resolve_db_path(args: &[String]) -> String {
    // 1. --db flag
    for (i, arg) in args.iter().enumerate() {
        if arg == "--db" {
            if let Some(p) = args.get(i + 1) {
                return p.clone();
            }
        } else if let Some(p) = arg.strip_prefix("--db=") {
            return p.to_string();
        }
    }

    // 2. Environment variable
    if let Ok(v) = std::env::var("SCRUMTUI_DB") {
        if !v.is_empty() {
            return v;
        }
    }

    // 3. Config file
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{home}/.config"));
    let config_path = format!("{config_dir}/scrumtui/config");
    if let Ok(contents) = std::fs::read_to_string(&config_path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() { continue; }
            if let Some(rest) = line.strip_prefix("db_path") {
                let rest = rest.trim_start_matches(|c: char| c == '=' || c == ' ');
                if !rest.is_empty() {
                    return rest.to_string();
                }
            }
        }
    }

    // 4. Legacy default
    format!("{home}/.scrumtui.db")
}

fn main() -> Result<()> {
    // `scrumtui list | head` closes stdout early: exit quietly instead of panicking.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        default_hook(info);
    }));

    // ── CLI subcommands ────────────────────────────────────────────────────────
    let args: Vec<String> = std::env::args().collect();

    // Resolve DB path: --db flag > $SCRUMTUI_DB > config file > ~/.scrumtui.db (legacy)
    let db_path = resolve_db_path(&args);

    // Strip --db <path> / --db=<path> from args so subcommand parsing is unaffected.
    let args: Vec<String> = {
        let mut clean = Vec::new();
        let mut i = 0usize;
        while i < args.len() {
            if args[i] == "--db" {
                i += 2; // skip flag and its value
            } else {
                if !args[i].starts_with("--db=") {
                    clean.push(args[i].clone());
                }
                i += 1;
            }
        }
        clean
    };

    match args.get(1).map(|s| s.as_str()) {
        Some("import") => {
            let csv_path = args.get(2).ok_or_else(|| anyhow!("usage: scrumtui import <path.csv>"))?;
            let db = Db::open(&db_path)?;
            println!("Importing from {csv_path} → {db_path}");
            let report = import::import_jira_csv(&db, csv_path)?;
            println!(
                "Done.  {} issues,  {} subtasks,  {} sprints,  {} rows skipped.",
                report.imported, report.subtasks_imported, report.sprints_created, report.skipped
            );
            Ok(())
        }
        Some("export") => {
            let out_path = args.get(2).map(|s| s.as_str()).unwrap_or("scrumtui-export.md");
            let db = Db::open(&db_path)?;
            export::export_markdown(&db, out_path)?;
            println!("Exported to {out_path}");
            Ok(())
        }
        Some("add") => cmd_add(&db_path, &args[2..]),
        Some("edit") => cmd_edit(&db_path, &args[2..]),
        Some("show") => cmd_show(&db_path, &args[2..]),
        Some("status") => cmd_status(&db_path, &args[2..]),
        Some("list") | Some("ls") => cmd_list(&db_path, &args[2..]),

        // ── scrumtui init [--demo]
        Some("init") => {
            let demo = args.iter().any(|a| a == "--demo");
            let db = Db::open(&db_path)?;
            if demo {
                if !db.is_empty()? {
                    bail!("{db_path} already has data. Delete it first to re-seed.");
                }
                seed::seed(&db)?;
                println!("Initialized {db_path} with demo data.");
            } else {
                println!("Initialized blank database at {db_path}.");
            }
            Ok(())
        }

        // ── scrumtui demo (temporary database with sample data, deleted on exit)
        Some("demo") => {
            let demo_db_path = std::env::temp_dir().join(format!("scrumtui_demo_{}.db", std::process::id()));
            let _ = std::fs::remove_file(&demo_db_path);
            let path = demo_db_path.to_string_lossy().to_string();
            let db = Db::open(&path)?;
            seed::seed(&db)?;
            let res = run_tui(db);
            let _ = std::fs::remove_file(&demo_db_path);
            res
        }

        Some("--version") | Some("-V") | Some("-v") => {
            println!("scrumtui {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }

        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(other) => bail!("unknown command: {other} (see scrumtui --help)"),
        None => run_tui(Db::open(&db_path)?),
    }
}

// ── CLI commands ──────────────────────────────────────────────────────────────

/// Flags shared by `add` and `edit`. `Some(None)` clears an optional field.
#[derive(Default)]
struct IssueArgs {
    title: Option<String>,
    epic: Option<String>,
    points: Option<f64>,
    status: Option<Status>,
    due: Option<Option<NaiveDate>>,
    desc: Option<Option<String>>,
    parent: Option<i64>,
    /// `--sprint` → Some(true), `--backlog` → Some(false)
    sprint: Option<bool>,
}

fn parse_status_arg(s: &str) -> Result<Status> {
    match s.to_lowercase().as_str() {
        "todo" => Ok(Status::Todo),
        "ip" | "in-progress" | "inprogress" | "in_progress" | "doing" => Ok(Status::InProgress),
        "done" => Ok(Status::Done),
        _ => bail!("unknown status {s:?} (use todo, ip or done)"),
    }
}

/// YYYY-MM-DD, or `today` / `tomorrow`. `none` (or "") clears the date.
fn parse_due_arg(s: &str) -> Result<Option<NaiveDate>> {
    let today = chrono::Local::now().date_naive();
    match s.to_lowercase().as_str() {
        "" | "none" => Ok(None),
        "today" => Ok(Some(today)),
        "tomorrow" => Ok(Some(today + chrono::Duration::days(1))),
        _ => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| anyhow!("bad date {s:?} (use YYYY-MM-DD, today, tomorrow or none)")),
    }
}

/// Load an issue, with a plain "no issue #id" when it doesn't exist.
fn find_issue(db: &Db, id: i64) -> Result<Issue> {
    db.get_issue(id).map_err(|e| match e.downcast_ref::<rusqlite::Error>() {
        Some(rusqlite::Error::QueryReturnedNoRows) => anyhow!("no issue #{id}"),
        _ => e,
    })
}

fn parse_id(s: Option<&String>, usage: &str) -> Result<i64> {
    let s = s.ok_or_else(|| anyhow!("usage: {usage}"))?;
    s.trim_start_matches('#').parse().map_err(|_| anyhow!("not an issue id: {s:?}"))
}

fn parse_issue_args(args: &[String]) -> Result<IssueArgs> {
    let mut out = IssueArgs::default();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = || -> Result<&str> {
            args.get(i + 1).map(|v| v.as_str()).ok_or_else(|| anyhow!("{flag} needs a value"))
        };
        let mut takes_value = true;
        match flag {
            "-t" | "--title" => out.title = Some(value()?.to_string()),
            "-e" | "--epic" => out.epic = Some(value()?.trim().to_string()),
            "-p" | "--points" => {
                let v = value()?;
                let p: f64 = v.parse().map_err(|_| anyhow!("--points: not a number: {v:?}"))?;
                if p <= 0.0 {
                    bail!("--points must be positive");
                }
                out.points = Some(p);
            }
            "-s" | "--status" => out.status = Some(parse_status_arg(value()?)?),
            "-d" | "--due" => out.due = Some(parse_due_arg(value()?)?),
            "-m" | "--desc" | "--description" => {
                let v = value()?.trim();
                out.desc = Some(if v.is_empty() { None } else { Some(v.to_string()) });
            }
            "--parent" => out.parent = Some(parse_id(args.get(i + 1), "--parent <id>")?),
            "--sprint" => { out.sprint = Some(true); takes_value = false; }
            "--backlog" | "--no-sprint" => { out.sprint = Some(false); takes_value = false; }
            other => bail!("unknown flag {other:?} (see scrumtui --help)"),
        }
        i += if takes_value { 2 } else { 1 };
    }
    Ok(out)
}

/// Put an issue into the active sprint (or take it out). Returns a note for the user.
fn apply_sprint_flag(db: &Db, id: i64, sprint: Option<bool>) -> Result<String> {
    match sprint {
        Some(true) => match db.get_active_sprint()? {
            Some(s) => {
                db.set_issue_sprint(id, Some(s.id))?;
                Ok(format!(" in sprint {:?}", s.name))
            }
            None => Ok(" (no active sprint: left in the backlog)".to_string()),
        },
        Some(false) => {
            db.set_issue_sprint(id, None)?;
            Ok(" (in the backlog)".to_string())
        }
        None => Ok(String::new()),
    }
}

/// `scrumtui add "Title" [-e epic] [-p points] [-s status] [-d due] [-m desc] [--parent ID] [--sprint]`
fn cmd_add(db_path: &str, args: &[String]) -> Result<()> {
    let usage = "scrumtui add \"Title\" [-e epic] [-p points] [-s todo|ip|done] [-d YYYY-MM-DD] [-m description] [--parent ID] [--sprint]";
    let title = args
        .first()
        .filter(|t| !t.starts_with('-') && !t.trim().is_empty())
        .ok_or_else(|| anyhow!("usage: {usage}"))?
        .trim()
        .to_string();
    let flags = parse_issue_args(&args[1..])?;
    let status = flags.status.clone().unwrap_or(Status::Todo);
    let db = Db::open(db_path)?;

    if let Some(parent_id) = flags.parent {
        let parent = find_issue(&db, parent_id)?;
        if parent.parent_id.is_some() {
            bail!("#{parent_id} is a subtask itself; subtasks can't be nested");
        }
        let id = db.create_subtask(&title, parent_id, &status)?;
        db.update_parent_status_from_children(parent_id)?;
        println!("Created subtask #{id} {title:?} under #{parent_id} {:?}", parent.title);
        return Ok(());
    }

    let epic = flags.epic.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| "general".to_string());
    let due = flags.due.flatten();
    let desc = flags.desc.clone().flatten();
    let id = db.create_issue(&title, flags.points.unwrap_or(1.0), &epic, &status, due, desc.as_deref())?;
    let note = apply_sprint_flag(&db, id, flags.sprint)?;
    println!("Created #{id} {title:?}{note}");
    Ok(())
}

/// `scrumtui edit ID [-t title] [-e epic] [-p points] [-s status] [-d due|none] [-m desc] [--sprint|--backlog]`
fn cmd_edit(db_path: &str, args: &[String]) -> Result<()> {
    let id = parse_id(args.first(), "scrumtui edit <id> [flags]")?;
    let flags = parse_issue_args(&args[1..])?;
    if flags.parent.is_some() {
        bail!("--parent only works with add");
    }
    let db = Db::open(db_path)?;
    let issue = find_issue(&db, id)?;
    let has_subtasks = db.get_all_issues()?.iter().any(|i| i.parent_id == Some(id));
    if flags.status.is_some() && has_subtasks {
        bail!("the status of #{id} follows its subtasks; change those instead");
    }

    let title = flags.title.clone().unwrap_or_else(|| issue.title.clone());
    let epic = flags.epic.clone().unwrap_or_else(|| issue.epic.clone());
    let points = flags.points.unwrap_or(issue.story_points);
    let status = flags.status.clone().unwrap_or_else(|| issue.status.clone());
    let due = flags.due.unwrap_or(issue.due_date);
    let desc = flags.desc.clone().unwrap_or_else(|| issue.description.clone());
    db.update_issue(id, &title, points, &epic, &status, due, desc.as_deref(), issue.parent_id)?;
    if let Some(pid) = issue.parent_id {
        db.update_parent_status_from_children(pid)?;
    }
    let note = apply_sprint_flag(&db, id, flags.sprint)?;
    println!("Updated #{id} {title:?}{note}");
    Ok(())
}

/// `scrumtui show ID`: every field of an issue, plus its subtasks.
fn cmd_show(db_path: &str, args: &[String]) -> Result<()> {
    let id = parse_id(args.first(), "scrumtui show <id>")?;
    let db = Db::open(db_path)?;
    let issue = find_issue(&db, id)?;
    let all = db.get_all_issues()?;
    let sprint = db.get_active_sprint()?;
    let fmt = |d: chrono::NaiveDateTime| d.format("%Y-%m-%d %H:%M").to_string();

    println!("#{}  {}", issue.id, issue.title);
    if let Some(pid) = issue.parent_id {
        println!("subtask of #{pid}");
    }
    println!("status    {}", issue.status.label());
    if issue.parent_id.is_none() {
        println!("epic      {}", issue.epic);
        println!("points    {}", format_sp(issue.story_points));
        let where_ = match (&sprint, issue.sprint_id) {
            (Some(s), Some(sid)) if s.id == sid => format!("active sprint ({})", s.name),
            (_, Some(_)) => "an earlier sprint".to_string(),
            _ => "backlog".to_string(),
        };
        println!("in        {where_}");
    }
    if let Some(d) = issue.due_date {
        println!("due       {d}");
    }
    if issue.carry_count > 0 {
        println!("carried   {}×", issue.carry_count);
    }
    println!("created   {}", fmt(issue.created_at));
    if let Some(s) = issue.started_at {
        println!("started   {}", fmt(s));
    }
    if let Some(c) = issue.completed_at {
        println!("done      {}", fmt(c));
    }
    if let Some(desc) = issue.description.as_deref().filter(|d| !d.trim().is_empty()) {
        println!("\n{}", desc.trim());
    }
    let mut subs: Vec<&Issue> = all.iter().filter(|i| i.parent_id == Some(id)).collect();
    subs.reverse(); // creation order
    if !subs.is_empty() {
        println!();
        for s in subs {
            let mark = match s.status {
                Status::Done => "x",
                Status::InProgress => "~",
                Status::Todo => " ",
            };
            println!("  [{mark}] #{} {}", s.id, s.title);
        }
    }
    Ok(())
}

/// `scrumtui status ID todo|ip|done`
fn cmd_status(db_path: &str, args: &[String]) -> Result<()> {
    let usage = "scrumtui status <id> <todo|ip|done>";
    let id = parse_id(args.first(), usage)?;
    let new_status = parse_status_arg(args.get(1).ok_or_else(|| anyhow!("usage: {usage}"))?)?;
    let db = Db::open(db_path)?;
    let issue = find_issue(&db, id)?;
    if db.get_all_issues()?.iter().any(|i| i.parent_id == Some(id)) {
        bail!("the status of #{id} follows its subtasks; change those instead");
    }
    db.update_issue_status(id, &new_status)?;
    println!("#{id} {:?}: {} → {}", issue.title, issue.status.label(), new_status.label());
    Ok(())
}

/// `scrumtui list [--all] [--sprint] [-s status] [-e epic]`
fn cmd_list(db_path: &str, args: &[String]) -> Result<()> {
    let mut show_all = false;
    let mut sprint_only = false;
    let mut status_filter: Option<Status> = None;
    let mut epic_filter: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" | "-a" => show_all = true,
            "--sprint" => sprint_only = true,
            "-s" | "--status" => {
                status_filter = Some(parse_status_arg(args.get(i + 1).ok_or_else(|| anyhow!("--status needs a value"))?)?);
                i += 1;
            }
            "-e" | "--epic" => {
                epic_filter = Some(args.get(i + 1).ok_or_else(|| anyhow!("--epic needs a value"))?.to_lowercase());
                i += 1;
            }
            other => bail!("unknown flag {other:?} for list"),
        }
        i += 1;
    }
    // Asking for done issues implies showing them.
    if status_filter == Some(Status::Done) {
        show_all = true;
    }

    let db = Db::open(db_path)?;
    let issues = db.get_all_issues()?;
    let active_sprint = db.get_active_sprint()?;
    let in_sprint = |i: &Issue| active_sprint.as_ref().is_some_and(|s| i.sprint_id == Some(s.id));
    if sprint_only && active_sprint.is_none() {
        println!("No active sprint.");
        return Ok(());
    }

    let filtered: Vec<&Issue> = issues
        .iter()
        .filter(|i| i.parent_id.is_none())
        .filter(|i| show_all || i.status != Status::Done)
        .filter(|i| !sprint_only || in_sprint(i))
        .filter(|i| status_filter.as_ref().map_or(true, |s| &i.status == s))
        .filter(|i| epic_filter.as_ref().map_or(true, |e| i.epic.to_lowercase() == *e))
        .collect();

    if filtered.is_empty() {
        println!("No issues.");
        return Ok(());
    }

    println!("{:<6} {:>4}  {:<11}  {:<14}  {:<10}  TITLE", "ID", "SP", "STATUS", "EPIC", "DUE");
    for issue in filtered {
        // * = in the active sprint
        let marker = if in_sprint(issue) { "*" } else { " " };
        println!(
            "{:<6} {:>4}  {:<11}  {:<14}  {:<10}  {}",
            format!("{marker}{}", issue.id),
            format_sp(issue.story_points),
            issue.status.label(),
            trunc_str(&issue.epic, 14),
            issue.due_date_str(),
            issue.title,
        );
    }
    if active_sprint.is_some() && !sprint_only {
        println!("\n* = in the active sprint");
    }
    Ok(())
}

fn trunc_str(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max.saturating_sub(1)].iter().collect::<String>())
    }
}

fn print_help() {
    println!("scrumtui {}: a local terminal scrum board", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USAGE:");
    println!("  scrumtui [--db <path>]               open the TUI");
    println!("  scrumtui demo                        try it on a throwaway database with sample data");
    println!("  scrumtui init [--demo]               create a database (optionally with sample data)");
    println!("  scrumtui add \"Title\" [flags]         create an issue (or a subtask with --parent)");
    println!("  scrumtui edit <id> [flags]           change fields of an issue");
    println!("  scrumtui show <id>                   print an issue with its description and subtasks");
    println!("  scrumtui status <id> <todo|ip|done>  change an issue's status");
    println!("  scrumtui list [flags]                list open issues (* = in the active sprint)");
    println!("  scrumtui import <path.csv>           import a Jira CSV export");
    println!("  scrumtui export [output.md]          export to markdown");
    println!("  scrumtui --version                   print version");
    println!();
    println!("ADD / EDIT FLAGS:");
    println!("  -t, --title <text>      new title (edit)");
    println!("  -e, --epic <name>       epic (add default: general)");
    println!("  -p, --points <n>        story points (add default: 1)");
    println!("  -s, --status <s>        todo | ip | done (add default: todo)");
    println!("  -d, --due <date>        YYYY-MM-DD, today, tomorrow, or none to clear");
    println!("  -m, --desc <text>       description (\"\" clears it)");
    println!("  --parent <id>           add as a subtask of this issue (add)");
    println!("  --sprint                put it in the active sprint");
    println!("  --backlog               take it out of the sprint (edit)");
    println!();
    println!("LIST FLAGS:");
    println!("  -a, --all               include done issues");
    println!("  --sprint                only the active sprint");
    println!("  -s, --status <s>        only this status");
    println!("  -e, --epic <name>       only this epic");
    println!();
    println!("DATABASE PATH (in priority order):");
    println!("  --db <path>                          explicit path flag");
    println!("  $SCRUMTUI_DB                         environment variable");
    println!("  ~/.config/scrumtui/config            config file (db_path = /path/to/file.db)");
    println!("  ~/.scrumtui.db                       default");
    println!();
    println!("A running TUI picks up changes made from the command line within a second.");
}

// ── TUI ───────────────────────────────────────────────────────────────────────

fn run_tui(db: Db) -> Result<()> {
    let mut app = App::new(db)?;
    // Ensure selection starts on a valid issue
    app.backlog_sel_to_first_issue();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Install a panic hook that restores the terminal before printing the
    // panic message, so the shell is never left in raw/alternate-screen mode.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    // Register a quit flag that is set by SIGTERM, SIGHUP, or SIGINT.
    // SIGHUP fires when the terminal window is closed.
    // SIGINT fires for Ctrl-C at the OS level (crossterm also handles it as a
    // key event, but we want the signal path too).
    let quit_flag = Arc::new(AtomicBool::new(false));
    for sig in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(sig, Arc::clone(&quit_flag))
            .expect("failed to register signal handler");
    }

    let res = run_app(&mut terminal, &mut app, quit_flag);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    quit_flag: Arc<AtomicBool>,
) -> Result<()> {
    // Draw once on startup
    terminal.draw(|f| ui::render(f, app))?;

    loop {
        // Exit immediately if a signal was received.
        if quit_flag.load(Ordering::Relaxed) {
            break;
        }

        // Use a short timeout while a status message is on screen so it can
        // expire and be erased.  Otherwise use a 1-second idle timeout:
        // near-zero CPU cost, but ensures we check quit_flag (set by
        // SIGTERM/SIGHUP) and external database changes within 1 s.
        let timeout = if app.current_status().is_some() {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(1)
        };

        let had_event = match event::poll(timeout) {
            Ok(v) => v,
            // poll() returns Err when stdin is closed / terminal detached.
            // Exit cleanly instead of spinning.
            Err(_) => break,
        };

        // Check signal flag again after waking.
        if quit_flag.load(Ordering::Relaxed) {
            break;
        }

        if had_event {
            let ev = match event::read() {
                Ok(e) => e,
                Err(_) => break, // stdin closed
            };
            match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.handle_key(key) {
                        break;
                    }
                }
                Event::Resize(_, _) => {} // fall through to redraw
                _ => continue,            // focus/mouse/paste: no state change, skip redraw
            }
        }

        // Issues added from the command line (by you, a script or an assistant)
        // show up without restarting.
        app.sync_external_changes();
        terminal.draw(|f| ui::render(f, app))?;
    }
    Ok(())
}
