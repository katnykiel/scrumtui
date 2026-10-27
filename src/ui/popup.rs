use chrono::{Local, NaiveDateTime};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Chart, Clear, Dataset, GraphType, List, ListItem,
        ListState, Paragraph, Wrap,
    },
    Frame,
};

use crate::app::{App, IssueForm, Popup, SprintForm, DROPDOWN_ROWS, F_DESC, F_DUE, F_EPIC, F_STATUS};
use crate::models::{format_sp, Issue, Outcome, Sprint, SprintEntry, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};
use crate::ui::hint;

/// Render whichever popup is active on top of the existing frame.
pub fn render(f: &mut Frame, popup: &Popup, app: &App) {
    match popup {
        Popup::NewIssue(form) => {
            let title = match (form.sprint_id, &app.active_sprint) {
                (Some(_), Some(s)) => format!("New issue · {}", s.name),
                _ => "New issue".to_string(),
            };
            render_issue_form(f, form, &title, app);
        }
        Popup::EditIssue(form) => {
            let title: String = match form.editing_id {
                Some(id) => format!("Edit issue #{id}"),
                None => "Edit issue".to_string(),
            };
            render_issue_form(f, form, &title, app);
        }
        Popup::SprintManager(form) => render_sprint_form(f, form, app),
        Popup::ConfirmDelete(_, title) => render_confirm(f, "Move to trash", title, None),
        Popup::ConfirmDeleteSprint(_, name) => {
            render_confirm(f, "Delete sprint", name, Some("Its issues stay; they're just unlinked."))
        }
        Popup::Trash { items, sel } => render_trash(f, items, *sel),
        // handled separately in mod.rs
        Popup::Help | Popup::GanttEpicDetail { .. } => {}
    }
}

// ── Layout helpers ─────────────────────────────────────────────────────────────

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(r.width),
        height: height.min(r.height),
    }
}

fn popup_block(title: &str, color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .border_style(Style::default().fg(color))
}

fn error_line(err: &str) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        format!("  ! {err}"),
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )))
}

// ── Issue form ─────────────────────────────────────────────────────────────────

const FIELD_LABELS: [&str; 6] = [
    "Title",
    "Epic",
    "Story points",
    "Status",
    "Due date (YYYY-MM-DD)",
    "Description",
];

