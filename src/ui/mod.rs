pub mod backlog;
pub mod gantt;
pub mod history;
pub mod kanban;
pub mod popup;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, View};

pub fn render(f: &mut Frame, app: &mut App) {
    let full = f.area();

    // Reserve 1 row at the bottom for the status bar when a message is active.
    let msg = app.current_status().map(|s| s.to_string());
    let (main_area, status_area) = if msg.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(full);
        (chunks[0], Some(chunks[1]))
    } else {
        (full, None)
    };

    match &app.view {
        View::Backlog => backlog::render(f, app, main_area),
        View::Kanban => kanban::render(f, app, main_area),
        View::Gantt => gantt::render(f, app, main_area),
        View::SprintHistory => history::render(f, app, main_area),
    }

    // Status bar
    if let (Some(area), Some(text)) = (status_area, msg) {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled(text, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ])),
            area,
        );
    }

    // Overlay popup if one is active. Clone to avoid double-borrow of app.
    if let Some(p) = app.popup.clone() {
        popup::render(f, &p, app);
    }
}
