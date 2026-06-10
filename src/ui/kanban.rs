use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};

const STATUSES: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

fn due_color(due: &chrono::NaiveDate, done: bool) -> Color {
    if done { return Color::DarkGray; }
    let today = Local::now().date_naive();
    let days = (*due - today).num_days();
    if days < 0 { Color::Red } else if days < 7 { Color::Yellow } else { Color::DarkGray }
}

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let has_subs = app.sprint_has_any_subtasks();

    let hint = if has_subs {
        if app.kanban_panel == 1 {
            " [h/l] col  [j/k] nav  [Tab] panel  [</>] parent  []/[] status  [e] edit  [?]help "
        } else {
            " [h/l] col  [j/k] nav  [Tab] sub-panel  []/[] status  [e] edit  [?]help "
        }
    } else {
        " [h/l] col  [j/k] nav  []/[] status  [e] edit  [?]help "
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("kanban", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Rgb(80, 80, 120)));

    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Vertical split: kanban board on top, detail pane on bottom (4 rows)
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(inner);

    let board_area = v_chunks[0];
    let detail_area = v_chunks[1];

    if has_subs {
        // Subtask bar height: enough for all subtasks of focused parent + 2 for header border
        let sub_count = app.sprint_subtasks_flat().len();
        let sub_h = ((sub_count + 2) as u16).max(3).min(board_area.height / 2);
        let parent_h = board_area.height.saturating_sub(sub_h);

        let panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(parent_h), Constraint::Length(sub_h)])
            .split(board_area);

        render_board(f, app, panels[0], false);
        render_subtask_bar(f, app, panels[1]);
    } else {
        render_board(f, app, board_area, false);
    }

    render_detail(f, app, detail_area);
}

fn render_board(f: &mut Frame, app: &mut App, area: Rect, is_sub_panel: bool) {
    let panel_focused = if is_sub_panel { app.kanban_panel == 1 } else { app.kanban_panel == 0 };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)])
        .split(area);

    for (col_idx, status) in STATUSES.iter().enumerate() {
        render_column(f, app, cols[col_idx], col_idx, status, panel_focused);
    }
}

fn render_subtask_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.kanban_panel == 1;
    let parents = app.sprint_parents_with_subtasks();
    let n = parents.len();
    let idx = app.kanban_sub_parent_idx.min(n.saturating_sub(1));
    let parent_name = parents.get(idx)
        .map(|p| p.title.as_str())
        .unwrap_or("");

    let border_color = if is_focused { Color::Magenta } else { Color::Rgb(60, 60, 90) };
    let title_style = if is_focused {
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let cycle_hint = if n > 1 {
        format!(" [</>]  {} / {}  {} ", idx + 1, n, parent_name)
    } else {
        format!("  {} ", parent_name)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("subtasks", title_style),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            cycle_hint,
            Style::default().fg(Color::DarkGray),
        )));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let subtasks = app.sprint_subtasks_flat();
    let sel_row = app.kanban_sub_rows[0];
    let w = inner.width as usize;

    let items: Vec<ListItem> = subtasks.iter().enumerate().map(|(row, issue)| {
        let is_sel = is_focused && row == sel_row;
        let pointer = if is_sel { "▶" } else { " " };
        let is_done = issue.status == Status::Done;
        let title_style = if is_done {
            Style::default().fg(Color::DarkGray)
        } else if is_sel {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        let sym = status_symbol(&issue.status);
        let sc = status_color(&issue.status);
        let due_str = issue.due_date
            .map(|d| format!("  {}", d.format("%b %d")))
            .unwrap_or_default();
        let title_max = w.saturating_sub(6 + due_str.len());
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {pointer} "), Style::default().fg(Color::Magenta)),
            Span::styled(format!("{sym} "), Style::default().fg(sc)),
            Span::styled(trunc(&issue.title, title_max), title_style),
            Span::styled(due_str, Style::default().fg(
                issue.due_date.map(|d| due_color(&d, is_done)).unwrap_or(Color::DarkGray)
            )),
        ]))
    }).collect();

    let mut list_state = ratatui::widgets::ListState::default();
    if is_focused && !subtasks.is_empty() {
        list_state.select(Some(sel_row.min(subtasks.len().saturating_sub(1))));
    }

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));
    f.render_stateful_widget(list, inner, &mut list_state);
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

    // Active col in focused panel uses the status color; everything else is dim purple
    let border_color = if is_active_col { sc } else { Color::Rgb(60, 60, 90) };

    let col_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    // ── Header ──────────────────────────────────────────────────────────────
    let sym = status_symbol(status);
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let sp_str = if total_sp > 0.0 { format!("  {}sp", format_sp(total_sp)) } else { String::new() };
    let header_text = format!("  {sym} {}  {}  {}{}", status.label(), "·", issues.len(), sp_str);
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

    // ── Issue list ───────────────────────────────────────────────────────────
    let col_w = area.width as usize;
    let items: Vec<ListItem> = issues
        .iter()
        .enumerate()
        .map(|(row, issue)| {
            let is_sel = is_active_col && row == selected_row;
            let pointer = if is_sel { "▶" } else { " " };
            let title_w = col_w.saturating_sub(4);
            let is_done = issue.status == Status::Done;
            let (done_subs, total_subs) = app.subtask_counts(issue.id);
            let sub_badge = if total_subs > 0 { format!("  [{}/{}]", done_subs, total_subs) } else { String::new() };
            let due_str = issue.due_date.map(|d| format!("  {}", d.format("%b %d"))).unwrap_or_default();
            let title_style = if is_done {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::Magenta)),
                    Span::styled(format!(" {}", trunc(&issue.title, title_w)), title_style),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("   #{} · {}sp  {}{}", issue.id, format_sp(issue.story_points), trunc(&issue.epic, 14), sub_badge),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(due_str, Style::default().fg(
                        issue.due_date.map(|d| due_color(&d, is_done)).unwrap_or(Color::DarkGray)
                    )),
                ]),
            ])
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    if is_active_col && !issues.is_empty() {
        list_state.select(Some(selected_row.min(issues.len().saturating_sub(1))));
    }

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    f.render_stateful_widget(list, col_chunks[1], &mut list_state);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 90)))
        .title(Span::styled(" detail ", Style::default().fg(Color::DarkGray)));

    let Some(issue) = app.kanban_selected_issue() else {
        f.render_widget(
            Paragraph::new("  No issue selected.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    };

    let sc = status_color(&issue.status);
    let sym = status_symbol(&issue.status);
    let desc = issue.description.as_deref().unwrap_or("");
    let due_str = issue.due_date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".into());
    let created = issue.created_at.format("%Y-%m-%d %H:%M").to_string();
    let updated = issue.updated_at.format("%Y-%m-%d %H:%M").to_string();
    let completed = issue.completed_at
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".into());

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("  {sym} "), Style::default().fg(sc)),
            Span::styled(
                issue.title.clone(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   #{} · {}sp · {} · {}", issue.id, format_sp(issue.story_points), issue.status.label(), issue.epic),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];
    if !desc.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("         ", Style::default()),
            Span::styled(desc, Style::default().fg(Color::Gray)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  due ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<12}", due_str), Style::default().fg(Color::Yellow)),
        Span::styled("  created ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", created), Style::default().fg(Color::Gray)),
        Span::styled("  updated ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", updated), Style::default().fg(Color::Gray)),
        Span::styled("  done ", Style::default().fg(Color::DarkGray)),
        Span::styled(completed, Style::default().fg(Color::Gray)),
    ]));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}
