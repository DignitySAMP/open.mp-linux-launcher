pub mod popups;

use crate::app::{App, ping_text};
use crate::theme;
use omptui_core::ListKind;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table, Wrap,
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)]).areas(area);
    draw_header(f, app, header);
    let right_width = if area.width >= 110 { 40 } else { (area.width / 3).max(24) };
    let [left, right] = Layout::horizontal([Constraint::Min(30), Constraint::Length(right_width)]).areas(body);
    let [list_area, details_area] =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(left);
    let [players_area, graph_area] =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(right);
    draw_list(f, app, list_area);
    draw_details(f, app, details_area);
    draw_players(f, app, players_area);
    draw_graph(f, app, graph_area);
    draw_footer(f, app, footer);
    popups::draw(f, app, area);
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = vec![Span::styled(" omp-tui ", theme::title())];
    let mut x = area.x + 9;
    app.hit.tabs.clear();
    for (i, kind) in ListKind::ALL.iter().enumerate() {
        let label = format!(" {} {} ({}) ", i + 1, kind.title(), app.list_len(*kind));
        let width = label.chars().count() as u16;
        app.hit.tabs.push((Rect::new(x, area.y, width, 1), *kind));
        x += width + 1;
        let style = if *kind == app.tab { theme::tab_active() } else { theme::tab_inactive() };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    let search_text = if app.search_editing || !app.filters.query.is_empty() {
        format!("/{}", app.search.display())
    } else {
        String::from("/ search")
    };
    x += 2;
    app.hit.search = Rect::new(x, area.y, search_text.chars().count().max(8) as u16, 1);
    spans.push(Span::styled("  ", theme::dim()));
    spans.push(Span::styled(search_text, if app.search_editing { theme::key() } else { theme::dim() }));
    let active = app.filters.active_count();
    if active > 0 || app.filters.sort != omptui_core::filter::SortKey::None {
        let mut parts = Vec::new();
        if app.filters.omp_only {
            parts.push("omp".to_string());
        }
        if app.filters.non_empty {
            parts.push("non-empty".to_string());
        }
        if app.filters.unpassworded {
            parts.push("open".to_string());
        }
        if !app.filters.gamemode.trim().is_empty() {
            parts.push(format!("mode {}", app.filters.gamemode.trim()));
        }
        if !app.filters.versions.is_empty() {
            parts.push(app.filters.versions.iter().cloned().collect::<Vec<_>>().join("/"));
        }
        if !app.filters.languages.is_empty() {
            parts.push(format!("{} lang", app.filters.languages.len()));
        }
        if app.filters.sort != omptui_core::filter::SortKey::None {
            parts.push(format!("sort {} {}", app.filters.sort.label(), app.filters.dir.arrow()));
        }
        spans.push(Span::styled(format!("  [{}]", parts.join(", ")), theme::warn()));
    }
    if app.loading {
        spans.push(Span::styled("  loading…", theme::warn()));
    }
    if app.game_running {
        spans.push(Span::styled("  ● game running", theme::ok()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    if app.search_editing {
        let cx = x + 1 + app.search.cursor() as u16;
        f.set_cursor_position((cx.min(area.right().saturating_sub(1)), area.y));
    }
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.tab {
        ListKind::Favorites => " Favorites ",
        ListKind::Internet => " Internet ",
        ListKind::Partners => " Partners ",
        ListKind::Recent => " Recently joined ",
    };
    let count = format!(" {}/{} ", app.view.len(), app.list().len());
    let block = Block::bordered()
        .title(Line::from(Span::styled(title, theme::title())))
        .title_bottom(Line::from(Span::styled(count, theme::dim())).right_aligned())
        .border_style(theme::border_focus());
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.hit.list = area;
    app.hit.rows = Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner };
    if app.view.is_empty() {
        let text = if let Some(e) = &app.api_error {
            format!("master list unavailable: {e}\npress r to retry")
        } else if app.loading {
            "loading master list…".to_string()
        } else if !app.list().is_empty() {
            "no servers match the current search/filters".to_string()
        } else if matches!(app.tab, ListKind::Favorites) {
            "no favorites yet\npress F on a server to add it, or a to add by address".to_string()
        } else if matches!(app.tab, ListKind::Recent) {
            "no recently joined servers".to_string()
        } else {
            "no servers".to_string()
        };
        f.render_widget(
            Paragraph::new(text).style(theme::dim()).alignment(Alignment::Center).wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let favorites = &app.lists.favorites;
    let rows: Vec<Row> = app
        .view
        .iter()
        .map(|&i| {
            let s = &app.list()[i];
            let fav = if favorites.iter().any(|x| x.addr == s.addr) { "★" } else { " " };
            let kind = if s.info.omp {
                Span::styled("◆", Style::default().fg(theme::OMP))
            } else {
                Span::styled("○", Style::default().fg(theme::SAMP))
            };
            let lock = if s.info.password { Span::styled("🔒", theme::warn()) } else { Span::raw("  ") };
            let name = Line::from(vec![
                Span::styled(fav, theme::warn()),
                Span::raw(" "),
                kind,
                Span::raw(" "),
                Span::styled(s.display_name(), theme::text()),
            ]);
            let players = format!("{}/{}", s.info.players, s.info.max_players);
            let ping = ping_text(s.ping);
            Row::new(vec![
                Cell::from(name),
                Cell::from(Line::from(players).alignment(Alignment::Right)),
                Cell::from(Line::from(Span::styled(ping, theme::ping_style(s.ping))).alignment(Alignment::Right)),
                Cell::from(lock),
            ])
        })
        .collect();
    let header = Row::new(vec![
        Cell::from("server"),
        Cell::from(Line::from("players").alignment(Alignment::Right)),
        Cell::from(Line::from("ping").alignment(Alignment::Right)),
        Cell::from(""),
    ])
    .style(theme::header());
    let table =
        Table::new(rows, [Constraint::Min(20), Constraint::Length(9), Constraint::Length(7), Constraint::Length(2)])
            .header(header)
            .column_spacing(1)
            .row_highlight_style(theme::selected())
            .highlight_symbol("▶ ");
    f.render_stateful_widget(table, inner, &mut app.table);
    if app.view.len() > inner.height as usize {
        let mut sb = ScrollbarState::new(app.view.len()).position(app.table.selected().unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
            &mut sb,
        );
    }
}

fn kv<'a>(k: &'a str, v: String) -> Line<'a> {
    Line::from(vec![Span::styled(format!("{k:<10}"), theme::dim()), Span::styled(v, theme::text())])
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let block =
        Block::bordered().title(Line::from(Span::styled(" Details ", theme::title()))).border_style(theme::border());
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(s) = app.selected_server() else {
        f.render_widget(Paragraph::new("select a server").style(theme::dim()), inner);
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(s.display_name(), theme::title()),
            Span::raw("  "),
            Span::styled(s.address_text(), theme::dim()),
        ]),
        kv("mode", s.info.gamemode.clone()),
        kv("language", s.info.language.clone()),
        kv(
            "players",
            format!(
                "{}/{}{}",
                s.info.players,
                s.info.max_players,
                if s.info.password { "   password protected" } else { "" }
            ),
        ),
        kv(
            "version",
            format!(
                "{}{}",
                if s.info.version.is_empty() { "?".to_string() } else { s.info.version.clone() },
                if s.info.omp { "  (open.mp)" } else { "" }
            ),
        ),
        kv("ping", format!("{}{}", ping_text(s.ping), if s.info.partner { "   partner server" } else { "" })),
    ];
    let per = s.addr.map(|a| app.lists.settings_for(a)).unwrap_or_default();
    if !per.is_empty() {
        let mut parts = Vec::new();
        if let Some(n) = &per.nickname {
            parts.push(format!("nickname {n}"));
        }
        if let Some(v) = per.samp_version {
            parts.push(format!("SA-MP {}", v.label()));
        }
        if per.password.is_some() {
            parts.push("password saved".into());
        }
        lines.push(kv("overrides", parts.join(", ")));
    }
    for (label, url, key) in [("website", s.website(), "w"), ("discord", s.discord(), "D")] {
        if let Some(url) = url {
            let mut line = kv(label, url);
            line.push_span(Span::styled(format!("  ({key})"), theme::key()));
            lines.push(line);
        }
    }
    if let Some(e) = &s.extra {
        if !e.light_banner.is_empty() {
            lines.push(kv("banner", e.light_banner.clone()));
        }
        if !e.logo.is_empty() {
            lines.push(kv("logo", e.logo.clone()));
        }
    }
    if !s.rules.is_empty() {
        let rules: Vec<String> = s
            .rules
            .iter()
            .filter(|(k, _)| !["version", "weburl"].contains(&k.as_str()))
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        lines.push(Line::from(vec![
            Span::styled(format!("{:<10}", "rules"), theme::dim()),
            Span::styled(rules.join("  "), theme::text()),
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_players(f: &mut Frame, app: &App, area: Rect) {
    let (title, players) = match app.selected_server() {
        Some(s) => (format!(" Players ({}) ", s.player_list.len()), s.player_list.as_slice()),
        None => (" Players ".to_string(), &[][..]),
    };
    let block = Block::bordered().title(Line::from(Span::styled(title, theme::title()))).border_style(theme::border());
    let inner = block.inner(area);
    f.render_widget(block, area);
    if players.is_empty() {
        let text = match app.selected_server() {
            Some(s) if s.info.players > 0 => "player list not available\n(servers stop answering above ~100 players)",
            Some(_) => "nobody online",
            None => "",
        };
        f.render_widget(Paragraph::new(text).style(theme::dim()).wrap(Wrap { trim: true }), inner);
        return;
    }
    let rows: Vec<Row> = players
        .iter()
        .map(|p| {
            Row::new(vec![
                Cell::from(p.name.clone()),
                Cell::from(Line::from(p.score.to_string()).alignment(Alignment::Right)),
            ])
        })
        .collect();
    let header = Row::new(vec![Cell::from("name"), Cell::from(Line::from("score").alignment(Alignment::Right))])
        .style(theme::header());
    let table = Table::new(rows, [Constraint::Min(10), Constraint::Length(8)]).header(header).column_spacing(1);
    f.render_widget(table, inner);
}

fn draw_graph(f: &mut Frame, app: &App, area: Rect) {
    let history = app.selected.and_then(|a| app.ping_history.get(&a));
    let reachable: Vec<u64> = history
        .map(|h| h.iter().map(|&p| if p >= omptui_core::UNREACHABLE_PING { 0 } else { u64::from(p) }).collect())
        .unwrap_or_default();
    let title = match history.and_then(|h| h.back().copied()) {
        Some(last) => {
            let valid: Vec<u64> = reachable.iter().copied().filter(|&p| p > 0).collect();
            if valid.is_empty() {
                " Ping: unreachable ".to_string()
            } else {
                let min = valid.iter().min().copied().unwrap_or(0);
                let max = valid.iter().max().copied().unwrap_or(0);
                let avg = valid.iter().sum::<u64>() / valid.len() as u64;
                format!(" Ping {}  min {min} avg {avg} max {max} ", ping_text(Some(last)))
            }
        }
        None => " Ping ".to_string(),
    };
    let block = Block::bordered().title(Line::from(Span::styled(title, theme::title()))).border_style(theme::border());
    let inner = block.inner(area);
    f.render_widget(block, area);
    if reachable.is_empty() {
        return;
    }
    let width = inner.width as usize;
    let start = reachable.len().saturating_sub(width);
    let window = &reachable[start..];
    let floor = window.iter().copied().filter(|&p| p > 0).min().unwrap_or(0);
    let bars: Vec<u64> = window.iter().map(|&p| if p == 0 { 0 } else { p - floor + 1 }).collect();
    let max = bars.iter().max().copied().unwrap_or(1).max(1);
    let sparkline = Sparkline::default().data(&bars).max(max).style(Style::default().fg(theme::ACCENT));
    f.render_widget(sparkline, inner);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let line = match &app.status {
        Some(st) => {
            Line::from(Span::styled(format!(" {}", st.text), if st.error { theme::err() } else { theme::ok() }))
        }
        None => {
            let keys: &[(&str, &str)] = if app.popup.is_some() {
                &[("Esc", "close")]
            } else {
                &[
                    ("Enter", "join"),
                    ("F", "favorite"),
                    ("a", "add"),
                    ("/", "search"),
                    ("f", "filters"),
                    ("s", "sort"),
                    ("r", "refresh"),
                    ("c", "copy"),
                    ("p", "server opts"),
                    (",", "settings"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            };
            let mut spans = Vec::new();
            for (k, v) in keys {
                spans.push(Span::styled(format!(" {k}"), theme::key()));
                spans.push(Span::styled(format!(" {v} "), theme::dim()));
            }
            Line::from(spans)
        }
    };
    f.render_widget(Paragraph::new(line).style(Style::default().add_modifier(Modifier::empty())), area);
}
