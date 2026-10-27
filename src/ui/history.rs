use std::collections::{HashMap, HashSet};

use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Outcome, Sprint, SprintEntry, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};
use crate::ui::popup::render_burnup_chart;
use crate::ui::{hint, progress_bar, BORDER, BORDER_DIM};

/// Story points of one sprint, split by what happened to them.
#[derive(Default)]
struct SprintStats {
    /// Joined by the planning day (see `Sprint::planning_day`).
    planned: f64,
    /// Joined later (scope creep).
    added: f64,
    removed: f64,
    done: f64,
    carried: f64,
    open: f64,
    /// Planned points that got done.
    planned_done: f64,
    issues: usize,
    issues_done: usize,
}

impl SprintStats {
    fn of(sprint: &Sprint, entries: &[SprintEntry]) -> Self {
        let mut s = SprintStats::default();
        for e in entries {
            let sp = e.issue.story_points;
            let mid = e.added_mid_sprint(sprint);
            if mid { s.added += sp } else { s.planned += sp }
            match e.outcome {
                Outcome::Done => {
                    s.done += sp;
                    s.issues_done += 1;
                    if !mid { s.planned_done += sp }
                }
                Outcome::Open => s.open += sp,
                Outcome::Carried => s.carried += sp,
                Outcome::Removed => s.removed += sp,
            }
            if e.outcome != Outcome::Removed {
                s.issues += 1;
            }
        }
        s
    }

    /// What the sprint ended up containing.
    fn scope(&self) -> f64 {
        self.planned + self.added - self.removed
    }

    fn completion(&self) -> f64 {
        if self.scope() > 0.0 { self.done / self.scope() } else { 0.0 }
    }
}

fn entries_of<'a>(app: &'a App, sprint: &Sprint) -> &'a [SprintEntry] {
    app.history_entries.get(&sprint.id).map(Vec::as_slice).unwrap_or(&[])
}

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    // Wide terminals get a third column with the burnup chart and the analysis;
    // narrower ones keep the analysis under the issue list and skip the chart.
    let wide = area.width >= 110;
    let right_w = if wide { (area.width.saturating_sub(30) * 2 / 5).clamp(30, 52) } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(30), Constraint::Length(right_w)])
        .split(area);

    render_sprint_list(f, app, chunks[0]);

    let Some(sprint) = app.history_sprints.get(app.history_sel).cloned() else {
        for chunk in &chunks[1..] {
            f.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_DIM)),
                *chunk,
            );
        }
        return;
    };
    let stats = SprintStats::of(&sprint, entries_of(app, &sprint));

    if !wide {
        let analysis_h = 12u16.min(chunks[1].height / 2);
        let middle = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(3), Constraint::Length(analysis_h)])
            .split(chunks[1]);
        render_header(f, &sprint, &stats, middle[0]);
        render_issue_list(f, app, &sprint, middle[1]);
        render_analysis(f, app, &sprint, &stats, middle[2]);
        return;
    }

    let middle = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(3)])
        .split(chunks[1]);
    render_header(f, &sprint, &stats, middle[0]);
    render_issue_list(f, app, &sprint, middle[1]);

    let analysis_h = 16u16.min(chunks[2].height.saturating_sub(8));
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(analysis_h)])
        .split(chunks[2]);
    render_burnup_chart(f, &sprint, entries_of(app, &sprint), right[0]);
    render_analysis(f, app, &sprint, &stats, right[1]);
}

