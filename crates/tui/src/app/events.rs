use super::*;
use omptui_core::launch::Prepared;

impl App {
    pub fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::ApiLoaded(Ok(servers)) => self.on_api_loaded(servers),
            AppEvent::ApiLoaded(Err(e)) => {
                self.loading = false;
                self.api_error = Some(e.clone());
                self.status(format!("master list unavailable: {e}"), true);
            }
            AppEvent::Basic { addr, result } => self.apply_basic(addr, &result),
            AppEvent::Full { addr, result } => self.apply_full(addr, &result),
            AppEvent::Ping { addr, ms } => {
                self.for_each_copy(addr, |s| s.ping = Some(ms));
                self.push_ping(addr, ms);
            }
            AppEvent::Resolved { result, join } => self.on_resolved(result, join),
            AppEvent::ImportedFavorites(r) => self.on_imported_favorites(r),
            AppEvent::LaunchPrepared(r) => self.on_launch_prepared(r),
            AppEvent::Launch(ev) => self.on_launch_event(ev),
            AppEvent::LaunchFinished(r) => self.on_launch_finished(r),
            AppEvent::TaskDone { title, result } => match result {
                Ok(lines) => self.message(title, lines, false),
                Err(e) => self.message(title, e.lines().map(str::to_owned).collect(), true),
            },
            AppEvent::Tick => self.on_tick(),
        }
    }

    fn on_api_loaded(&mut self, servers: Vec<Server>) {
        self.loading = false;
        self.api_error = None;
        let old: HashMap<ServerAddr, Option<u32>> =
            self.internet.iter().filter_map(|s| s.addr.map(|a| (a, s.ping))).collect();
        self.internet = servers;
        for s in &mut self.internet {
            if let Some(p) = s.addr.and_then(|a| old.get(&a).copied().flatten()) {
                s.ping = Some(p);
            }
        }
        self.languages.clear();
        self.versions.clear();
        for s in &self.internet {
            *self.languages.entry(normalize_language(&s.info.language)).or_default() += 1;
            *self.versions.entry(version_family(&s.info.version)).or_default() += 1;
        }
        self.rebuild_view();
        if matches!(self.tab, ListKind::Internet | ListKind::Partners)
            && self.table.selected().is_none()
            && !self.view.is_empty()
        {
            self.select_row(Some(0));
        }
        self.status(format!("{} servers loaded", self.internet.len()), false);
        if self.settings.query_lists && !self.queried_once {
            self.queried_once = true;
            let addrs: Vec<ServerAddr> = self.internet.iter().filter_map(|s| s.addr).collect();
            self.svc.query_basic_many(addrs);
        }
    }

    fn on_resolved(&mut self, result: Result<(ServerAddr, String), String>, join: bool) {
        match result {
            Ok((addr, label)) => {
                let mut s = Server::with_addr(addr);
                s.host_label = Some(label.clone());
                if let Some(existing) = self.any_copy(addr) {
                    s = existing.clone();
                } else {
                    s.info.hostname = label;
                }
                if join {
                    self.pending_link_password_into_join(s);
                } else {
                    if !self.lists.is_favorite(addr) {
                        self.lists.toggle_favorite(&s);
                        self.save_all();
                    }
                    self.set_tab(ListKind::Favorites);
                    self.selected = None;
                    self.rebuild_view();
                    let row = self.view.iter().position(|&i| self.list()[i].addr == Some(addr));
                    self.select_row(row);
                    self.svc.query_basic_many(vec![addr]);
                    self.status(format!("added {addr} to favorites"), false);
                }
            }
            Err(e) => {
                self.pending_link = None;
                self.message("Could not add server", vec![e], true);
            }
        }
    }

    fn on_imported_favorites(&mut self, r: Result<Vec<Server>, String>) {
        match r {
            Ok(servers) => {
                let mut added = 0;
                for s in &servers {
                    if s.addr.is_some_and(|a| !self.lists.is_favorite(a)) {
                        self.lists.toggle_favorite(s);
                        added += 1;
                    }
                }
                self.save_all();
                self.set_tab(ListKind::Favorites);
                self.rebuild_view();
                let addrs: Vec<ServerAddr> = servers.iter().filter_map(|s| s.addr).collect();
                self.svc.query_basic_many(addrs);
                self.message(
                    "Import favorites",
                    vec![format!("{added} new favorites imported ({} in file)", servers.len())],
                    false,
                );
            }
            Err(e) => self.message("Import favorites", vec![e], true),
        }
    }

    fn on_launch_prepared(&mut self, r: Result<Box<Prepared>, String>) {
        match r {
            Ok(prepared) => {
                let st = self.launch_state();
                st.push(format!("running: {}", prepared.preview()), false);
                for w in &prepared.warnings {
                    st.push(format!("warning: {w}"), true);
                }
                for c in &prepared.copied {
                    st.push(format!("copied {c} into the game folder"), false);
                }
                self.mirror_launch_state();
            }
            Err(e) => {
                let st = self.launch_state();
                st.push(format!("cannot launch: {e}"), true);
                st.finished = true;
                st.failed = true;
                self.mirror_launch_state();
                self.status(format!("cannot launch: {e}"), true);
            }
        }
    }

    fn on_launch_event(&mut self, ev: HelperEvent) {
        let error = matches!(ev, HelperEvent::Error { .. } | HelperEvent::Retry { .. });
        if matches!(ev, HelperEvent::Spawned { .. }) {
            self.game_running = true;
        }
        if matches!(ev, HelperEvent::Exit { .. }) {
            self.game_running = false;
        }
        let line = ev.describe();
        let st = self.launch_state();
        st.push(line.clone(), error);
        if matches!(ev, HelperEvent::Resumed | HelperEvent::Injected { .. }) {
            st.game_started = true;
        }
        if matches!(ev, HelperEvent::Error { .. }) {
            st.failed = true;
        }
        self.mirror_launch_state();
        if matches!(ev, HelperEvent::Resumed) && self.settings.quit_after_launch {
            self.should_quit = true;
        }
        if !matches!(ev, HelperEvent::Log(_)) {
            self.status(line, error);
        }
    }

    fn on_launch_finished(&mut self, r: Result<Option<i32>, String>) {
        self.game_running = false;
        let line = match &r {
            Ok(Some(0)) => "helper finished".to_string(),
            Ok(Some(c)) => format!("helper exited with code {c}"),
            Ok(None) => "helper terminated".to_string(),
            Err(e) => format!("launch failed: {e}"),
        };
        let failed = !matches!(r, Ok(Some(0)));
        let st = self.launch_state();
        st.push(line.clone(), failed);
        st.finished = true;
        st.failed |= failed;
        self.mirror_launch_state();
        self.status(line, failed);
    }

    pub(super) fn apply_basic(&mut self, addr: ServerAddr, b: &BasicResult) {
        self.for_each_copy(addr, |s| s.apply_basic(b));
        if let Some(p) = b.ping {
            self.push_ping(addr, p);
        }
        if self.filters_depend_on_queries() {
            self.rebuild_view();
        }
    }

    pub(super) fn apply_full(&mut self, addr: ServerAddr, f: &FullResult) {
        self.for_each_copy(addr, |s| s.apply_full(f));
        if let Some(p) = f.basic.ping {
            self.push_ping(addr, p);
        }
        if self.filters_depend_on_queries() {
            self.rebuild_view();
        }
    }

    // player counts and passwords come from the queries, so these filters change as answers arrive
    fn filters_depend_on_queries(&self) -> bool {
        self.filters.sort != SortKey::None || self.filters.non_empty || self.filters.unpassworded
    }

    pub(super) fn launch_state(&mut self) -> &mut LaunchState {
        if let Some(Popup::Launch(st)) = &mut self.popup {
            return st;
        }
        self.last_launch.get_or_insert_with(LaunchState::default)
    }

    pub(super) fn mirror_launch_state(&mut self) {
        if let Some(Popup::Launch(st)) = &self.popup {
            self.last_launch = Some(st.clone());
        }
    }

    pub(super) fn push_ping(&mut self, addr: ServerAddr, ms: u32) {
        let h = self.ping_history.entry(addr).or_default();
        h.push_back(ms);
        while h.len() > PING_HISTORY {
            h.pop_front();
        }
    }

    pub(super) fn on_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        if let Some(st) = &self.status {
            let keep = if st.error { STATUS_ERROR_SECS } else { STATUS_SECS };
            if st.at.elapsed().as_secs() >= keep {
                self.status = None;
            }
        }
        if let Some(addr) = self.selected {
            let paused = matches!(self.popup, Some(Popup::Settings(_)) | Some(Popup::Help));
            if !paused {
                if self.ticks.is_multiple_of(FULL_EVERY_TICKS) {
                    let omp = self.selected_server().map(|s| s.info.omp && s.extra.is_none()).unwrap_or(false);
                    self.svc.query_full(addr, omp);
                } else {
                    self.svc.ping(addr);
                }
            }
        }
    }
}
