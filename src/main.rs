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
        Some("help") | Some("--help") | Some("-h") => {
            println!("scrumtui — local terminal scrum board");
            println!();
            println!("USAGE:");
            println!("  scrumtui                      open the TUI");
            println!("  scrumtui import <path.csv>    import Jira CSV");
            println!("  scrumtui export [output.md]   export to markdown (default: scrumtui-export.md)");
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
