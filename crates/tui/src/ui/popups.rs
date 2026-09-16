use crate::app::App;
use crate::app::popup::{
    FilterForm, JoinForm, LaunchState, Popup, ServerSettingsForm, SettingsForm, SettingsRow, sort_label,
};
use crate::input::Input;
use crate::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let Some(popup) = &app.popup else { return };
    match popup {
        Popup::Join(form) => draw_join(f, area, form),
        Popup::AddServer(input) => {
            draw_prompt(f, area, "Add server", "host:port or ip:port (port defaults to 7777)", input)
        }
        Popup::PathPrompt(p) => draw_prompt(f, area, &p.title, &p.hint, &p.input),
        Popup::Filters(form) => draw_filters(f, app, area, form),
        Popup::Settings(form) => draw_settings(f, app, area, form),
        Popup::ServerSettings(form) => draw_server_settings(f, area, form),
        Popup::Help => draw_help(f, area),
        Popup::Message { title, lines, error } => draw_message(f, area, title, lines, *error),
        Popup::Confirm { title, text, .. } => draw_confirm(f, area, title, text),
        Popup::Launch(st) => draw_launch(f, area, st),
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2)).max(10).min(area.width);
    let h = height.min(area.height.saturating_sub(2)).max(3).min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn frame(f: &mut Frame, area: Rect, title: &str, error: bool) -> Rect {
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(Span::styled(format!(" {title} "), if error { theme::err() } else { theme::title() })))
        .border_style(if error { theme::err() } else { theme::border_focus() });
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

fn field<'a>(label: &'a str, input: &Input, active: bool) -> Line<'a> {
    let style = if active { theme::key() } else { theme::text() };
    Line::from(vec![
        Span::styled(format!("{label:<12}"), theme::dim()),
        Span::styled(if active { "▏" } else { " " }, style),
        Span::styled(input.display(), style),
    ])
}

fn set_cursor(f: &mut Frame, inner: Rect, row: u16, label_width: u16, input: &Input) {
    let x = inner.x + label_width + 1 + input.cursor() as u16;
    f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y + row));
}

fn draw_join(f: &mut Frame, area: Rect, form: &JoinForm) {
    let rect = centered(area, 64, 12);
    let inner = frame(f, rect, "Join server", false);
    let s = &form.server;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(s.display_name(), theme::title()),
            Span::raw("  "),
            Span::styled(s.address_text(), theme::dim()),
        ]),
        Line::from(Span::styled(if s.info.password { "server has a password" } else { "" }, theme::warn())),
        field("nickname", &form.nickname, form.field == 0),
        field("password", &form.password, form.field == 1),
        Line::from(vec![
            Span::styled(format!("{:<12}", ""), theme::dim()),
            Span::styled(
                format!("[{}] remember password", if form.remember_password { "x" } else { " " }),
                if form.field == 2 { theme::key() } else { theme::text() },
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<12}", "SA-MP"), theme::dim()),
            Span::styled(
                format!("{}{}", if form.field == 3 { "< " } else { "  " }, form.samp_version.label()),
                if form.field == 3 { theme::key() } else { theme::text() },
            ),
            Span::styled(if form.field == 3 { " >" } else { "" }, theme::key()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw(format!("{:<12}", "")),
            Span::styled("[ Join ]", if form.field == 4 { theme::tab_active() } else { theme::key() }),
        ]),
    ];
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(e.clone(), theme::err())));
    } else {
        lines.push(Line::from(Span::styled("Enter join  Tab next field  v or ←/→ version  Esc cancel", theme::dim())));
    }
    f.render_widget(Paragraph::new(lines), inner);
    match form.field {
        0 => set_cursor(f, inner, 2, 12, &form.nickname),
        1 => set_cursor(f, inner, 3, 12, &form.password),
        _ => {}
    }
}

fn draw_prompt(f: &mut Frame, area: Rect, title: &str, hint: &str, input: &Input) {
    let rect = centered(area, 80, 6);
    let inner = frame(f, rect, title, false);
    let lines = vec![
        Line::from(Span::styled(hint, theme::dim())),
        Line::raw(""),
        Line::from(vec![Span::styled("> ", theme::key()), Span::styled(input.display(), theme::text())]),
        Line::from(Span::styled("Enter confirm  Esc cancel", theme::dim())),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    let x = inner.x + 2 + input.cursor() as u16;
    f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y + 2));
}

