use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};
use crate::ui::{detail_height, due_color, hint, render_issue_detail, word_wrap, BORDER, BORDER_DIM, FROG};

const STATUSES: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let has_subs = app.sprint_has_any_subtasks();

    let hint_text = if app.kanban_panel == 1 {
        "[n] add subtask  [e] edit  [^H/^L] move  [h/l] parent  [Tab] cards  [u] undo  [?] help"
    } else if has_subs {
        "[n] new  [e] edit  [h/l] column  [^H/^L] move  [Tab] subtasks  [u] undo  [?] help"
    } else {
        "[n] new  [e] edit  [h/l] column  [^H/^L] move  [f] frog  [u] undo  [?] help"
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title_bottom(hint(hint_text))
        .border_style(Style::default().fg(BORDER));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if app.active_sprint.is_none() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No active sprint. Press S to start one, then s on backlog issues (1) to pull them in.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
            inner,
        );
        return;
    }

    let selected = app.kanban_selected_issue();
    let detail_h = detail_height(app, selected.as_ref(), inner.width, (inner.height / 3).max(5));
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(detail_h)])
        .split(inner);

    let board_area = v_chunks[0];
    if has_subs {
        let sub_count = app.sprint_subtasks_flat().len();
        let sub_h = ((sub_count + 2) as u16).max(4).min(board_area.height / 2);
        let panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(sub_h)])
            .split(board_area);
        render_board(f, app, panels[0]);
        render_subtask_panel(f, app, panels[1]);
    } else {
        render_board(f, app, board_area);
    }

    render_issue_detail(f, app, selected.as_ref(), v_chunks[1]);
}

fn render_board(f: &mut Frame, app: &mut App, area: Rect) {
    let panel_focused = app.kanban_panel == 0;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)])
        .split(area);

    for (col_idx, status) in STATUSES.iter().enumerate() {
        render_column(f, app, cols[col_idx], col_idx, status, panel_focused);
    }
}

