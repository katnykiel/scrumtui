//! Renders the demo database into HTML "screenshots" (used for the README images).
//!
//! ```sh
//! SCRUMTUI_SNAPSHOT_DIR=/some/dir cargo test render_readme_snapshots -- --ignored
//! firefox --headless --screenshot backlog.png --window-size=2400,1600 file:///some/dir/backlog.html
//! ```
//!
//! The pages are drawn at 2× (`zoom: 2`), so the PNGs stay sharp on high-DPI screens;
//! crop the white margin afterwards.
//!
//! ```text
//! ```

use std::fmt::Write as _;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, style::{Color, Modifier}, Terminal};

use crate::app::App;
use crate::db::Db;
use crate::seed;

// Small enough that the text stays readable when GitHub scales the image to page width.
const COLS: u16 = 88;
const ROWS: u16 = 28;
// A light terminal theme.
const BG: &str = "#ffffff";
const FG: &str = "#1f2328";

fn css(color: Color, default: &str) -> String {
    let named = match color {
        Color::Reset => default,
        Color::Black => "#1f2328",
        Color::Red => "#c62f37",
        Color::Green => "#2f8a3b",
        Color::Yellow => "#a17200",
        Color::Blue => "#1f5fbf",
        Color::Magenta => "#9b3fb5",
        Color::Cyan => "#1a7f99",
        Color::Gray => "#8b939c",
        Color::DarkGray => "#6e7781",
        Color::LightRed => "#d4473f",
        Color::LightGreen => "#3fa34d",
        Color::LightYellow => "#b8860b",
        Color::LightBlue => "#3a7bd5",
        Color::LightMagenta => "#b25ccf",
        Color::LightCyan => "#2b95b0",
        Color::White => "#c9ced4",
        Color::Rgb(r, g, b) => return format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(_) => default,
    };
    named.to_string()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The buffer as an HTML page that looks like a terminal window.
fn to_html(buf: &Buffer, title: &str) -> String {
    let mut rows = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        let mut current: Option<String> = None;
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            let mut fg = css(cell.fg, FG);
            let mut bg = css(cell.bg, BG);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let mut style = format!("color:{fg};background:{bg}");
            if cell.modifier.contains(Modifier::BOLD) {
                style.push_str(";font-weight:bold");
            }
            if current.as_deref() != Some(style.as_str()) {
                if current.is_some() {
                    line.push_str("</span>");
                }
                let _ = write!(line, "<span style=\"{style}\">");
                current = Some(style);
            }
            let sym = cell.symbol();
            if sym.is_ascii() {
                line.push_str(&escape(sym));
            } else {
                // Pin non-ASCII glyphs to one cell so font fallback can't shift the row.
                let _ = write!(line, "<i>{}</i>", escape(sym));
            }
        }
        if current.is_some() {
            line.push_str("</span>");
        }
        rows.push_str(&line);
        rows.push('\n');
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{title}</title><style>
body {{ margin: 0; background: #ffffff; overflow: hidden; zoom: 2; }}
.win {{ display: inline-block; margin: 28px; border-radius: 10px; overflow: hidden;
        border: 1px solid #d0d7de; box-shadow: 0 8px 28px rgba(0,0,0,.16); background: {BG}; }}
.bar {{ height: 30px; background: #ececef; border-bottom: 1px solid #d8d8dc; display: flex;
        align-items: center; gap: 8px; padding: 0 14px; font: 13px sans-serif; color: #57606a; }}
.dot {{ width: 12px; height: 12px; border-radius: 50%; }}
.name {{ flex: 1; text-align: center; margin-right: 52px; }}
pre {{ margin: 0; padding: 8px 10px 10px; font-family: 'DejaVu Sans Mono', monospace;
       font-size: 14px; line-height: 17px; color: {FG}; }}
i {{ font-style: inherit; display: inline-block; width: 1ch; overflow: visible; }}
</style></head><body><div class="win"><div class="bar">
<span class="dot" style="background:#ff5f57"></span><span class="dot" style="background:#febc2e"></span>
<span class="dot" style="background:#28c840"></span><span class="name">scrumtui</span></div>
<pre>{rows}</pre></div></body></html>"#
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Popups and dropdowns stay inside small screens (drawing outside the buffer panics).
#[test]
fn popups_fit_small_screens() {
    let n_tab = |tabs: usize| {
        let mut keys = vec![KeyCode::Char('n'), KeyCode::Char('x')];
        keys.extend(std::iter::repeat(KeyCode::Tab).take(tabs));
        keys
    };
    let mut epic_dropdown = n_tab(1);
    epic_dropdown.push(KeyCode::Char('a'));
    let cases = [
        vec![KeyCode::Char('?')],
        vec![KeyCode::Char('T')],
        vec![KeyCode::Char('S')],
        vec![KeyCode::Char('3'), KeyCode::Char('e')],
        n_tab(4), // due date dropdown
        epic_dropdown,
    ];
    for (w, h) in [(80, 24), (60, 16), (40, 12), (20, 5)] {
        for keys in &cases {
            let db = Db::open(":memory:").unwrap();
            seed::seed(&db).unwrap();
            let mut app = App::new(db).unwrap();
            for k in keys {
                app.handle_key(key(*k));
            }
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        }
    }
}

#[test]
#[ignore = "writes README screenshots; run with SCRUMTUI_SNAPSHOT_DIR set"]
fn render_readme_snapshots() {
    let dir = std::env::var("SCRUMTUI_SNAPSHOT_DIR").expect("set SCRUMTUI_SNAPSHOT_DIR");
    let db = Db::open(":memory:").unwrap();
    seed::seed(&db).unwrap();
    let mut app = App::new(db).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();

    let mut shoot = |app: &mut App, name: &str, keys: &[KeyCode]| {
        for k in keys {
            app.handle_key(key(*k));
        }
        terminal.draw(|f| crate::ui::render(f, app)).unwrap();
        let html = to_html(terminal.backend().buffer(), name);
        std::fs::write(format!("{dir}/{name}.html"), html).unwrap();
    };

    // Backlog with the frog selected (and its startup hint in the status line).
    app.backlog_sel_to_first_issue();
    shoot(&mut app, "backlog", &[]);
    app.status_msg = None;
    shoot(&mut app, "kanban", &[KeyCode::Char('2'), KeyCode::Char('l')]);
    shoot(&mut app, "gantt", &[KeyCode::Char('3')]);
    // Last week's sprint: carry-overs, scope changes and the epic breakdown.
    shoot(&mut app, "history", &[KeyCode::Char('4'), KeyCode::Char('j')]);
    shoot(&mut app, "help", &[KeyCode::Char('?')]);
}