fn draw_filters(f: &mut Frame, app: &App, area: Rect, form: &FilterForm) {
    let rect = centered(area, 50, (form.rows() + 5).min(30) as u16);
    let inner = frame(f, rect, "Filters & sort", false);
    let check = |b: bool| if b { "[x]" } else { "[ ]" };
    let fl = &app.filters;
    let mut rows = vec![
        format!("{} open.mp servers only", check(fl.omp_only)),
        format!("{} hide empty servers", check(fl.non_empty)),
        format!("{} hide passworded servers", check(fl.unpassworded)),
        format!("sort by: {}", sort_label(fl.sort, fl.dir)),
        format!("direction: {}", if fl.dir == omptui_core::filter::SortDir::Asc { "ascending" } else { "descending" }),
    ];
    rows.extend(form.languages.iter().map(|(l, n)| format!("{} {l} ({n})", check(fl.languages.contains(l)))));
    let mut display: Vec<Line> = Vec::new();
    let mut cursor_line = 0;
    for (i, text) in rows.iter().enumerate() {
        if i == FilterForm::FIXED {
            display.push(Line::from(Span::styled(" languages", theme::dim())));
        }
        if i == form.cursor {
            cursor_line = display.len();
        }
        let style = if i == form.cursor { theme::selected() } else { theme::text() };
        display.push(Line::from(Span::styled(format!(" {text} "), style)));
    }
    let visible = inner.height.saturating_sub(1) as usize;
    let start = cursor_line.saturating_sub(visible.saturating_sub(1));
    let mut lines: Vec<Line> = display.into_iter().skip(start).take(visible).collect();
    lines.push(Line::from(Span::styled("Space toggle  c clear  Esc close", theme::dim())));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_settings(f: &mut Frame, app: &App, area: Rect, form: &SettingsForm) {
    let rect = centered(area, 96, 27);
    let inner = frame(f, rect, "Settings", false);
    let [list_area, msg_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).areas(inner);
    let mut lines = Vec::new();
    for (i, row) in SettingsRow::ALL.iter().enumerate() {
        let active = i == form.cursor;
        let style = if active { theme::selected() } else { theme::text() };
        if *row == SettingsRow::ActionCheckFiles {
            lines.push(Line::raw(""));
        }
        let line = if row.is_action() {
            Line::from(Span::styled(
                format!(" {} ", row.label()),
                if active { theme::selected() } else { theme::key() },
            ))
        } else if active && form.editing.is_some() {
            let input = form.editing.as_ref().unwrap();
            Line::from(vec![
                Span::styled(format!(" {:<38}", row.label()), theme::dim()),
                Span::styled(format!("▏{}", input.display()), theme::key()),
            ])
        } else {
            let value = app.settings_display(*row);
            Line::from(vec![
                Span::styled(format!(" {:<38}", row.label()), if active { style } else { theme::dim() }),
                Span::styled(value, style),
            ])
        };
        lines.push(line);
    }
    f.render_widget(Paragraph::new(lines), list_area);
    let hint = match &form.message {
        Some(m) => Line::from(Span::styled(m.clone(), theme::err())),
        None if form.editing.is_some() => Line::from(Span::styled("Enter save  Esc cancel edit", theme::dim())),
        None => Line::from(Span::styled(
            "Enter edit/toggle/run  ←/→ cycle  Esc close (settings are saved immediately)",
            theme::dim(),
        )),
    };
    f.render_widget(Paragraph::new(hint).wrap(Wrap { trim: true }), msg_area);
    if let Some(input) = &form.editing {
        let extra =
            if form.cursor >= SettingsRow::ALL.iter().position(|r| *r == SettingsRow::ActionCheckFiles).unwrap_or(99) {
                1
            } else {
                0
            };
        let y = list_area.y + form.cursor as u16 + extra;
        let x = list_area.x + 1 + 38 + 1 + input.cursor() as u16;
        f.set_cursor_position((x.min(list_area.right().saturating_sub(1)), y));
    }
}

fn draw_server_settings(f: &mut Frame, area: Rect, form: &ServerSettingsForm) {
    let rect = centered(area, 64, 10);
    let inner = frame(f, rect, "Server settings", false);
    let version = form.samp_version.map(|v| v.label().to_string()).unwrap_or_else(|| "(global setting)".into());
    let lines = vec![
        Line::from(vec![
            Span::styled(form.name.clone(), theme::title()),
            Span::raw("  "),
            Span::styled(form.addr.to_string(), theme::dim()),
        ]),
        Line::raw(""),
        field("nickname", &form.nickname, form.field == 0),
        field("password", &form.password, form.field == 1),
        Line::from(vec![
            Span::styled(format!("{:<12}", "SA-MP"), theme::dim()),
            Span::styled(format!(" {version}"), if form.field == 2 { theme::key() } else { theme::text() }),
        ]),
        Line::raw(""),
        match &form.error {
            Some(e) => Line::from(Span::styled(e.clone(), theme::err())),
            None => Line::from(Span::styled(
                "empty = use global setting.  Enter save  Esc cancel  Tab next  ←/→ change version",
                theme::dim(),
            )),
        },
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    match form.field {
        0 => set_cursor(f, inner, 2, 12, &form.nickname),
        1 => set_cursor(f, inner, 3, 12, &form.password),
        _ => {}
    }
}

const HELP: &[(&str, &str)] = &[
    ("1-4 / Tab", "switch list: favorites, internet, partners, recent"),
    ("j/k ↑/↓ PgUp/PgDn g/G", "move"),
    ("Enter", "join the selected server"),
    ("F / Space", "add or remove favorite"),
    ("a", "add a server by address (also hostnames)"),
    ("d / Del", "remove from favorites or recent"),
    ("J / K", "reorder favorites"),
    ("/", "search (Esc clears)"),
    ("f", "filters and sort menu"),
    ("o / e / n", "toggle: open.mp only, hide empty, hide passworded"),
    ("s / S", "cycle sort key / flip direction"),
    ("r / R", "refresh master list / re-query servers"),
    ("c", "copy address to clipboard"),
    ("w / D", "open the server's website / Discord"),
    ("p", "per-server nickname, password, SA-MP version"),
    ("x", "clear recently joined"),
    ("l", "show the last launch log"),
    ("i", "import client files from a launcher data folder"),
    (",", "settings"),
    ("q", "quit"),
];

fn draw_help(f: &mut Frame, area: Rect) {
    let rect = centered(area, 72, (HELP.len() + 5) as u16);
    let inner = frame(f, rect, "Keys", false);
    let mut lines: Vec<Line> = HELP
        .iter()
        .map(|(k, v)| {
            Line::from(vec![Span::styled(format!(" {k:<22}"), theme::key()), Span::styled(*v, theme::text())])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(" ◆ open.mp server   ○ SA-MP server   ★ favorite   🔒 password", theme::dim())));
    lines.push(Line::from(Span::styled(" mouse: click selects, double-click joins, wheel scrolls", theme::dim())));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_message(f: &mut Frame, area: Rect, title: &str, lines: &[String], error: bool) {
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(20).clamp(30, 110) as u16 + 4;
    let rect = centered(area, width, (lines.len() + 4).min(40) as u16);
    let inner = frame(f, rect, title, error);
    let mut text: Vec<Line> = lines.iter().map(|l| Line::from(Span::styled(l.clone(), theme::text()))).collect();
    text.push(Line::raw(""));
    text.push(Line::from(Span::styled("Esc / Enter close", theme::dim())));
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

fn draw_confirm(f: &mut Frame, area: Rect, title: &str, text: &str) {
    let rect = centered(area, 60, 6);
    let inner = frame(f, rect, title, false);
    let lines = vec![
        Line::from(Span::styled(text, theme::text())),
        Line::raw(""),
        Line::from(vec![
            Span::styled("y / Enter", theme::key()),
            Span::styled(" confirm   ", theme::dim()),
            Span::styled("n / Esc", theme::key()),
            Span::styled(" cancel", theme::dim()),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).alignment(Alignment::Left), inner);
}

fn draw_launch(f: &mut Frame, area: Rect, st: &LaunchState) {
    let rect = centered(area, 100, 22);
    let title = if st.failed {
        format!("Launch failed: {}", st.server)
    } else if st.finished {
        format!("Finished: {}", st.server)
    } else {
        format!("Launching {}", st.server)
    };
    let inner = frame(f, rect, &title, st.failed);
    let visible = inner.height.saturating_sub(1) as usize;
    let start = st.lines.len().saturating_sub(visible);
    let mut lines: Vec<Line> = st
        .lines
        .iter()
        .skip(start)
        .map(|(l, err)| Line::from(Span::styled(l.clone(), if *err { theme::err() } else { theme::text() })))
        .collect();
    let footer = if st.finished { "Esc close   l reopen later" } else { "Esc hide (the game keeps running)   c clear" };
    lines.push(Line::from(Span::styled(footer, theme::dim())));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).style(Style::default()), inner);
}
