mod app;
mod db;
mod export;
mod import;
mod models;
mod seed;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;
use db::Db;
use models::Status;

fn main() -> Result<()> {
    let db_path = {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{home}/.scrumtui.db")
    };

    // ── CLI subcommands ────────────────────────────────────────────────────────
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("import") => {
            let csv_path = args.get(2).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Usage: scrumtui import <path.csv>");
                std::process::exit(1);
            });
            let db = Db::open(&db_path)?;
            println!("Importing from {csv_path} → {db_path}");
            let report = import::import_jira_csv(&db, csv_path)?;
            println!("Done.  {} issues imported,  {} rows skipped.", report.imported, report.skipped);
            return Ok(());
        }
        Some("export") => {
            let out_path = args.get(2).map(|s| s.as_str()).unwrap_or("scrumtui-export.md");
            let db = Db::open(&db_path)?;
            export::export_markdown(&db, out_path)?;
            println!("Exported to {out_path}");
            return Ok(());
        }

        // ── scrumtui add "Title" [-e epic] [-p points] [-s status] [-d YYYY-MM-DD] [--sprint]
        Some("add") => {
            let db = Db::open(&db_path)?;
            let title = args.get(2).cloned().unwrap_or_else(|| {
                eprintln!("Usage: scrumtui add \"Issue title\" [-e epic] [-p points] [-s todo|ip|done] [-d YYYY-MM-DD] [--sprint]");
                std::process::exit(1);
            });
            let mut epic = String::from("general");
            let mut points: f64 = 1.0;
            let mut status = Status::Todo;
            let mut due_date: Option<chrono::NaiveDate> = None;
            let mut add_to_sprint = false;

            let mut i = 3usize;
            while i < args.len() {
                match args[i].as_str() {
                    "-e" | "--epic" => {
                        epic = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "-p" | "--points" => {
                        points = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1.0);
                        i += 2;
                    }
                    "-s" | "--status" => {
                        status = match args.get(i + 1).map(|s| s.to_lowercase()).as_deref() {
                            Some("ip") | Some("in-progress") | Some("inprogress") => Status::InProgress,
                            Some("done") => Status::Done,
                            _ => Status::Todo,
                        };
                        i += 2;
                    }
                    "-d" | "--due" => {
                        due_date = args.get(i + 1).and_then(|s| {
                            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
                        });
                        i += 2;
                    }
                    "--sprint" => {
                        add_to_sprint = true;
                        i += 1;
                    }
                    _ => { i += 1; }
                }
            }

            let id = db.create_issue(&title, points, &epic, &status, due_date, None)?;

            if add_to_sprint {
                if let Some(sprint) = db.get_active_sprint()? {
                    db.set_issue_sprint(id, Some(sprint.id))?;
                    println!("Created issue #{id}: {title:?}  (added to sprint \"{}\")", sprint.name);
                } else {
                    println!("Created issue #{id}: {title:?}  (no active sprint — use scrumtui to create one)");
                }
            } else {
                println!("Created issue #{id}: {title:?}");
            }
            return Ok(());
        }

        // ── scrumtui status <id> <todo|ip|done>
        Some("status") => {
            let id: i64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                eprintln!("Usage: scrumtui status <id> <todo|ip|done>");
                std::process::exit(1);
            });
            let new_status = match args.get(3).map(|s| s.to_lowercase()).as_deref() {
                Some("ip") | Some("in-progress") | Some("inprogress") => Status::InProgress,
                Some("done") => Status::Done,
                Some("todo") => Status::Todo,
                _ => {
                    eprintln!("Usage: scrumtui status <id> <todo|ip|done>");
                    std::process::exit(1);
                }
            };
            let db = Db::open(&db_path)?;
            let issue = db.get_issue(id)?;
            db.update_issue_status(id, &new_status)?;
            println!("Issue #{id} \"{}\": {} → {}", issue.title, issue.status.label(), new_status.label());
            return Ok(());
        }

        // ── scrumtui list [--all] [--sprint] [--status todo|ip|done]
        Some("list") => {
            let db = Db::open(&db_path)?;
            let show_all = args.iter().any(|a| a == "--all");
            let sprint_only = args.iter().any(|a| a == "--sprint");
            let status_filter: Option<Status> = args.windows(2).find_map(|w| {
                if w[0] == "--status" || w[0] == "-s" {
                    match w[1].to_lowercase().as_str() {
                        "ip" | "in-progress" => Some(Status::InProgress),
                        "done" => Some(Status::Done),
                        "todo" => Some(Status::Todo),
                        _ => None,
                    }
                } else { None }
            });

            let issues = db.get_all_issues()?;
            let active_sprint = db.get_active_sprint()?;

            let filtered: Vec<_> = issues.iter().filter(|i| {
                if i.parent_id.is_some() { return false; } // skip subtasks
                if !show_all && i.status == Status::Done { return false; }
                if sprint_only {
                    if let Some(ref s) = active_sprint {
                        if i.sprint_id != Some(s.id) { return false; }
                    } else {
                        return false;
                    }
                }
                if let Some(ref sf) = status_filter {
                    if &i.status != sf { return false; }
                }
                true
            }).collect();

            if filtered.is_empty() {
                println!("No issues.");
                return Ok(());
            }

            // Print a simple table
            println!("{:<5}  {:<3}  {:<8}  {:<14}  {}", "ID", "SP", "STATUS", "EPIC", "TITLE");
            println!("{}", "─".repeat(72));
            for issue in filtered {
                println!(
                    "{:<5}  {:>3}  {:<8}  {:<14}  {}",
                    issue.id,
                    models::format_sp(issue.story_points),
                    issue.status.label(),
                    trunc_str(&issue.epic, 14),
                    trunc_str(&issue.title, 40),
                );
            }
            return Ok(());
        }

        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            return Ok(());
        }
        _ => {}
    }

    let db = Db::open(&db_path)?;

    // Auto-seed if the database is empty
    if db.is_empty()? {
        seed::seed(&db)?;
    }

    let mut app = App::new(db)?;

    // Ensure selection starts on a valid issue
    app.backlog_sel_to_first_issue();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {err:?}");
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
    println!("scrumtui — local terminal scrum board");
    println!();
    println!("USAGE:");
    println!("  scrumtui                           open the TUI");
    println!("  scrumtui add \"Title\" [flags]       create a new issue");
    println!("  scrumtui status <id> <todo|ip|done> change an issue's status");
    println!("  scrumtui list [flags]               list issues");
    println!("  scrumtui import <path.csv>          import Jira CSV");
    println!("  scrumtui export [output.md]         export to markdown");
    println!();
    println!("ADD FLAGS:");
    println!("  -e, --epic <name>      epic label (default: general)");
    println!("  -p, --points <n>       story points (default: 1)");
    println!("  -s, --status <s>       initial status: todo | ip | done (default: todo)");
    println!("  -d, --due <YYYY-MM-DD> due date");
    println!("  --sprint               add to active sprint");
    println!();
    println!("LIST FLAGS:");
    println!("  --all                  include done issues");
    println!("  --sprint               only show active sprint issues");
    println!("  -s, --status <s>       filter by status: todo | ip | done");
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.handle_key(key) {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
