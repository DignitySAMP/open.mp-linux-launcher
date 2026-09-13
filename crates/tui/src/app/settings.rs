use super::*;

impl App {
    pub(super) fn open_settings(&mut self) {
        self.popup = Some(Popup::Settings(SettingsForm { cursor: 0, editing: None, message: None }));
    }

    pub(super) fn settings_text_value(&self, row: SettingsRow) -> String {
        let p = |o: &Option<PathBuf>| o.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        match row {
            SettingsRow::Nickname => self.settings.nickname.clone(),
            SettingsRow::GameDir => p(&self.settings.game_dir),
            SettingsRow::GameExe => self.settings.game_exe.clone(),
            SettingsRow::WineBinary => p(&self.settings.wine_binary),
            SettingsRow::WinePrefix => p(&self.settings.wine_prefix),
            SettingsRow::Env => self.settings.env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "),
            SettingsRow::Terminal => self.settings.terminal.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub fn settings_display(&self, row: SettingsRow) -> String {
        let onoff = |b: bool| if b { "on".to_string() } else { "off".to_string() };
        match row {
            SettingsRow::SampVersion => self.settings.samp_version.label().to_string(),
            SettingsRow::OmpInject => onoff(self.settings.omp_inject),
            SettingsRow::Suspended => onoff(self.settings.create_suspended),
            SettingsRow::QuitAfterLaunch => onoff(self.settings.quit_after_launch),
            SettingsRow::QueryLists => onoff(self.settings.query_lists),
            SettingsRow::WineBinary if self.settings.wine_binary.is_none() => discover_wine()
                .into_iter()
                .next()
                .map(|w| format!("(auto) {}", w.path.display()))
                .unwrap_or_else(|| "(auto) none found".into()),
            SettingsRow::WinePrefix if self.settings.wine_prefix.is_none() => {
                format!("(default) {}", self.paths.data_dir.join("prefix").display())
            }
            SettingsRow::Terminal if self.settings.terminal.is_none() => {
                format!("(auto) {}", crate::desktop::terminal_command(None).unwrap_or_else(|| "none found".into()))
            }
            r if r.is_text() => self.settings_text_value(r),
            _ => String::new(),
        }
    }

    pub(super) fn apply_settings_text(&mut self, row: SettingsRow, value: &str) -> Result<(), String> {
        let v = value.trim();
        tracing::info!("setting {} = {v:?}", row.label());
        let opt_path = |v: &str| if v.is_empty() { None } else { Some(expand_tilde(v)) };
        match row {
            SettingsRow::Nickname => {
                if !v.is_empty() {
                    validate_nickname(v).map_err(|e| e.to_string())?;
                }
                self.settings.nickname = v.to_owned();
            }
            SettingsRow::GameDir => {
                if let Some(p) = opt_path(v) {
                    if !p.is_dir() {
                        return Err(format!("{} is not a directory", p.display()));
                    }
                    if !p.join(&self.settings.game_exe).is_file() {
                        return Err(format!("{} not found in {}", self.settings.game_exe, p.display()));
                    }
                }
                self.settings.game_dir = opt_path(v);
            }
            SettingsRow::GameExe => {
                if v.is_empty() {
                    return Err("executable name cannot be empty".into());
                }
                self.settings.game_exe = v.to_owned();
            }
            SettingsRow::WineBinary => {
                if let Some(p) = opt_path(v)
                    && !p.is_file()
                {
                    return Err(format!("{} does not exist", p.display()));
                }
                self.settings.wine_binary = opt_path(v);
            }
            SettingsRow::WinePrefix => self.settings.wine_prefix = opt_path(v),
            SettingsRow::Env => {
                let pairs: Vec<String> = v.split_whitespace().map(str::to_owned).collect();
                if pairs.iter().any(|p| !p.contains('=')) {
                    return Err("use KEY=VALUE pairs separated by spaces".into());
                }
                self.settings.env = omptui_core::launch::parse_env_pairs(&pairs);
            }
            SettingsRow::Terminal => self.settings.terminal = if v.is_empty() { None } else { Some(v.to_owned()) },
            _ => {}
        }
        Ok(())
    }

    pub(super) fn handle_settings_key(&mut self, mut form: SettingsForm, key: KeyEvent) {
        let row = form.row();
        if let Some(mut input) = form.editing.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => match self.apply_settings_text(row, input.value()) {
                    Ok(()) => {
                        form.message = None;
                        self.save_all();
                    }
                    Err(e) => {
                        form.message = Some(e);
                        form.editing = Some(input);
                    }
                },
                _ => {
                    input.handle(key);
                    form.editing = Some(input);
                }
            }
            self.popup = Some(Popup::Settings(form));
            return;
        }
        let n = SettingsRow::ALL.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(',') => {
                self.save_all();
                return;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => form.cursor = (form.cursor + 1) % n,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => form.cursor = (form.cursor + n - 1) % n,
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                let back = key.code == KeyCode::Left;
                form.message = None;
                match row {
                    r if r.is_text() => {
                        if key.code == KeyCode::Enter {
                            form.editing = Some(Input::new(self.settings_text_value(r)));
                        } else if r == SettingsRow::WineBinary {
                            let found = discover_wine();
                            if !found.is_empty() {
                                let cur =
                                    found.iter().position(|w| Some(&w.path) == self.settings.wine_binary.as_ref());
                                let next = match (cur, back) {
                                    (None, _) => 0,
                                    (Some(i), false) => (i + 1) % found.len(),
                                    (Some(i), true) => (i + found.len() - 1) % found.len(),
                                };
                                self.settings.wine_binary = Some(found[next].path.clone());
                                self.save_all();
                            }
                        }
                    }
                    SettingsRow::SampVersion => {
                        self.settings.samp_version =
                            if back { self.settings.samp_version.prev() } else { self.settings.samp_version.next() };
                        self.save_all();
                    }
                    SettingsRow::OmpInject => {
                        self.settings.omp_inject = !self.settings.omp_inject;
                        self.save_all();
                    }
                    SettingsRow::Suspended => {
                        self.settings.create_suspended = !self.settings.create_suspended;
                        self.save_all();
                    }
                    SettingsRow::QuitAfterLaunch => {
                        self.settings.quit_after_launch = !self.settings.quit_after_launch;
                        self.save_all();
                    }
                    SettingsRow::QueryLists => {
                        self.settings.query_lists = !self.settings.query_lists;
                        self.save_all();
                    }
                    r if r.is_action() && key.code == KeyCode::Enter => {
                        self.popup = Some(Popup::Settings(form));
                        self.run_settings_action(r);
                        return;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.popup = Some(Popup::Settings(form));
    }

    pub(super) fn run_settings_action(&mut self, row: SettingsRow) {
        match row {
            SettingsRow::ActionCheckFiles => {
                let lines = self.check_report();
                self.after_message = self.popup.take();
                self.message("Check", lines, false);
            }
            SettingsRow::ActionDownloadClient => {
                self.status("downloading client files from open.mp…", false);
                self.svc.download_client_files(self.files.clone(), crate::assets_url());
            }
            SettingsRow::ActionImportClient => {
                self.popup = Some(Popup::PathPrompt(PathPrompt {
                    title: "Import client files".into(),
                    hint: "Folder of an official launcher install (.../AppData/Local/mp.open.launcher) or a copy of it"
                        .into(),
                    input: Input::new(self.default_import_dir()),
                    action: PathAction::ImportClientFiles,
                }));
            }
            SettingsRow::ActionDetect => {
                let report = crate::setup::auto_detect(&mut self.settings, &self.files);
                self.save_all();
                self.after_message = self.popup.take();
                self.message(
                    "Auto-detect",
                    if report.is_empty() { vec!["nothing to change".into()] } else { report },
                    false,
                );
            }
            SettingsRow::ActionInitPrefix => {
                let env = self.wine_env();
                self.status(format!("running wineboot in {}…", env.prefix.display()), false);
                self.svc.init_prefix(env);
            }
            SettingsRow::ActionInstallD3dx9 => {
                let env = self.wine_env();
                self.status("running winetricks d3dx9 (this takes a while)…", false);
                self.svc.install_d3dx9(env);
            }
            SettingsRow::ActionImportUserdata => {
                let guess = self
                    .settings
                    .wine_prefix
                    .as_ref()
                    .and_then(|p| omptui_core::import::userdata_candidates(p).into_iter().next())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.popup = Some(Popup::PathPrompt(PathPrompt {
                    title: "Import SA-MP favorites".into(),
                    hint: "Path to USERDATA.DAT (Documents/GTA San Andreas User Files/SAMP/USERDATA.DAT)".into(),
                    input: Input::new(guess),
                    action: PathAction::ImportUserdata,
                }));
            }
            SettingsRow::ActionInstallDesktop => {
                let terminal = self.settings.terminal.clone();
                self.svc.blocking_task("Desktop integration", move || {
                    let bin = crate::desktop::install_binary().map_err(|e| format!("copy binary: {e}"))?;
                    let mut lines = vec![format!("binary installed to {}", bin.display())];
                    lines.extend(
                        crate::desktop::install_desktop_entries(&bin, terminal.as_deref())
                            .map_err(|e| e.to_string())?,
                    );
                    Ok(lines)
                });
            }
            _ => {}
        }
    }

    pub fn check_report(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let env = self.wine_env();
        lines.push(format!("Wine: {} {}", env.wine.display(), if env.wine.is_file() { "✓" } else { "✗ not found" }));
        let pfx = Prefix::new(&env.prefix);
        lines.push(format!(
            "Prefix: {} {}",
            env.prefix.display(),
            if pfx.exists() {
                format!("✓ ({})", pfx.arch().unwrap_or_else(|| "?".into()))
            } else {
                "✗ not initialised".into()
            }
        ));
        if pfx.exists() {
            let d3dx = pfx.system_dirs().iter().any(|d| d.join("d3dx9_25.dll").is_file());
            lines.push(format!(
                "d3dx9_25.dll in prefix: {}",
                if d3dx { "✓" } else { "✗ missing (needed by the open.mp client)" }
            ));
        }
        match &self.settings.game_dir {
            None => lines.push("Game folder: ✗ not set".into()),
            Some(dir) => {
                let exe = dir.join(&self.settings.game_exe);
                match inspect_game_exe(&exe) {
                    Ok(i) if i.is_10_us => lines.push(format!("Game: {} ✓ (1.0 US)", exe.display())),
                    Ok(i) => {
                        lines.push(format!("Game: {} ⚠ not 1.0 US ({} bytes, md5 {})", exe.display(), i.size, i.md5))
                    }
                    Err(e) => lines.push(format!("Game: {} ✗ {e}", exe.display())),
                }
                for f in omptui_core::resources::SHARED_FILES {
                    if !dir.join(f.rel).is_file() {
                        lines.push(format!("  game folder is missing {} (copied automatically at launch)", f.rel));
                    }
                }
            }
        }
        lines.push(format!("Client files in {}:", self.files.data_dir.display()));
        for st in self.files.check(self.settings.samp_version, self.settings.omp_inject) {
            let mark = match &st.state {
                FileState::Ok => "✓".to_string(),
                FileState::Unverified => "✓ (unverified)".to_string(),
                FileState::Missing => "✗ missing".to_string(),
                FileState::Mismatch { actual } => format!("⚠ checksum {actual}"),
            };
            lines.push(format!("  {} {mark}", st.label));
        }
        let avail = self.files.available_versions();
        lines.push(format!("Available SA-MP versions: {}", avail.iter().map(|v| v.id()).collect::<Vec<_>>().join(" ")));
        lines.push(format!(
            "Helper embedded: {}",
            if self.svc.helper.is_empty() { "✗ (build without injector)" } else { "✓" }
        ));
        lines
    }
}