fn render_issue_form(f: &mut Frame, form: &IssueForm, title: &str, app: &App) {
    let visible_subtasks = form.subtasks.iter().filter(|s| !s.deleted).count();
    // header + each subtask row, minimum 3 rows so the section is always visible
    let subtask_section_height: u16 = (2 + visible_subtasks).max(3) as u16;
    // Description field gets more rows when focused so text wraps visibly
    let desc_extra: u16 = if form.focused_field == F_DESC && !form.in_subtask_list { 2 } else { 0 };
    // Six fields of `field_h` rows, 2 for the error and 2 for the border.
    let fields_height = |field_h: u16| 6 * field_h + 4;
    let max_height = f.area().height.saturating_sub(2);
    // A field is a label, its value and an underline. When the screen is too short
    // for that plus 3 subtask rows, drop the underlines rather than let the layout
    // squeeze out values. The subtask list takes what is left and scrolls.
    let compact = fields_height(3) + 3 > max_height;
    let field_h: u16 = if compact { 2 } else { 3 };
    let desc_height = field_h + desc_extra;
    let total_height = (fields_height(field_h) + desc_extra + subtask_section_height).min(max_height);
    let area = centered_rect(76, total_height, f.area());
    f.render_widget(Clear, area);

    // At most 72 characters, or the bottom border of the 76-wide popup cuts them.
    let bottom_hint = if form.in_subtask_list && form.subtask_editing {
        "[Enter] done  [Tab] next subtask  [Esc] done"
    } else if form.in_subtask_list {
        "[e] rename  [h/l] status  [n/x] add/remove  [Enter] save  [Esc] back"
    } else if form.epic_dropdown_open || form.due_date_dropdown_open {
        "[↑/↓] choose  [Enter/Tab] pick  [Esc] close list"
    } else if form.focused_field == F_STATUS {
        "[h/l] status  [Tab] next  [^N] subtask  [Enter] save  [Esc] cancel"
    } else {
        "[Tab/↓] next  [S-Tab/↑] prev  [^N] subtask  [Enter] save  [Esc] cancel"
    };

    let block = popup_block(title, Color::Cyan)
        .title_bottom(hint(bottom_hint))
        .border_style(Style::default().fg(Color::Rgb(100, 100, 180)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let field_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(field_h), // title
            Constraint::Length(field_h), // epic
            Constraint::Length(field_h), // story points
            Constraint::Length(field_h), // status
            Constraint::Length(field_h), // due
            Constraint::Length(desc_height), // desc (taller when focused)
            Constraint::Length(2), // error
            Constraint::Min(0), // subtasks (the rest)
        ])
        .split(inner);

    let values: [String; 6] = [
        form.title.clone(),
        form.epic.clone(),
        form.story_points.clone(),
        Status::from_index(form.status_idx).label().to_string(),
        form.due_date.clone(),
        form.description.clone(),
    ];

    for (i, label) in FIELD_LABELS.iter().enumerate() {
        // Don't show any field as focused when the subtask section has focus
        let is_focused = form.focused_field == i && !form.in_subtask_list;
        let label_style = if is_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if compact {
            // No underlines between the fields, so set the labels apart.
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let underline = Block::default()
            .borders(if compact { Borders::NONE } else { Borders::BOTTOM })
            .border_style(Style::default().fg(if is_focused { Color::Cyan } else { Color::DarkGray }));

        if i == F_DESC {
            // Description: multi-line with word wrap. When focused, use the terminal's
            // real cursor instead of a REVERSED highlight (which looks broken on a
            // multi-line block in dark terminals).
            let mut desc_lines: Vec<Line> = vec![Line::from(Span::styled(format!(" {label}"), label_style))];
            let desc_src = if is_focused {
                form.description.clone()
            } else {
                form.description.replace('\n', "  ·  ")
            };
            for text_line in desc_src.split('\n') {
                desc_lines.push(Line::from(Span::styled(format!(" {text_line}"), Style::default())));
            }
            f.render_widget(
                Paragraph::new(desc_lines).wrap(Wrap { trim: false }).block(underline),
                field_areas[i],
            );
            // Place the real terminal cursor at the end of the last content line.
            if is_focused {
                let raw = &form.description;
                let last_line_raw = raw.split('\n').last().unwrap_or("");
                let last_idx = raw.split('\n').count().saturating_sub(1);
                // +1 for the single space prefix added in format!(" {}", text_line)
                let cx = (field_areas[i].x + 1 + last_line_raw.chars().count() as u16)
                    .min(field_areas[i].x + field_areas[i].width.saturating_sub(2));
                // +1 for the label row at the top of the field
                let cy = (field_areas[i].y + 1 + last_idx as u16)
                    .min(field_areas[i].y + field_areas[i].height.saturating_sub(1));
                f.set_cursor_position((cx, cy));
            }
            continue;
        }

        let value_line = if i == F_STATUS {
            let s = Status::from_index(form.status_idx);
            let style = Style::default().fg(status_color(&s)).add_modifier(Modifier::BOLD);
            let mut spans = vec![Span::styled(format!(" {} {}", status_symbol(&s), values[i]), style)];
            if is_focused {
                spans.push(Span::styled("   ← h  l →", Style::default().fg(Color::DarkGray)));
            }
            Line::from(spans)
        } else {
            let cursor = if is_focused { "▌" } else { "" };
            Line::from(vec![
                Span::raw(format!(" {}", values[i])),
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
            ])
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(format!(" {label}"), label_style)),
                value_line,
            ])
            .block(underline),
            field_areas[i],
        );
    }

    if let Some(err) = &form.error {
        f.render_widget(error_line(err), field_areas[6]);
    }

    render_subtask_list(f, form, field_areas[7]);

    if form.epic_dropdown_open {
        render_epic_dropdown(f, form, app, field_areas[F_EPIC], inner);
    }
    if form.due_date_dropdown_open {
        render_due_date_dropdown(f, form, app, field_areas[F_DUE], inner);
    }
}

