pub mod backlog;
pub mod gantt;
pub mod kanban;
pub mod popup;

use ratatui::Frame;

use crate::app::{App, View};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    match &app.view {
        View::Backlog => backlog::render(f, app, area),
        View::Kanban => kanban::render(f, app, area),
        View::Gantt => gantt::render(f, app, area),
    }

    // Overlay popup if one is active. Clone to avoid double-borrow of app.
    if let Some(p) = app.popup.clone() {
        popup::render(f, &p, app);
    }
}
