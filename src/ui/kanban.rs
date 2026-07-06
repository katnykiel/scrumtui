use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::models::{format_duration, format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};

const STATUSES: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

/// Split `text` into lines that fit within `width` characters, breaking on word boundaries.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

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
            " [e] edit  [^H/^L] move  [Tab] parent  [</>] cycle  [u] undo  [?] help "
        } else {
            " [n] new  [e] edit  [h/l] col  [^H/^L] move  [Tab] subs  [u] undo  [?] help "
        }
    } else {
        " [n] new  [e] edit  [h/l] col  [^H/^L] move issue  [/] search  [u] undo  [?] help "
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
        .border_style(Style::default().fg(Color::Rgb(100, 100, 160)));

    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(inner);

    let board_area = v_chunks[0];
    let detail_area = v_chunks[1];

    if has_subs {
        let sub_count = app.sprint_subtasks_flat().len();
        let sub_h = ((sub_count + 2) as u16).max(4).min(board_area.height / 2);
        let parent_h = board_area.height.saturating_sub(sub_h);

        let panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(parent_h), Constraint::Length(sub_h)])
            .split(board_area);

        render_board(f, app, panels[0]);
        render_subtask_panel(f, app, panels[1]);
    } else {
        render_board(f, app, board_area);
    }

    render_detail(f, app, detail_area);
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
    let parent_name = parents.get(idx)
        .map(|p| p.title.clone())
        .unwrap_or_default();

    let border_color = if is_focused { Color::Magenta } else { Color::Rgb(90, 90, 140) };
    let title_style = if is_focused {
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let parent_label = if n > 1 {
        format!(" subtasks — {} ({}/{}) ", parent_name, idx + 1, n)
    } else {
        format!(" subtasks — {} ", parent_name)
    };
    let cycle_hint = if n > 1 { " [</>] cycle parent " } else { "" };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(Span::styled(parent_label, title_style)))
        .title_bottom(Line::from(Span::styled(
            cycle_hint,
            Style::default().fg(Color::DarkGray),
        )));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let subtasks = app.sprint_subtasks_flat();
    let sel_row = app.kanban_sub_rows[0];
    let col_w = inner.width as usize;

    let items: Vec<ListItem> = subtasks.iter().enumerate().map(|(flat_row, issue)| {
        let is_sel = is_focused && flat_row == sel_row;
        let pointer = if is_sel { "▶" } else { " " };
        let is_done = issue.status == Status::Done;
        let sc = status_color(&issue.status);
        let sym = status_symbol(&issue.status);
        let status_badge = format!("[{}]", issue.status.short().trim());

        // Subtasks: terminal default fg for active (no bold — subtler than parents), DarkGray for done.
        let title_style = if is_done {
            Style::default().fg(Color::Gray)
        } else {
            Style::default()
        };

        let due_str = issue.due_date
            .map(|d| format!(" {}", d.format("%b %d")))
            .unwrap_or_default();
        let badge_w = status_badge.len() + 1 + due_str.len();
        let title_max = col_w.saturating_sub(4 + badge_w);

        ListItem::new(Line::from(vec![
            Span::styled(format!("{pointer} "), Style::default().fg(Color::Magenta)),
            Span::styled(format!("{sym} "), Style::default().fg(sc)),
            Span::styled(
                format!("{:<width$}", trunc(&issue.title, title_max), width = title_max),
                title_style,
            ),
            Span::raw(" "),
            Span::styled(status_badge, Style::default().fg(sc)),
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
    let border_color = if is_active_col { sc } else { Color::Rgb(90, 90, 140) };

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
            // available width for title text: col_w minus 2 borders minus pointer minus space
            let title_w = col_w.saturating_sub(4).max(4);
            let is_done = issue.status == Status::Done;
            let (done_subs, total_subs) = app.subtask_counts(issue.id);
            let sub_badge = if total_subs > 0 { format!("  [{}/{}]", done_subs, total_subs) } else { String::new() };
            let due_str = issue.due_date.map(|d| format!("  {}", d.format("%b %d"))).unwrap_or_default();
            let carry_badge = if issue.carry_count > 0 {
                format!("  ↩{}", issue.carry_count)
            } else {
                String::new()
            };
            // All kanban cards are bold; done cards are faded (DarkGray) but still bold.
            let title_style = if is_done {
                Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };

            // Word-wrap the title across multiple lines.
            let title_lines = word_wrap(&issue.title, title_w);
            let mut lines: Vec<Line> = title_lines
                .into_iter()
                .enumerate()
                .map(|(i, chunk)| {
                    if i == 0 {
                        Line::from(vec![
                            Span::styled(pointer, Style::default().fg(Color::Magenta)),
                            Span::styled(format!(" {}", chunk), title_style),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::styled(chunk, title_style),
                        ])
                    }
                })
                .collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {}sp  {}{}", format_sp(issue.story_points), trunc(&issue.epic, 14), sub_badge),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(carry_badge, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(due_str, Style::default().fg(
                    issue.due_date.map(|d| due_color(&d, is_done)).unwrap_or(Color::DarkGray)
                )),
            ]));

            ListItem::new(lines)
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
        .border_style(Style::default().fg(Color::Rgb(90, 90, 140)))
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

    let carry_str = if issue.carry_count > 0 {
        format!("  ↩{} carried", issue.carry_count)
    } else {
        String::new()
    };

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
            Span::styled(carry_str, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
    ];
    if !desc.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("         ", Style::default()),
            Span::styled(desc, Style::default().fg(Color::DarkGray)),
        ]));
    }
    let time_str = if let Some(start) = issue.started_at {
        let end = issue.completed_at.unwrap_or_else(|| Local::now().naive_local());
        let label = if issue.status == Status::Done { "actual" } else { "ongoing" };
        format!("{}  {}", format_duration(start, end), label)
    } else {
        String::new()
    };

    lines.push(Line::from(vec![
        Span::styled("  due ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<12}", due_str), Style::default().fg(Color::Yellow)),
        Span::styled("  created ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", created), Style::default().fg(Color::DarkGray)),
        Span::styled("  updated ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", updated), Style::default().fg(Color::DarkGray)),
        Span::styled("  done ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", completed), Style::default().fg(Color::DarkGray)),
    ]));
    if !time_str.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  time ", Style::default().fg(Color::DarkGray)),
            Span::styled(time_str, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}