fn render_subtask_list(f: &mut Frame, form: &IssueForm, area: Rect) {
    let is_focused = form.in_subtask_list;
    let border_color = if is_focused { Color::Cyan } else { Color::Rgb(60, 60, 90) };
    // Reset, not unset: a title span without a color takes the border's.
    let label_style = if is_focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    let visible: Vec<_> = form.subtasks.iter().filter(|s| !s.deleted).collect();
    let done = visible.iter().filter(|s| s.status_idx == 2).count();
    let count = if visible.is_empty() { String::new() } else { format!("{done}/{} ", visible.len()) };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(vec![
            Span::styled(" Subtasks ", label_style),
            Span::styled(count, Style::default().fg(Color::DarkGray)),
        ]));

    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    if visible.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  (none)  Ctrl-N adds one",
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(vis_idx, st)| {
            let is_sel = is_focused && vis_idx == form.subtask_sel;
            let status = Status::from_index(st.status_idx);
            let sc = status_color(&status);
            // Show cursor only when actively editing the title
            let cursor = if is_sel && form.subtask_editing { "▌" } else { "" };
            let row_style = if is_sel && !form.subtask_editing {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else if is_sel {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { " ▶ " } else { "   " }, Style::default().fg(Color::Magenta)),
                Span::styled(format!("{} ", status_symbol(&status)), Style::default().fg(sc)),
                Span::styled(format!("{}{}", st.title, cursor), row_style),
                Span::styled(format!("  {}", status.short().trim()), Style::default().fg(sc)),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    if is_focused {
        state.select(Some(form.subtask_sel));
    }
    f.render_stateful_widget(List::new(items), inner, &mut state);
}

/// A dropdown list under a form field (inside the form's `bounds`): (label, hint)
/// rows, `sel` highlighted.
fn render_dropdown(
    f: &mut Frame,
    field_area: Rect,
    bounds: Rect,
    title: &str,
    rows: &[(String, String)],
    sel: usize,
) {
    let rows = &rows[..rows.len().min(DROPDOWN_ROWS)];
    if rows.is_empty() {
        return;
    }
    let width = rows
        .iter()
        .map(|(v, h)| v.chars().count() + if h.is_empty() { 0 } else { h.len() + 3 })
        .max()
        .unwrap_or(10)
        .saturating_add(4)
        .max(22)
        .min(field_area.width.saturating_sub(2) as usize) as u16;
    let height = rows.len() as u16 + 2;
    // Below the field, or above it when only that fits inside the form; never off
    // screen (drawing outside the buffer panics).
    let below = field_area.y + field_area.height;
    let y = if below + height > bounds.bottom() && field_area.y >= bounds.y + height {
        field_area.y - height
    } else {
        below
    };
    let drop_area = Rect { x: field_area.x + 2, y, width, height }.intersection(f.area());
    f.render_widget(Clear, drop_area);

    let sel = sel.min(rows.len() - 1);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (value, hint))| {
            let style = if i == sel {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(format!(" {value}"), style)];
            if !hint.is_empty() {
                spans.push(Span::styled(format!("  {hint}"), Style::default().fg(Color::DarkGray)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(format!(" {title} "), Style::default().fg(Color::DarkGray))),
    );
    f.render_widget(list, drop_area);
}

fn render_epic_dropdown(f: &mut Frame, form: &IssueForm, app: &App, field_area: Rect, bounds: Rect) {
    let rows: Vec<(String, String)> = app
        .epic_matches(&form.epic)
        .into_iter()
        .map(|e| (e, String::new()))
        .collect();
    render_dropdown(f, field_area, bounds, "epics", &rows, form.epic_dropdown_sel);
}

fn render_due_date_dropdown(f: &mut Frame, form: &IssueForm, app: &App, field_area: Rect, bounds: Rect) {
    let rows = app.due_matches(&form.due_date);
    render_dropdown(f, field_area, bounds, "due date", &rows, form.due_date_dropdown_sel);
}

// ── Trash popup ──────────────────────────────────────────────────────────────────

fn render_trash(f: &mut Frame, items: &[Issue], sel: usize) {
    // max then min, not clamp: clamp panics when the screen is shorter than 12 rows.
    let height = (items.len() as u16 + 4).max(8).min(f.area().height.saturating_sub(4));
    let area = centered_rect(70, height, f.area());
    f.render_widget(Clear, area);

    let block = popup_block("Trash", Color::Red)
        .title_bottom(hint("[r] restore  [D] delete forever  [Esc] close"));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled("  Trash is empty.", Style::default().fg(Color::DarkGray)))),
            inner,
        );
        return;
    }

    let title_w = (inner.width as usize).saturating_sub(22);
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, issue)| {
            let is_sel = i == sel;
            let style = if is_sel {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { " ▶ " } else { "   " }, Style::default().fg(Color::Red)),
                Span::styled(format!("{:<title_w$}", trunc(&issue.title, title_w)), style),
                Span::styled(format!("  {:<14}", trunc(&issue.epic, 14)), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(sel));
    f.render_stateful_widget(List::new(list_items), inner, &mut state);
}

