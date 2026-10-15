//! The release playground, rendered from the host's retained records.

use super::scenario::App;
use osc7501::{AppId, RecordId, State};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Wrap},
};

const BACKGROUND: Color = Color::Rgb(17, 22, 34);
const TEXT: Color = Color::Rgb(219, 228, 244);
const MUTED: Color = Color::Rgb(142, 157, 183);
const ACCENT: Color = Color::Rgb(119, 185, 255);
const HOST: Color = Color::Rgb(103, 218, 208);

fn color(state: State) -> Color {
    match state {
        State::Working => ACCENT,
        State::Blocked => Color::Rgb(255, 207, 112),
        State::Done => Color::Rgb(118, 218, 163),
        State::Error => Color::Rgb(255, 128, 147),
        _ => MUTED,
    }
}

fn bold(tint: Color) -> Style {
    Style::default().fg(tint).add_modifier(Modifier::BOLD)
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
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
                "OSC 7501 / release playground\nTerminal host / osc7501-store\n\n{} retained records\n\nResize to at least 80 x 24.\ns start  X exit  R reset  q quit",
                app.host.records.len(),
            ))
            .style(Style::default().fg(HOST))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [header, transport, root, body, log, insight, controls] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(11),
        Constraint::Length(if area.height >= 30 { 5 } else { 0 }),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" OSC 7501 / release playground", bold(ACCENT)),
                Span::styled("  + terminal host", bold(HOST)),
            ]),
            Line::styled(
                " Same release. Same controls. The terminal now keeps its reports.",
                Style::default().fg(MUTED),
            ),
        ]),
        header,
    );
    draw_transport(frame, app, transport);
    draw_root(frame, app, root);
    let [tasks, inspector] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);
    draw_tasks(frame, app, tasks);
    draw_inspector(frame, app, inspector);
    draw_events(frame, app, log);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(" HOST INSIGHT", bold(HOST)),
            Line::raw(format!(" {}", app.insight)),
        ])
        .wrap(Wrap { trim: true }),
        insight,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(" s start   Up/Down/Tab select   b root/child   i fields/API   q quit"),
            Line::raw(
                " p permission  a auth  ? question  Enter resume  f fail  r retry  c/Esc cancel",
            ),
            Line::from(vec![
                Span::styled(" Host: ", bold(HOST)),
                Span::raw("P prompt   X exit   S soft reset   R full reset"),
            ]),
        ])
        .style(Style::default().fg(MUTED)),
        controls,
    );
}

fn draw_transport(frame: &mut Frame, app: &App, area: Rect) {
    let program = app.program.as_ref().map_or_else(
        || "PROGRAM EXITED".to_owned(),
        |program| format!("PROGRAM {}", program.state.as_str().to_uppercase()),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" osc7501", bold(ACCENT)),
                Span::raw(" -> output bytes -> Decoder -> "),
                Span::styled("osc7501-store", bold(HOST)),
            ]),
            Line::from(vec![
                Span::styled(
                    format!(" {program}"),
                    bold(if app.program.is_some() { ACCENT } else { HOST }),
                ),
                Span::styled(
                    format!(
                        "  /  {} reports  /  {} of {} slots  /  input released",
                        app.host.reports_received,
                        app.host.records.len(),
                        app.host.records.capacity()
                    ),
                    Style::default().fg(MUTED),
                ),
            ]),
        ]),
        area,
    );
}

fn draw_root(frame: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(record) = app.host.records.get(None) {
        Line::from(vec![
            Span::styled(
                format!(" {} ", record.state().as_str().to_uppercase()),
                Style::default()
                    .bg(color(record.state()))
                    .fg(BACKGROUND)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  {}",
                record.message().unwrap_or("No message reported")
            )),
        ])
    } else {
        Line::styled(
            " No root record. New reports can recreate it.",
            Style::default().fg(MUTED),
        )
    };
    frame.render_widget(
        Paragraph::new(line).block(panel(" Host / root record ")),
        area,
    );
}

