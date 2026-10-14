//! Screen rendering for the interactive protocol example.

use super::{App, Task};
use osc7501::State;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};

const BACKGROUND: Color = Color::Rgb(17, 22, 34);
const TEXT: Color = Color::Rgb(219, 228, 244);
const MUTED: Color = Color::Rgb(142, 157, 183);
const ACCENT: Color = Color::Rgb(119, 185, 255);

fn color(state: State) -> Color {
    match state {
        State::Working => ACCENT,
        State::Blocked => Color::Rgb(255, 207, 112),
        State::Done => Color::Rgb(118, 218, 163),
        State::Error => Color::Rgb(255, 128, 147),
        _ => MUTED,
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
}

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND).fg(TEXT)),
        area,
    );
    if area.width < 80 || area.height < 24 {
        frame.render_widget(
            Paragraph::new(format!(
                "OSC 7501 playground\n\n{}: {}\n\nResize to at least 80 x 24.\ns start  Enter resume  c cancel  q quit",
                app.state.as_str(), app.message
            ))
            .style(Style::default().fg(color(app.state)))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [header, status, body, log, help] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(14),
        Constraint::Length(if area.height >= 29 { 5 } else { 0 }),
        Constraint::Length(5),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                " OSC 7501 / release playground",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                " Simulated work. Real status reports. b switches the API preview.",
                Style::default().fg(MUTED),
            )),
        ]),
        header,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", app.state.as_str().to_uppercase()),
                Style::default()
                    .bg(color(app.state))
                    .fg(BACKGROUND)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {}", app.message)),
        ]))
        .block(panel(" Program / root ")),
        status,
    );
    let [tasks, preview] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(body);
    draw_tasks(frame, app, tasks);
    draw_api(frame, app, preview);
    let items: Vec<ListItem<'_>> = app
        .log
        .iter()
        .rev()
        .take(3)
        .map(|(state, label)| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:7} ", state.as_str()),
                    Style::default().fg(color(*state)),
                ),
                Span::raw(label.as_str()),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel(&format!(" Reports sent / {} ", app.reports))),
        log,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("s start/restart   Up/Down/Tab select   b root/child API"),
            Line::from("p permission   a auth   ? question   Enter answer/resume"),
            Line::from("f fail   r retry   c/Esc cancel + clear   q/Ctrl+C quit"),
        ])
        .style(Style::default().fg(MUTED))
        .block(panel(" Controls ")),
        help,
    );
}

fn draw_tasks(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel(" Tasks / child records ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([Constraint::Length(4); 3]).split(inner);
    for (index, (task, row)) in app.tasks.iter().zip(rows.iter()).enumerate() {
        let marker = if app.selected == index { ">" } else { " " };
        let [label, gauge] =
            Layout::vertical([Constraint::Length(2), Constraint::Length(1)]).areas(*row);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("{marker} {}", task.title),
                    Style::default()
                        .fg(color(task.state))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  {}", task.state.as_str()),
                    Style::default().fg(color(task.state)),
                )),
            ]),
            label,
        );
        frame.render_widget(
            Gauge::default()
                .percent(u16::from(task.progress))
                .label(format!("{}%", task.progress))
                .gauge_style(
                    Style::default()
                        .fg(color(task.state))
                        .bg(Color::Rgb(33, 43, 61)),
                ),
            gauge,
        );
    }
}

fn task_code(task: &Task) -> Vec<String> {
    let mut lines = vec![
        format!("Status::new(State::{:?})", task.state),
        format!("    .id(RecordId::new({:?})?)", task.id.as_str()),
        format!("    .title({:?})", task.title),
        format!("    .message({:?})", task.message()),
    ];
    if task.state == State::Working {
        lines.push(format!("    .progress(Progress::new({})?)", task.progress));
    }
    lines.push("    .write_to(terminal.backend_mut())?;".into());
    lines
}

fn draw_api(frame: &mut Frame, app: &App, area: Rect) {
    if !app.inspect_root {
        frame.render_widget(
            Paragraph::new(task_code(&app.tasks[app.selected]).join("\n"))
                .style(Style::default().fg(ACCENT))
                .wrap(Wrap { trim: false })
                .block(panel(" osc7501 / child inherits root app ")),
            area,
        );
        return;
    }
    let mut lines = vec![
        Line::from(format!("Status::new(State::{:?})", app.state)),
        Line::from("    .app(AppId::new(\"osc7501-tui\")?)"),
        Line::from("    .title(\"Release playground\")"),
        Line::from(format!("    .message({:?})", app.message)),
    ];
    if let Some(kind) = app.kind {
        lines.push(Line::from(format!("    .kind(Kind::{kind:?})")));
    }
    if matches!(app.state, State::Working | State::Blocked) {
        lines.push(Line::from(format!(
            "    .progress(Progress::new({})?)",
            app.progress()
        )));
    }
    lines.push(Line::from("    .write_to(terminal.backend_mut())?;"));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(ACCENT))
            .wrap(Wrap { trim: false })
            .block(panel(" osc7501 / root builder API ")),
        area,
    );
}