// ── Sprint form ────────────────────────────────────────────────────────────────

fn render_sprint_form(f: &mut Frame, form: &SprintForm, app: &App) {
    let area = centered_rect(96, 24, f.area());
    f.render_widget(Clear, area);

    let is_new = form.editing_id.is_none();
    let toggle_hint = match (is_new, &app.active_sprint) {
        (true, Some(_)) => "[Ctrl-N] edit current sprint  ",
        (false, _) => "[Ctrl-N] new sprint  ",
        (true, None) => "",
    };
    let block = popup_block(if is_new { "New sprint" } else { "Edit sprint" }, Color::Magenta)
        .title_bottom(hint(&format!("[Tab] next  [Enter] save  {toggle_hint}[Esc] cancel")));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Left: form fields; right: burnup chart (editing) or what saving will do (new)
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(10)])
        .split(inner);

    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(h_chunks[0]);

    let fields: [(&str, String); 4] = [
        ("Name", form.name.clone()),
        ("Start (YYYY-MM-DD)", form.start_date.clone()),
        ("End (YYYY-MM-DD)", form.end_date.clone()),
        ("Active", String::new()),
    ];
    for (i, (label, value)) in fields.iter().enumerate() {
        let is_focused = form.focused_field == i;
        let label_style = if is_focused {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let value_line = if i == 3 {
            let mut spans = vec![if form.is_active {
                Span::styled(" ✓ yes", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" ✗ no", Style::default().fg(Color::DarkGray))
            }];
            if is_focused {
                spans.push(Span::styled("   space toggles", Style::default().fg(Color::DarkGray)));
            }
            Line::from(spans)
        } else {
            Line::from(vec![
                Span::raw(format!(" {value}")),
                Span::styled(if is_focused { "▌" } else { "" }, Style::default().fg(Color::Magenta)),
            ])
        };
        f.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(format!(" {label}"), label_style)), value_line])
                .block(
                    Block::default()
                        .borders(Borders::BOTTOM)
                        .border_style(Style::default().fg(if is_focused { Color::Magenta } else { Color::DarkGray })),
                ),
            field_chunks[i],
        );
    }

    if let Some(err) = &form.error {
        f.render_widget(error_line(err), field_chunks[4]);
    }

    let side = h_chunks[1];
    if let Some(sprint) = &form.sprint {
        render_burnup_chart(f, sprint, &form.entries, side);
        return;
    }

    // New sprint: say what saving will do.
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![Line::from("")];
    let carried: Vec<&Issue> = match (&app.active_sprint, form.is_active) {
        (Some(old), true) => app
            .issues
            .iter()
            .filter(|i| i.sprint_id == Some(old.id) && i.parent_id.is_none() && i.status != Status::Done)
            .collect(),
        _ => Vec::new(),
    };
    match (&app.active_sprint, form.is_active) {
        (Some(old), true) => {
            lines.push(Line::from(format!("  Saving ends {} and starts this sprint.", old.name)));
            if carried.is_empty() {
                lines.push(Line::from(Span::styled("  Nothing unfinished to carry over.", dim)));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  Carried over ({}), each marked ↩:", carried.len()),
                    dim,
                )));
                let w = (side.width as usize).saturating_sub(12);
                let room = (side.height as usize).saturating_sub(5);
                for issue in carried.iter().take(room) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("   {} ", status_symbol(&issue.status)), Style::default().fg(status_color(&issue.status))),
                        Span::styled(trunc(&issue.title, w), Style::default()),
                        Span::styled(format!(" {}sp", format_sp(issue.story_points)), Style::default().fg(Color::Magenta)),
                    ]));
                }
                if carried.len() > room {
                    lines.push(Line::from(Span::styled(format!("   … and {} more", carried.len() - room), dim)));
                }
            }
        }
        (None, true) => lines.push(Line::from(
            "  Saving starts this sprint. Then press s on backlog issues to pull them in.",
        )),
        (_, false) => lines.push(Line::from(Span::styled(
            "  Saved as an inactive sprint (for planning or record keeping).",
            dim,
        ))),
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(80, 60, 100))),
        ),
        side,
    );
}