/// Stacked vertical list of subtasks for the focused parent.
fn render_subtask_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.kanban_panel == 1;
    let parents = app.sprint_parents_with_subtasks();
    let n = parents.len();
    let idx = app.kanban_sub_parent_idx.min(n.saturating_sub(1));
    let parent_name = parents.get(idx).map(|p| p.title.clone()).unwrap_or_default();

    let border_color = if is_focused { Color::Magenta } else { BORDER_DIM };
    let title_style = if is_focused {
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let parent_label = if n > 1 {
        format!(" subtasks: {} ({}/{}) ", parent_name, idx + 1, n)
    } else {
        format!(" subtasks: {} ", parent_name)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(Span::styled(parent_label, title_style)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let subtasks = app.sprint_subtasks_flat();
    let sel_row = app.kanban_sub_row;
    let col_w = inner.width as usize;

    let items: Vec<ListItem> = subtasks.iter().enumerate().map(|(row, issue)| {
        let is_sel = is_focused && row == sel_row;
        let is_done = issue.status == Status::Done;
        let sc = status_color(&issue.status);
        let status_badge = format!("[{}]", issue.status.short().trim());
        let title_style = match (is_sel, is_done) {
            (true, _) => Style::default().add_modifier(Modifier::REVERSED),
            (false, true) => Style::default().fg(Color::DarkGray),
            (false, false) => Style::default(),
        };
        let title_max = col_w.saturating_sub(5 + status_badge.len());
        ListItem::new(Line::from(vec![
            Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(Color::Magenta)),
            Span::styled(format!("{} ", status_symbol(&issue.status)), Style::default().fg(sc)),
            Span::styled(format!("{:<title_max$}", trunc(&issue.title, title_max)), title_style),
            Span::raw(" "),
            Span::styled(status_badge, Style::default().fg(sc)),
        ]))
    }).collect();

    let mut state = ListState::default();
    if is_focused && !subtasks.is_empty() {
        state.select(Some(sel_row.min(subtasks.len() - 1)));
    }
    f.render_stateful_widget(List::new(items), inner, &mut state);
}

fn render_column(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    col_idx: usize,
    status: &Status,
    panel_focused: bool,
) {
    let is_active_col = app.kanban_col == col_idx && panel_focused;
    let sc = status_color(status);
    let issues: Vec<Issue> = app.sprint_parents_by_status(status);
    let selected_row = app.kanban_rows[col_idx];
    let border_color = if is_active_col { sc } else { BORDER_DIM };

    let col_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    // ── Header ──────────────────────────────────────────────────────────────
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let sp_str = if total_sp > 0.0 { format!(" · {}sp", format_sp(total_sp)) } else { String::new() };
    let header_text = format!(" {} {} · {}{}", status_symbol(status), status.label(), issues.len(), sp_str);
    let header_style = if is_active_col {
        Style::default().fg(sc).add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(sc).add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(header_text, header_style)))
            .block(Block::default()
                .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))),
        col_chunks[0],
    );

    // ── Cards ────────────────────────────────────────────────────────────────
    // Text width inside the borders, minus the pointer column.
    let title_w = (area.width as usize).saturating_sub(5).max(4);
    let items: Vec<ListItem> = issues
        .iter()
        .enumerate()
        .map(|(row, issue)| {
            let is_sel = is_active_col && row == selected_row;
            let is_done = issue.status == Status::Done;
            // Cards are bold; done cards are faded. The selected card's title is reversed.
            let title_style = match (is_sel, is_done) {
                (true, _) => Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                (false, true) => Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                (false, false) => Style::default().add_modifier(Modifier::BOLD),
            };
            let mut lines: Vec<Line> = word_wrap(&issue.title, title_w)
                .into_iter()
                .enumerate()
                .map(|(i, chunk)| {
                    let lead = if i == 0 && is_sel { "▶ " } else { "  " };
                    Line::from(vec![
                        Span::styled(lead, Style::default().fg(Color::Magenta)),
                        Span::styled(chunk, title_style),
                    ])
                })
                .collect();

            // Most important first: a narrow column cuts the end of this line.
            let dim = Style::default().fg(Color::DarkGray);
            let mut meta = vec![Span::styled(format!("  {}sp", format_sp(issue.story_points)), dim)];
            if app.frog_id == Some(issue.id) {
                meta.push(Span::styled(" frog", Style::default().fg(FROG).add_modifier(Modifier::BOLD)));
            }
            if issue.carry_count > 0 {
                meta.push(Span::styled(
                    format!(" ↩{}", issue.carry_count),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
            if let Some(d) = issue.due_date {
                meta.push(Span::styled(
                    format!(" · {}", d.format("%b %d")),
                    Style::default().fg(due_color(d, is_done)),
                ));
            }
            let (done_subs, total_subs) = app.subtask_counts(issue.id);
            if total_subs > 0 {
                meta.push(Span::styled(format!(" · {done_subs}/{total_subs}"), dim));
            }
            // The epic is shortened to the room left (inside the borders), or left out.
            let used: usize = meta.iter().map(|s| s.width()).sum();
            let room = (area.width as usize).saturating_sub(2 + used + 3).min(14);
            if room >= 4 {
                meta.push(Span::styled(format!(" · {}", trunc(&issue.epic, room)), dim));
            }
            lines.push(Line::from(meta));
            ListItem::new(lines)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    if items.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  (empty)", Style::default().fg(Color::DarkGray))).block(block),
            col_chunks[1],
        );
        return;
    }
    let mut state = ListState::default().with_offset(app.kanban_offsets[col_idx]);
    state.select(Some(selected_row.min(issues.len() - 1)));
    f.render_stateful_widget(List::new(items).block(block), col_chunks[1], &mut state);
    app.kanban_offsets[col_idx] = state.offset();
}
