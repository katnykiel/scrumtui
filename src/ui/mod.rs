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

    // Overlay popup if one is active.
    // Take the popup out of app temporarily so we can borrow both without cloning.
    if let Some(p) = app.popup.take() {
        match &p {
            crate::app::Popup::GanttEpicDetail { .. } => {
                gantt::render_epic_detail_popup(f, &p);
            }
            _ => popup::render(f, &p, app),
        }
        // Restore the popup (only if the key handler hasn't cleared it in the meantime —
        // but render is called before key handling, so it will always be None at this point
        // unless render itself clears it, which it doesn't).
        app.popup = Some(p);
    }
}