/// Burnup series for one sprint, x in days since the start of its first day.
struct Burnup {
    scope: Vec<(f64, f64)>,
    ideal: Vec<(f64, f64)>,
    actual: Vec<(f64, f64)>,
    max_scope: f64,
    days: f64,
}

/// Compute the burnup lines of a sprint from its membership log:
/// - scope: steps up when an issue joins after planning, down when one is removed
/// - ideal: straight line from 0 to the current scope at the end of the sprint
/// - actual: done story points, stepping up at each completion
fn compute_burnup(sprint: &Sprint, entries: &[SprintEntry]) -> Option<Burnup> {
    let days = sprint.duration_days() as f64;
    let start: NaiveDateTime = sprint.start_date.and_hms_opt(0, 0, 0)?;
    let x_of = |t: NaiveDateTime| ((t - start).num_seconds() as f64 / 86400.0).clamp(0.0, days);
    let now_x = x_of(Local::now().naive_local());

    // Scope events: (x, delta)
    let mut initial = 0.0;
    let mut changes: Vec<(f64, f64)> = Vec::new();
    for e in entries {
        let sp = e.issue.story_points;
        if e.added_mid_sprint(sprint) {
            changes.push((x_of(e.joined_at(sprint)), sp));
        } else {
            initial += sp;
        }
        if let (Outcome::Removed, Some(removed)) = (e.outcome, e.removed_at) {
            changes.push((x_of(removed), -sp));
        }
    }
    changes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut scope = vec![(0.0, initial)];
    let mut running = initial;
    let mut max_scope = initial;
    for (x, delta) in changes {
        scope.push((x, running));
        running += delta;
        scope.push((x, running));
        max_scope = max_scope.max(running);
    }
    if scope.last().map_or(0.0, |p| p.0) < now_x {
        scope.push((now_x, running));
    }
    if max_scope <= 0.0 {
        return None;
    }

    let mut done_events: Vec<(f64, f64)> = entries
        .iter()
        .filter(|e| e.outcome == Outcome::Done)
        .filter_map(|e| e.issue.completed_at.map(|c| (x_of(c), e.issue.story_points)))
        .collect();
    done_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut actual = vec![(0.0, 0.0)];
    let mut done = 0.0;
    for (x, sp) in done_events {
        actual.push((x, done));
        done += sp;
        actual.push((x, done));
    }
    if actual.last().map_or(0.0, |p| p.0) < now_x {
        actual.push((now_x, done));
    }

    Some(Burnup {
        scope,
        ideal: vec![(0.0, 0.0), (days, running.max(0.0))],
        actual,
        max_scope,
        days,
    })
}

/// Render the burnup chart of a sprint (used by the sprint manager and the history view).
pub fn render_burnup_chart(f: &mut Frame, sprint: &Sprint, entries: &[SprintEntry], area: Rect) {
    let chart_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(" burnup ", Style::default().fg(Color::DarkGray)))
        .border_style(Style::default().fg(Color::Rgb(80, 60, 100)));

    let Some(b) = compute_burnup(sprint, entries) else {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  No story points in this sprint.", Style::default().fg(Color::DarkGray))),
            ])
            .block(chart_block),
            area,
        );
        return;
    };

    let dim = Style::default().fg(Color::DarkGray);
    let x_labels = vec![
        Span::styled(sprint.start_date.format("%a %d").to_string(), dim),
        Span::styled(sprint.end_date.format("%a %d").to_string(), dim),
    ];
    let y_labels = vec![
        Span::styled("0", dim),
        Span::styled(format_sp(b.max_scope / 2.0), dim),
        Span::styled(format_sp(b.max_scope), Style::default().fg(Color::Yellow)),
    ];
    let datasets = vec![
        Dataset::default()
            .name("scope")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow))
            .data(&b.scope),
        Dataset::default()
            .name("ideal")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Rgb(120, 120, 120)))
            .data(&b.ideal),
        Dataset::default()
            .name("done")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&b.actual),
    ];

    // Burnup lines rise toward the top-right, so keep the legend out of their way.
    let chart = Chart::new(datasets)
        .block(chart_block)
        .legend_position(Some(ratatui::widgets::LegendPosition::BottomRight))
        // Keep the legend in smaller charts too (the default hides it below ~40 rows).
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)))
        .x_axis(Axis::default().bounds([0.0, b.days]).labels(x_labels).style(dim))
        .y_axis(Axis::default().bounds([0.0, b.max_scope * 1.1]).labels(y_labels).style(dim));
    f.render_widget(chart, area);
}