fn render_sprint_list(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.history_focus == 0;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(Span::styled(
            " sprints ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )))
        .title_bottom(hint("[n]ew [e]dit [d]elete"))
        .border_style(Style::default().fg(if focused { BORDER } else { BORDER_DIM }));

    if app.history_sprints.is_empty() {
        f.render_widget(
            Paragraph::new("  No sprints yet. n to start one.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let name_w = (area.width as usize).saturating_sub(7);
    let items: Vec<ListItem> = app
        .history_sprints
        .iter()
        .enumerate()
        .map(|(i, sprint)| {
            let is_sel = i == app.history_sel;
            let stats = SprintStats::of(sprint, entries_of(app, sprint));
            let name_style = match (is_sel, sprint.is_active) {
                (true, _) => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED),
                (false, true) => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                (false, false) => Style::default(),
            };
            let pct = stats.completion();
            let pct_color = if pct >= 0.8 { Color::Green } else if pct >= 0.5 { Color::Yellow } else { Color::Red };
            let mut second = vec![Span::styled(
                format!("  {} → {} ", sprint.start_date.format("%b %d"), sprint.end_date.format("%b %d")),
                Style::default().fg(Color::DarkGray),
            )];
            second.extend(progress_bar(pct, 5, pct_color));
            second.push(Span::styled(format!(" {:.0}%", pct * 100.0), Style::default().fg(Color::DarkGray)));
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(if is_sel { "▶" } else { " " }, Style::default().fg(Color::Magenta)),
                    Span::styled(if sprint.is_active { " ● " } else { "   " }, Style::default().fg(Color::Green)),
                    Span::styled(trunc(&sprint.name, name_w), name_style),
                ]),
                Line::from(second),
            ])
        })
        .collect();

    let mut state = ListState::default()
        .with_offset(app.history_offset)
        .with_selected(Some(app.history_sel));
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
    app.history_offset = state.offset();
}

fn render_header(f: &mut Frame, sprint: &Sprint, stats: &SprintStats, area: Rect) {
    let today = Local::now().date_naive();
    let dim = Style::default().fg(Color::DarkGray);
    let days = sprint.duration_days();

    let state = if sprint.is_active && today > sprint.end_date {
        Span::styled("  ENDED, STILL ACTIVE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else if sprint.is_active {
        Span::styled("  ACTIVE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else if today < sprint.start_date {
        Span::styled("  UPCOMING", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  COMPLETED", dim.add_modifier(Modifier::BOLD))
    };
    let mut dates = format!(
        "  {} → {} · {days} days",
        sprint.start_date.format("%a %b %d"),
        sprint.end_date.format("%a %b %d, %Y"),
    );
    if sprint.is_active && today >= sprint.start_date && today <= sprint.end_date {
        dates.push_str(&format!(" · day {}/{days}", (today - sprint.start_date).num_days() + 1));
    }

    let mut scope = vec![
        Span::styled("  planned ", dim),
        Span::styled(format!("{}sp", format_sp(stats.planned)), Style::default().fg(Color::Gray)),
    ];
    if stats.added > 0.0 {
        scope.push(Span::styled(format!("  +{}sp added", format_sp(stats.added)), Style::default().fg(Color::Yellow)));
    }
    if stats.removed > 0.0 {
        scope.push(Span::styled(format!("  −{}sp removed", format_sp(stats.removed)), dim));
    }
    if stats.carried > 0.0 {
        scope.push(Span::styled(format!("  ↪ {}sp carried over", format_sp(stats.carried)), Style::default().fg(Color::Yellow)));
    }

    let lines = vec![
        Line::from(vec![
            Span::styled(format!("  {}", sprint.name), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            state,
        ]),
        Line::from(Span::styled(dates, dim)),
        Line::from(vec![
            Span::styled("  done ", dim),
            Span::styled(
                format!("{}/{}sp", format_sp(stats.done), format_sp(stats.scope())),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" ({:.0}%)", stats.completion() * 100.0), dim),
            Span::styled(format!(" · {}/{} issues", stats.issues_done, stats.issues), dim),
        ]),
        Line::from(scope),
    ];
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_DIM)),
        ),
        area,
    );
}