fn draw_tasks(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel(" Host / child records ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let row_height = if inner.height >= 12 { 4 } else { 3 };
    let rows = Layout::vertical([Constraint::Length(row_height); 3]).split(inner);
    for (index, ((id, title), row)) in app.tasks.iter().zip(rows.iter()).enumerate() {
        let record = app.host.records.get(Some(*id));
        let tint = record.map_or(MUTED, |record| color(record.state()));
        let marker = if !app.inspect_root && app.selected == index {
            ">"
        } else {
            " "
        };
        let [label, gauge] =
            Layout::vertical([Constraint::Length(2), Constraint::Length(1)]).areas(*row);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(format!("{marker} {title}"), bold(tint)),
                Line::styled(
                    format!(
                        "  {}",
                        record.map_or("no stored record", |record| record.state().as_str())
                    ),
                    Style::default().fg(tint),
                ),
            ]),
            label,
        );
        let percent = record.map_or(0, |record| {
            if record.state() == State::Done {
                100
            } else {
                record.progress().map_or(0, osc7501::Progress::get)
            }
        });
        let label = record.map_or_else(
            || "not retained".to_owned(),
            |record| {
                record.progress().map_or_else(
                    || record.state().as_str().to_owned(),
                    |progress| format!("{}%", progress.get()),
                )
            },
        );
        frame.render_widget(
            Gauge::default()
                .percent(u16::from(percent))
                .label(label)
                .gauge_style(Style::default().fg(tint).bg(Color::Rgb(33, 43, 61))),
            gauge,
        );
    }
}

fn draw_inspector(frame: &mut Frame, app: &App, area: Rect) {
    if app.inspect_api {
        let id = app.selected_id().map_or_else(
            || "None".to_owned(),
            |id| format!("Some(RecordId::new({:?})?)", id.as_str()),
        );
        let code = format!(
            "// Accepted Decoder::feed report:\nresult = store.apply(report, |_| {{}});\n\n// After feed, release input and draw:\nresult?;\nlet id = {id};\nlet record = store.get(id);\nlet app = store.effective_app(id);"
        );
        let mut lines: Vec<_> = code
            .lines()
            .map(|line| Line::raw(line.to_owned()))
            .collect();
        if area.height >= 15 {
            lines.extend([
                Line::raw(""),
                Line::styled(
                    "// Terminal lifecycle, supplied by host:",
                    Style::default().fg(MUTED),
                ),
                Line::raw("store.on_process_exit(|_| {});"),
                Line::raw("// done and error survive"),
                Line::raw("store.on_full_reset(|_| {});"),
            ]);
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().fg(HOST))
                .wrap(Wrap { trim: false })
                .block(panel(" osc7501-store / i for fields ")),
            area,
        );
        return;
    }
    let Some(record) = app.selected_record() else {
        frame.render_widget(
            Paragraph::new("\n No stored record for this selection.\n\n Start with s, or select a retained task.\n\n Completed and failed results survive\n process exit and the next shell prompt.")
                .wrap(Wrap { trim: false }).style(Style::default().fg(MUTED))
                .block(panel(" osc7501-store / retained fields ")),
            area,
        );
        return;
    };
    let inherited = record.app().is_none();
    let app_id = app.host.records.effective_app(record.id());
    let mut lines = vec![
        Line::styled(
            record.title().unwrap_or("Untitled record"),
            bold(color(record.state())),
        ),
        field("ID", record.id().map_or("<root>", RecordId::as_str)),
        field("State", record.state().as_str()),
        field(
            "App",
            format!(
                "{}{}",
                app_id.map_or("-", AppId::as_str),
                if inherited && app_id.is_some() {
                    " (inherited)"
                } else {
                    ""
                }
            ),
        ),
    ];
    if let Some(kind) = record.kind() {
        lines.push(field("Kind", kind.as_str()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(
        record.message().unwrap_or("No message in this report"),
    ));
    if area.height >= 14 {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Read-only decoded fields.",
            Style::default().fg(HOST),
        ));
        lines.push(Line::styled(
            "The input buffer is already gone.",
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel(" osc7501-store / i for API ")),
        area,
    );
}

fn field(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:8}"), Style::default().fg(MUTED)),
        Span::raw(value.into()),
    ])
}

fn draw_events(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<_> = app
        .log
        .iter()
        .rev()
        .take(3)
        .map(|(state, message)| {
            Line::from(vec![
                Span::styled(
                    format!(" {:7} ", state.as_str()),
                    Style::default().fg(color(*state)),
                ),
                Span::raw(message.as_str()),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(panel(" Host events / history stays outside the store ")),
        area,
    );
}