// ── Confirm dialogs ────────────────────────────────────────────────────────────

fn render_confirm(f: &mut Frame, title: &str, subject: &str, note: Option<&str>) {
    let area = centered_rect(60, if note.is_some() { 8 } else { 7 }, f.area());
    f.render_widget(Clear, area);
    let block = popup_block(title, Color::Red);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let width = (inner.width as usize).saturating_sub(4);
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(format!("  {}", trunc(subject, width)), Style::default().add_modifier(Modifier::BOLD))),
    ];
    if let Some(note) = note {
        lines.push(Line::from(Span::styled(format!("  {note}"), Style::default().fg(Color::DarkGray))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [d]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(" confirm    ", Style::default().fg(Color::DarkGray)),
        Span::styled("[n / Esc]", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

// ── Help ───────────────────────────────────────────────────────────────────────

/// (keys, description) rows; a row with empty keys starts a section.
/// Keys mean the same thing in every view, so each is listed once.
const HELP_LEFT: &[(&str, &str)] = &[
    ("", "EVERYWHERE"),
    ("1 2 3 4", "switch view"),
    ("j k   ↓ ↑", "move"),
    ("g G   Home End", "top / bottom"),
    ("PgDn PgUp", "page"),
    ("h l   ← →", "status (kanban: column)"),
    ("Tab", "next panel / field"),
    ("n", "new"),
    ("e   Enter", "edit / open"),
    ("d", "delete"),
    ("/   Esc", "search · clear / back"),
    ("f", "pin / unpin the frog"),
    ("y", "copy issue to clipboard"),
    ("u", "undo"),
    ("S   T", "sprint manager · trash"),
    ("?   q", "help · quit"),
];

const HELP_RIGHT: &[(&str, &str)] = &[
    ("", "BACKLOG"),
    ("s", "into / out of the sprint"),
    ("Ctrl-J/K", "reorder"),
    ("c", "show / hide done"),
    ("", "KANBAN"),
    ("Ctrl-H/L", "move card left / right"),
    ("", "FORMS"),
    ("Enter   Ctrl-S", "save"),
    ("Esc", "cancel (twice if changed)"),
    ("Ctrl-N   x", "add · remove subtask"),
    ("Del", "clear due date"),
    ("", "SPRINT MANAGER"),
    ("Ctrl-N", "new sprint ↔ edit current"),
    ("", "TRASH"),
    ("r   D", "restore · delete forever"),
];

fn help_lines(rows: &[(&str, &str)]) -> Vec<Line<'static>> {
    rows.iter()
        .map(|(keys, desc)| {
            if keys.is_empty() {
                Line::from(Span::styled(
                    format!("  {desc}"),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(vec![
                    Span::styled(format!("  {keys:<17}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(desc.to_string()),
                ])
            }
        })
        .collect()
}

/// The key reference. On a screen too short for it, it scrolls by `scroll` rows
/// (capped here at what the screen needs).
pub fn render_help(f: &mut Frame, scroll: &mut u16) {
    let two_columns = f.area().width >= 96;
    let rows = HELP_LEFT.len().max(HELP_RIGHT.len());
    let (width, height) = if two_columns {
        (94, rows as u16 + 3)
    } else {
        (56, (HELP_LEFT.len() + HELP_RIGHT.len()) as u16 + 3)
    };
    let area = centered_rect(width, height, f.area());
    f.render_widget(Clear, area);
    *scroll = (*scroll).min(height.saturating_sub(area.height));

    let block = popup_block("Keys", Color::Cyan)
        .title_bottom(hint(if height > area.height {
            "[j/k] scroll  [Esc / q / ?] close"
        } else {
            "[Esc / q / ?] close"
        }))
        .border_style(Style::default().fg(Color::Rgb(80, 80, 120)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut left = vec![Line::from("")];
    left.extend(help_lines(HELP_LEFT));
    let mut right = vec![Line::from("")];
    right.extend(help_lines(HELP_RIGHT));
    if two_columns {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(inner);
        f.render_widget(Paragraph::new(left).scroll((*scroll, 0)), cols[0]);
        f.render_widget(Paragraph::new(right).scroll((*scroll, 0)), cols[1]);
    } else {
        left.extend(right.into_iter().skip(1));
        f.render_widget(Paragraph::new(left).scroll((*scroll, 0)), inner);
    }
}