fn render_issue_list(f: &mut Frame, app: &mut App, sprint: &Sprint, area: Rect) {
    let focused = app.history_focus == 1;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(Span::styled(" issues ", Style::default().fg(Color::DarkGray))))
        .title_bottom(hint(if focused {
            "[j/k] move  [e] edit  [Tab] sprints"
        } else {
            "[Tab] browse issues   + = added mid-sprint"
        }))
        .border_style(Style::default().fg(if focused { BORDER } else { BORDER_DIM }));

    let rows: Vec<SprintEntry> = app.history_rows().into_iter().cloned().collect();
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  No issues in this sprint.", Style::default().fg(Color::DarkGray)))
                .block(block),
            area,
        );
        return;
    }

    // Fixed columns: pointer+symbol (4), points (6), outcome (10), added marker (2),
    // carry badge (4). The title gets the rest; the epic column only shows up when
    // there's room (the analysis panel has the per-epic breakdown anyway).
    let flexible = (area.width as usize).saturating_sub(2 + 4 + 6 + 10 + 2 + 4);
    let epic_w = if flexible >= 48 { 14 } else { 0 };
    let title_w = flexible.saturating_sub(epic_w + 1).max(10);

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(idx, e)| {
            let issue = &e.issue;
            let is_sel = focused && idx == app.history_issue_sel;
            let (sym, sym_color, label, label_color) = match e.outcome {
                Outcome::Done => ("✓", Color::Green, "done", Color::Green),
                Outcome::Open => (
                    status_symbol(&issue.status),
                    status_color(&issue.status),
                    if issue.status == Status::InProgress { "in prog" } else { "open" },
                    status_color(&issue.status),
                ),
                Outcome::Carried => ("↪", Color::Yellow, "carried", Color::Yellow),
                Outcome::Removed => ("×", Color::DarkGray, "removed", Color::DarkGray),
            };
            let title_style = match (is_sel, e.outcome) {
                (true, _) => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                (false, Outcome::Done) => Style::default().fg(Color::Gray),
                (false, Outcome::Removed) => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };
            let added = if e.added_mid_sprint(sprint) { " +" } else { "  " };
            let carry = if issue.carry_count > 0 { format!(" ↩{}", issue.carry_count) } else { String::new() };
            let mut spans = vec![
                Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(Color::Magenta)),
                Span::styled(format!("{sym} "), Style::default().fg(sym_color)),
                Span::styled(format!("{:<title_w$}", trunc(&issue.title, title_w)), title_style),
                Span::styled(format!(" {:>3}sp", format_sp(issue.story_points)), Style::default().fg(Color::Magenta)),
                Span::styled(format!("  {label:<8}"), Style::default().fg(label_color)),
                Span::styled(added, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ];
            if epic_w > 0 {
                spans.push(Span::styled(
                    format!(" {:<epic_w$}", trunc(&issue.epic, epic_w)),
                    Style::default().fg(Color::Cyan),
                ));
            }
            spans.push(Span::styled(carry, Style::default().fg(Color::Yellow)));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default().with_offset(app.history_issue_offset);
    if focused {
        state.select(Some(app.history_issue_sel.min(rows.len() - 1)));
    }
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
    app.history_issue_offset = state.offset();
}

