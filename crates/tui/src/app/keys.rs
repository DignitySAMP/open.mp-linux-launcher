use super::*;

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return;
        }
        if self.popup.is_some() {
            self.handle_popup_key(key);
            return;
        }
        if self.search_editing {
            match key.code {
                KeyCode::Esc => {
                    self.search_editing = false;
                    self.search.set("");
                    self.filters.query.clear();
                    self.rebuild_view();
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Tab => self.search_editing = false,
                _ => {
                    if self.search.handle(key) {
                        self.filters.query = self.search.value().to_owned();
                        self.rebuild_view();
                    }
                }
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Char('1') => self.set_tab(ListKind::Favorites),
            KeyCode::Char('2') => self.set_tab(ListKind::Internet),
            KeyCode::Char('3') => self.set_tab(ListKind::Partners),
            KeyCode::Char('4') => self.set_tab(ListKind::Recent),
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::PageDown =>
            {
                self.move_selection(15)
            }
            KeyCode::PageUp | KeyCode::Char('u')
                if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::PageUp =>
            {
                self.move_selection(-15)
            }
            KeyCode::Home | KeyCode::Char('g') => self.select_row(if self.view.is_empty() { None } else { Some(0) }),
            KeyCode::End | KeyCode::Char('G') => self.select_row(self.view.len().checked_sub(1)),
            KeyCode::Enter => self.open_join(),
            KeyCode::Char('/') => self.search_editing = true,
            KeyCode::Char('f') => self.open_filters(),
            KeyCode::Char('o') => {
                self.filters.omp_only = !self.filters.omp_only;
                self.rebuild_view();
            }
            KeyCode::Char('e') => {
                self.filters.non_empty = !self.filters.non_empty;
                self.rebuild_view();
            }
            KeyCode::Char('n') => {
                self.filters.unpassworded = !self.filters.unpassworded;
                self.rebuild_view();
            }
            KeyCode::Char('s') => {
                self.filters.sort = self.filters.sort.next();
                self.rebuild_view();
            }
            KeyCode::Char('S') => {
                self.filters.dir = self.filters.dir.toggle();
                self.rebuild_view();
            }
            KeyCode::Char('F') | KeyCode::Char(' ') => self.toggle_favorite(),
            KeyCode::Char('a') => self.popup = Some(Popup::AddServer(Input::new(""))),
            KeyCode::Char('d') | KeyCode::Delete => self.remove_selected(),
            KeyCode::Char('J') => self.move_favorite(1),
            KeyCode::Char('K') => self.move_favorite(-1),
            KeyCode::Char('r') => {
                self.loading = true;
                self.status("refreshing master list…", false);
                self.svc.fetch_api();
            }
            KeyCode::Char('R') => {
                if let Some(addr) = self.selected {
                    let omp = self.selected_server().map(|s| s.info.omp).unwrap_or(false);
                    self.svc.query_full(addr, omp);
                    self.status(format!("querying {addr}"), false);
                }
                if self.settings.query_lists {
                    let addrs: Vec<ServerAddr> = self.view.iter().filter_map(|&i| self.list()[i].addr).collect();
                    self.svc.query_basic_many(addrs);
                }
            }
            KeyCode::Char('c') => {
                if let Some(s) = self.selected_server() {
                    let text = s.address_text();
                    let how = crate::clipboard::copy(&text);
                    self.status(format!("copied {text} ({how})"), false);
                }
            }
            KeyCode::Char('p') => self.open_server_settings(),
            KeyCode::Char(',') => self.open_settings(),
            KeyCode::Char('x') => {
                if self.tab == ListKind::Recent && !self.lists.recent.is_empty() {
                    self.popup = Some(Popup::Confirm {
                        title: "Clear recently joined".into(),
                        text: format!("Remove all {} entries?", self.lists.recent.len()),
                        action: ConfirmAction::ClearRecent,
                    });
                }
            }
            KeyCode::Char('l') => {
                if let Some(st) = self.last_launch.clone() {
                    self.popup = Some(Popup::Launch(st));
                }
            }
            KeyCode::Char('i') => {
                self.popup = Some(Popup::PathPrompt(PathPrompt {
                    title: "Import client files".into(),
                    hint:
                        "Folder of an official launcher install: .../AppData/Local/mp.open.launcher (or a copy of it)"
                            .into(),
                    input: Input::new(self.default_import_dir()),
                    action: PathAction::ImportClientFiles,
                }))
            }
            _ => {}
        }
    }

    pub(super) fn cycle_tab(&mut self, delta: isize) {
        let i = ListKind::ALL.iter().position(|k| *k == self.tab).unwrap_or(0) as isize;
        let n = ListKind::ALL.len() as isize;
        self.set_tab(ListKind::ALL[((i + delta) % n + n) as usize % n as usize]);
    }

    pub(super) fn toggle_favorite(&mut self) {
        let Some(s) = self.selected_server().cloned() else { return };
        let now = self.lists.toggle_favorite(&s);
        tracing::info!("favorite {} {}", if now { "added" } else { "removed" }, s.address_text());
        self.save_all();
        self.status(
            if now {
                format!("★ {} added to favorites", s.display_name())
            } else {
                format!("{} removed from favorites", s.display_name())
            },
            false,
        );
        if self.tab == ListKind::Favorites {
            self.rebuild_view();
        }
    }

    pub(super) fn remove_selected(&mut self) {
        let Some(s) = self.selected_server().cloned() else { return };
        let Some(addr) = s.addr else { return };
        match self.tab {
            ListKind::Favorites => {
                self.popup = Some(Popup::Confirm {
                    title: "Remove favorite".into(),
                    text: format!("Remove {} ({addr}) from favorites?", s.display_name()),
                    action: ConfirmAction::RemoveFavorite(addr),
                });
            }
            ListKind::Recent => {
                self.lists.recent.retain(|r| r.addr != Some(addr));
                self.save_all();
                self.rebuild_view();
                self.status(format!("removed {addr} from recent"), false);
            }
            _ => self.toggle_favorite(),
        }
    }

    pub(super) fn move_favorite(&mut self, delta: isize) {
        if self.tab != ListKind::Favorites || self.filters.sort != SortKey::None {
            return;
        }
        let Some(idx) = self.selected_index() else { return };
        if let Some(new_idx) = self.lists.move_favorite(idx, delta) {
            self.save_all();
            self.rebuild_view();
            if let Some(row) = self.view.iter().position(|&i| i == new_idx) {
                self.select_row(Some(row));
            }
        }
    }

    pub(super) fn default_import_dir(&self) -> String {
        self.settings
            .wine_prefix
            .as_ref()
            .and_then(|p| crate::setup::find_launcher_data_in_prefix(p))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub(super) fn open_filters(&mut self) {
        let mut langs: Vec<(String, usize)> = self.languages.iter().map(|(k, v)| (k.clone(), *v)).collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        langs.truncate(40);
        for l in &self.filters.languages {
            if !langs.iter().any(|(n, _)| n == l) {
                langs.push((l.clone(), 0));
            }
        }
        self.popup = Some(Popup::Filters(FilterForm { cursor: 0, languages: langs }));
    }

    pub(super) fn handle_popup_key(&mut self, key: KeyEvent) {
        let Some(popup) = self.popup.take() else { return };
        match popup {
            Popup::Help => {
                if !matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter) {
                    self.popup = Some(Popup::Help);
                }
            }
            Popup::Message { title, lines, error } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                    self.popup = self.after_message.take();
                } else {
                    self.popup = Some(Popup::Message { title, lines, error });
                }
            }
            Popup::Confirm { title, text, action } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.run_confirm(action),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {}
                _ => self.popup = Some(Popup::Confirm { title, text, action }),
            },
            Popup::Launch(mut st) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                    self.last_launch = Some(st);
                }
                _ => {
                    if key.code == KeyCode::Char('c') {
                        st.lines.clear();
                    }
                    self.popup = Some(Popup::Launch(st));
                }
            },
            Popup::AddServer(mut input) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let text = input.value().trim().to_owned();
                    if text.is_empty() {
                        self.popup = Some(Popup::AddServer(input));
                    } else {
                        self.status(format!("resolving {text}…"), false);
                        self.svc.resolve(text, false);
                    }
                }
                _ => {
                    input.handle(key);
                    self.popup = Some(Popup::AddServer(input));
                }
            },
            Popup::PathPrompt(mut p) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let path = expand_tilde(p.input.value().trim());
                    self.run_path_action(p.action, path);
                }
                _ => {
                    p.input.handle(key);
                    self.popup = Some(Popup::PathPrompt(p));
                }
            },
            Popup::Join(form) => self.handle_join_key(*form, key),
            Popup::Filters(form) => self.handle_filters_key(form, key),
            Popup::Settings(form) => self.handle_settings_key(form, key),
            Popup::ServerSettings(form) => self.handle_server_settings_key(form, key),
        }
    }

    pub(super) fn run_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::ClearRecent => {
                tracing::info!("recent list cleared");
                self.lists.recent.clear();
                self.save_all();
                self.rebuild_view();
                self.status("recently joined cleared", false);
            }
            ConfirmAction::RemoveFavorite(addr) => {
                tracing::info!("favorite removed {addr}");
                self.lists.favorites.retain(|s| s.addr != Some(addr));
                self.save_all();
                self.rebuild_view();
                self.status(format!("removed {addr} from favorites"), false);
            }
        }
    }

    pub(super) fn run_path_action(&mut self, action: PathAction, path: PathBuf) {
        match action {
            PathAction::ImportClientFiles => {
                let files = self.files.clone();
                self.svc.blocking_task("Import client files", move || {
                    if !path.is_dir() {
                        return Err(format!("{} is not a directory", path.display()));
                    }
                    let rep = files.import_from(&path).map_err(|e| e.to_string())?;
                    let mut lines =
                        vec![format!("copied {} files into {}", rep.copied.len(), files.data_dir.display())];
                    lines.extend(rep.copied.iter().map(|c| format!("  + {c}")));
                    if !rep.missing.is_empty() {
                        lines.push(format!("not found in source ({}):", rep.missing.len()));
                        lines.extend(rep.missing.iter().map(|m| format!("  - {m}")));
                    }
                    Ok(lines)
                });
            }
            PathAction::ImportUserdata => {
                let path = if path.is_dir() { path.join("USERDATA.DAT") } else { path };
                self.svc.import_userdata(path);
            }
        }
    }

    pub(super) fn handle_filters_key(&mut self, mut form: FilterForm, key: KeyEvent) {
        let rows = form.rows();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('f') => {
                self.rebuild_view();
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => form.cursor = (form.cursor + 1) % rows,
            KeyCode::Up | KeyCode::Char('k') => form.cursor = (form.cursor + rows - 1) % rows,
            KeyCode::Char('c') => {
                self.filters = Filters { query: self.filters.query.clone(), ..Default::default() };
                self.rebuild_view();
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                let back = key.code == KeyCode::Left;
                match form.cursor {
                    0 => self.filters.omp_only = !self.filters.omp_only,
                    1 => self.filters.non_empty = !self.filters.non_empty,
                    2 => self.filters.unpassworded = !self.filters.unpassworded,
                    3 => {
                        self.filters.sort = if back {
                            let all = SortKey::ALL;
                            let i = all.iter().position(|k| *k == self.filters.sort).unwrap_or(0);
                            all[(i + all.len() - 1) % all.len()]
                        } else {
                            self.filters.sort.next()
                        }
                    }
                    4 => self.filters.dir = self.filters.dir.toggle(),
                    n => {
                        if let Some((lang, _)) = form.languages.get(n - FilterForm::FIXED)
                            && !self.filters.languages.remove(lang)
                        {
                            self.filters.languages.insert(lang.clone());
                        }
                    }
                }
                self.rebuild_view();
            }
            _ => {}
        }
        self.popup = Some(Popup::Filters(form));
    }
}
