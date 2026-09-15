use super::*;

impl App {
    pub(super) fn open_join(&mut self) {
        let Some(s) = self.selected_server().cloned() else { return };
        self.open_join_for(s, None);
    }

    pub(super) fn pending_link_password_into_join(&mut self, server: Server) {
        let pw = self.pending_link.take().and_then(|l| l.password);
        self.open_join_for(server, pw);
    }

    pub(super) fn open_join_for(&mut self, server: Server, password: Option<String>) {
        let Some(addr) = server.addr else { return };
        let per = self.lists.settings_for(addr);
        let nickname = per.nickname.clone().unwrap_or_else(|| self.settings.nickname.clone());
        let pw = password.or(per.password.clone()).unwrap_or_default();
        let remember = per.password.is_some();
        self.popup = Some(Popup::Join(Box::new(JoinForm {
            samp_version: per.samp_version.unwrap_or(self.settings.samp_version),
            server,
            nickname: Input::new(nickname),
            password: Input::new(pw).masked(),
            remember_password: remember,
            field: 0,
            error: None,
        })));
    }

    pub(super) fn open_server_settings(&mut self) {
        let Some(s) = self.selected_server().cloned() else { return };
        let Some(addr) = s.addr else { return };
        let per = self.lists.settings_for(addr);
        self.popup = Some(Popup::ServerSettings(ServerSettingsForm {
            addr,
            name: s.display_name(),
            nickname: Input::new(per.nickname.unwrap_or_default()),
            password: Input::new(per.password.unwrap_or_default()).masked(),
            samp_version: per.samp_version,
            field: 0,
            error: None,
        }));
    }

    pub fn wine_env(&self) -> WineEnv {
        let wine = self
            .settings
            .wine_binary
            .clone()
            .or_else(|| discover_wine().into_iter().next().map(|w| w.path))
            .unwrap_or_else(|| PathBuf::from("wine"));
        let prefix = self.settings.wine_prefix.clone().unwrap_or_else(|| self.paths.data_dir.join("prefix"));
        WineEnv { wine, prefix, extra_env: self.settings.env.clone() }
    }

    pub(super) fn handle_join_key(&mut self, mut form: JoinForm, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % JoinForm::FIELDS,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + JoinForm::FIELDS - 1) % JoinForm::FIELDS,
            KeyCode::Enter => match form.field {
                2 => form.remember_password = !form.remember_password,
                3 => form.samp_version = form.samp_version.next(),
                _ => match validate_nickname(form.nickname.value()) {
                    Ok(nick) => {
                        self.launch(
                            form.server.clone(),
                            nick,
                            form.password.value().to_owned(),
                            form.remember_password,
                            form.samp_version,
                        );
                        return;
                    }
                    Err(e) => {
                        form.error = Some(e.to_string());
                        form.field = 0;
                    }
                },
            },
            KeyCode::Char(' ') if form.field == 2 => form.remember_password = !form.remember_password,
            KeyCode::Left if form.field == 3 => form.samp_version = form.samp_version.prev(),
            KeyCode::Right | KeyCode::Char(' ') if form.field == 3 => form.samp_version = form.samp_version.next(),
            KeyCode::Char('v') if form.field > 1 => form.samp_version = form.samp_version.next(),
            _ => match form.field {
                0 => {
                    form.nickname.handle(key);
                    form.error = None;
                }
                1 => {
                    form.password.handle(key);
                }
                _ => {}
            },
        }
        self.popup = Some(Popup::Join(Box::new(form)));
    }

    pub(super) fn launch(
        &mut self,
        server: Server,
        nickname: String,
        password: String,
        remember_password: bool,
        samp_version: SampVersion,
    ) {
        let Some(addr) = server.addr else { return };
        let Some(game_dir) = self.settings.game_dir.clone() else {
            self.message(
                "Game folder not set",
                vec!["Open Settings (,) and set the game folder, or run auto-detect.".into()],
                true,
            );
            return;
        };
        self.settings.use_nickname(&nickname);
        let mut per = self.lists.settings_for(addr);
        per.password = if remember_password && !password.is_empty() { Some(password.clone()) } else { None };
        per.samp_version = if samp_version == self.settings.samp_version { None } else { Some(samp_version) };
        self.lists.set_settings_for(addr, per);
        self.lists.push_recent(&server);
        self.save_all();
        let req = LaunchRequest {
            addr,
            nickname,
            password: if password.is_empty() { None } else { Some(password) },
            game_dir,
            game_exe: self.settings.game_exe.clone(),
            samp_version,
            omp_inject: self.settings.omp_inject,
            wine: self.wine_env(),
            create_suspended: self.settings.create_suspended,
            wait_for_module: self.settings.wait_for_module.clone(),
        };
        let mut st = LaunchState { server: format!("{} ({addr})", server.display_name()), ..Default::default() };
        st.push(
            format!(
                "joining as {} with SA-MP {}{}",
                req.nickname,
                samp_version.label(),
                if req.omp_inject { " + open.mp" } else { "" }
            ),
            false,
        );
        self.popup = Some(Popup::Launch(st));
        self.svc.launch(req, self.files.clone());
        if self.tab == ListKind::Recent {
            self.rebuild_view();
        }
    }

    pub(super) fn handle_server_settings_key(&mut self, mut form: ServerSettingsForm, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 3,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 2) % 3,
            KeyCode::Enter => {
                self.commit_server_settings(form);
                return;
            }
            KeyCode::Left | KeyCode::Right if form.field == 2 => {
                form.samp_version = cycle_opt_version(form.samp_version, key.code == KeyCode::Left);
            }
            _ => match form.field {
                0 => {
                    form.nickname.handle(key);
                }
                1 => {
                    form.password.handle(key);
                }
                _ => {}
            },
        }
        self.popup = Some(Popup::ServerSettings(form));
    }

    pub(super) fn commit_server_settings(&mut self, mut form: ServerSettingsForm) {
        let nick = form.nickname.value().trim().to_owned();
        if !nick.is_empty()
            && let Err(e) = validate_nickname(&nick)
        {
            form.error = Some(e.to_string());
            form.field = 0;
            self.popup = Some(Popup::ServerSettings(form));
            return;
        }
        let pw = form.password.value().trim();
        let s = ServerSettings {
            nickname: if nick.is_empty() { None } else { Some(nick) },
            samp_version: form.samp_version,
            password: if pw.is_empty() { None } else { Some(pw.to_owned()) },
        };
        tracing::info!("server settings for {}: {s:?}", form.addr);
        self.lists.set_settings_for(form.addr, s);
        self.save_all();
        self.status(format!("saved settings for {}", form.addr), false);
    }
}

fn cycle_opt_version(cur: Option<SampVersion>, back: bool) -> Option<SampVersion> {
    match (cur, back) {
        (None, false) => Some(SampVersion::ALL[0]),
        (None, true) => Some(*SampVersion::ALL.last().unwrap()),
        (Some(v), false) => {
            if v == *SampVersion::ALL.last().unwrap() {
                None
            } else {
                Some(v.next())
            }
        }
        (Some(v), true) => {
            if v == SampVersion::ALL[0] {
                None
            } else {
                Some(v.prev())
            }
        }
    }
}