fn render_analysis(f: &mut Frame, app: &App, sprint: &Sprint, stats: &SprintStats, area: Rect) {
    if area.height < 4 {
        return;
    }
    let dim = Style::default().fg(Color::DarkGray);
    let label = |s: &str| Span::styled(format!("  {s:<11}"), dim);

    // ── Velocity: sp/day of up to 5 completed sprints that ended before this one ──
    let prior_spd: Vec<f64> = app
        .history_sprints
        .iter()
        .filter(|s| !s.is_active && s.end_date < sprint.end_date)
        .map(|s| SprintStats::of(s, entries_of(app, s)).done / s.duration_days() as f64)
        .collect(); // newest first, like history_sprints
    let window = prior_spd.len().min(5);
    let velocity = (window > 0).then(|| prior_spd[..window].iter().sum::<f64>() / window as f64);
    // Trend over prior sprints: newest 2 sp/day vs the 2 before that
    let trend = if prior_spd.len() >= 4 {
        let recent = (prior_spd[0] + prior_spd[1]) / 2.0;
        let older = (prior_spd[2] + prior_spd[3]) / 2.0;
        if recent > older * 1.05 { ("↑", Color::Green) } else if recent < older * 0.95 { ("↓", Color::Red) } else { ("→", Color::DarkGray) }
    } else {
        ("", Color::DarkGray)
    };
    // Safe start: p25 of prior sp/day (never above the average) × this sprint's length.
    let safe_start = if prior_spd.len() >= 3 {
        let mut sorted = prior_spd[..prior_spd.len().min(10)].to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p25 = sorted[(sorted.len() as f64 * 0.25) as usize].min(velocity.unwrap_or(f64::MAX));
        Some(p25 * sprint.duration_days() as f64)
    } else {
        velocity.map(|v| v * sprint.duration_days() as f64)
    };

    let mut lines: Vec<Line> = vec![Line::from("")];

    lines.push(Line::from(match velocity {
        Some(v) => vec![
            label("velocity"),
            Span::styled(format!("{v:.2}sp/day"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", trend.0), Style::default().fg(trend.1).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  last {window}"), dim),
        ],
        None => vec![label("velocity"), Span::styled("no earlier sprints yet", dim)],
    }));

    let pct = stats.completion() * 100.0;
    let pct_color = if pct >= 80.0 { Color::Green } else if pct >= 50.0 { Color::Yellow } else { Color::Red };
    lines.push(Line::from(vec![
        label("completed"),
        Span::styled(format!("{pct:.0}%"), Style::default().fg(pct_color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}/{}sp", format_sp(stats.done), format_sp(stats.scope())), dim),
    ]));

    // How much of what was planned on day one got done.
    if stats.planned > 0.0 {
        let kept = stats.planned_done / stats.planned * 100.0;
        let kept_color = if kept >= 80.0 { Color::Green } else if kept >= 50.0 { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            label("plan kept"),
            Span::styled(format!("{kept:.0}%"), Style::default().fg(kept_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  of {}sp planned", format_sp(stats.planned)), dim),
        ]));
    }

    if let Some(target) = safe_start {
        let rounded = (target * 10.0).round() / 10.0;
        let ok = stats.planned <= target * 1.1;
        lines.push(Line::from(vec![
            label("safe start"),
            Span::styled(
                format!("≤{}sp", format_sp(rounded)),
                Style::default().fg(if ok { Color::Green } else { Color::Yellow }).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  planned {}sp", format_sp(stats.planned)), dim),
        ]));
    }

    // Issues carried into this sprint from earlier ones, and how many got finished.
    let carried_out_before: HashSet<i64> = app
        .history_sprints
        .iter()
        .filter(|s| s.start_date < sprint.start_date)
        .flat_map(|s| entries_of(app, s).iter())
        .filter(|e| e.outcome == Outcome::Carried)
        .map(|e| e.issue.id)
        .collect();
    let carried_in: Vec<&SprintEntry> = entries_of(app, sprint)
        .iter()
        .filter(|e| e.outcome != Outcome::Removed && carried_out_before.contains(&e.issue.id))
        .collect();
    if !carried_in.is_empty() {
        let done = carried_in.iter().filter(|e| e.outcome == Outcome::Done).count();
        let color = if done == carried_in.len() { Color::Green } else { Color::Yellow };
        lines.push(Line::from(vec![
            label("carried in"),
            Span::styled(format!("{} → {done} done", carried_in.len()), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]));
    }

    // Where the points went, by epic.
    let mut by_epic: HashMap<&str, (f64, f64)> = HashMap::new();
    for e in entries_of(app, sprint).iter().filter(|e| e.outcome != Outcome::Removed) {
        let entry = by_epic.entry(e.issue.epic.as_str()).or_default();
        entry.0 += e.issue.story_points;
        if e.outcome == Outcome::Done {
            entry.1 += e.issue.story_points;
        }
    }
    let mut epics: Vec<(&str, (f64, f64))> = by_epic.into_iter().collect();
    epics.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
    let room = (area.height as usize).saturating_sub(lines.len() + 3);
    if !epics.is_empty() && room > 0 {
        lines.push(Line::from(""));
        let inner_w = area.width.saturating_sub(2) as usize;
        let name_w = 12usize;
        let bar_w = inner_w.saturating_sub(4 + name_w + 10).clamp(4, 16);
        for (epic, (total, done)) in epics.into_iter().take(room) {
            let mut spans = vec![Span::styled(format!("  {:<name_w$} ", trunc(epic, name_w)), Style::default().fg(Color::Cyan))];
            spans.extend(progress_bar(if total > 0.0 { done / total } else { 0.0 }, bar_w, Color::Green));
            spans.push(Span::styled(format!(" {}/{}sp", format_sp(done), format_sp(total)), dim));
            lines.push(Line::from(spans));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(Span::styled(
                    " analysis ",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                )))
                .border_style(Style::default().fg(BORDER_DIM)),
        ),
        area,
    );
}
