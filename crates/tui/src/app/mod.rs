mod events;
mod join;
mod keys;
mod mouse;
pub mod popup;
mod settings;
pub mod tasks;

use crate::input::Input;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use omptui_core::deeplink::DeepLink;
use omptui_core::filter::{Filters, SortKey, normalize_language, version_family};
use omptui_core::launch::{HelperEvent, LaunchRequest};
use omptui_core::query::{BasicResult, FullResult};
use omptui_core::resources::{ClientFiles, FileState, SampVersion, inspect_game_exe};
use omptui_core::store::{Lists, Paths, ServerSettings, Settings};
use omptui_core::validation::validate_nickname;
use omptui_core::wine::{Prefix, WineEnv, discover_wine};
use omptui_core::{ListKind, Server, ServerAddr, UNREACHABLE_PING};
use popup::*;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tasks::{AppEvent, Services};

pub const PING_HISTORY: usize = 120;
const FULL_EVERY_TICKS: u32 = 3;
const STATUS_SECS: u64 = 3;
const STATUS_ERROR_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

// Where the last frame put the clickable parts.
#[derive(Debug, Default)]
pub struct HitAreas {
    pub tabs: Vec<(Rect, ListKind)>,
    pub search: Rect,
    pub update: Rect,
    pub list: Rect,
    // table rows without the header line
    pub rows: Rect,
    // urls in the details pane
    pub links: Vec<(Rect, String)>,
    pub popup: PopupHits,
}

// row -> field / cursor index
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PopupHits {
    pub area: Rect,
    pub rows: Vec<(Rect, usize)>,
}

pub struct App {
    pub svc: Services,
    pub paths: Paths,
    pub files: ClientFiles,
    pub settings: Settings,
    pub lists: Lists,
    pub tab: ListKind,
    pub internet: Vec<Server>,
    pub filters: Filters,
    pub search: Input,
    pub search_editing: bool,
    // indices into the current list, filtered + sorted
    pub view: Vec<usize>,
    pub table: TableState,
    pub selected: Option<ServerAddr>,
    pub ping_history: HashMap<ServerAddr, VecDeque<u32>>,
    pub popup: Option<Popup>,
    pub status: Option<StatusLine>,
    pub loading: bool,
    pub api_error: Option<String>,
    pub should_quit: bool,
    pub ticks: u32,
    pub game_running: bool,
    pub last_launch: Option<LaunchState>,
    pub pending_link: Option<DeepLink>,
    pub languages: BTreeMap<String, usize>,
    pub versions: BTreeMap<String, usize>,
    pub update: Option<String>,
    pub hit: HitAreas,
    last_click: Option<(usize, Instant)>,
    queried_once: bool,
    after_message: Option<Popup>,
}

impl App {
    pub fn new(svc: Services, paths: Paths, settings: Settings, lists: Lists) -> Self {
        let files = ClientFiles::new(&paths.data_dir);
        let filters = settings.filters.clone();
        let search = Input::new(filters.query.clone());
        let mut app = Self {
            svc,
            paths,
            files,
            settings,
            lists,
            tab: ListKind::Internet,
            internet: Vec::new(),
            filters,
            search,
            search_editing: false,
            view: Vec::new(),
            table: TableState::default(),
            selected: None,
            ping_history: HashMap::new(),
            popup: None,
            status: None,
            loading: false,
            api_error: None,
            should_quit: false,
            ticks: 0,
            game_running: false,
            last_launch: None,
            pending_link: None,
            languages: BTreeMap::new(),
            versions: BTreeMap::new(),
            update: None,
            hit: HitAreas::default(),
            last_click: None,
            queried_once: false,
            after_message: None,
        };
        if !app.lists.favorites.is_empty() {
            app.tab = ListKind::Favorites;
        }
        app.rebuild_view();
        app
    }

    pub fn list(&self) -> &[Server] {
        match self.tab {
            ListKind::Favorites => &self.lists.favorites,
            ListKind::Internet | ListKind::Partners => &self.internet,
            ListKind::Recent => &self.lists.recent,
        }
    }

    pub fn list_len(&self, kind: ListKind) -> usize {
        match kind {
            ListKind::Favorites => self.lists.favorites.len(),
            ListKind::Internet => self.internet.len(),
            ListKind::Partners => self.internet.iter().filter(|s| s.info.partner).count(),
            ListKind::Recent => self.lists.recent.len(),
        }
    }

    pub fn selected_server(&self) -> Option<&Server> {
        let row = self.table.selected()?;
        let idx = *self.view.get(row)?;
        self.list().get(idx)
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        self.table.selected().and_then(|r| self.view.get(r).copied())
    }

    pub fn rebuild_view(&mut self) {
        let partners_only = self.tab == ListKind::Partners;
        let list = self.list();
        let mut view = self.filters.apply(list);
        if partners_only {
            view.retain(|&i| list[i].info.partner);
        }
        self.view = view;
        let sel = self.selected.and_then(|a| self.view.iter().position(|&i| self.list()[i].addr == Some(a)));
        let row = match sel {
            Some(r) => Some(r),
            None if self.view.is_empty() => None,
            None => Some(self.table.selected().unwrap_or(0).min(self.view.len() - 1)),
        };
        self.table.select(row);
        self.sync_selected();
    }

    pub(super) fn sync_selected(&mut self) {
        let new = self.selected_server().and_then(|s| s.addr);
        if new != self.selected {
            self.selected = new;
            if let Some(addr) = new {
                let omp = self.selected_server().map(|s| s.info.omp).unwrap_or(false);
                self.svc.query_full(addr, omp);
            }
        }
    }

    pub(super) fn select_row(&mut self, row: Option<usize>) {
        self.table.select(row);
        self.sync_selected();
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.view.is_empty() {
            self.select_row(None);
            return;
        }
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, self.view.len() as isize - 1) as usize;
        self.select_row(Some(next));
    }

    // A server can be in the internet list, favorites and recent at the same time.
    pub(super) fn for_each_copy(&mut self, addr: ServerAddr, mut f: impl FnMut(&mut Server)) {
        for s in self.internet.iter_mut().chain(self.lists.favorites.iter_mut()).chain(self.lists.recent.iter_mut()) {
            if s.addr == Some(addr) {
                f(s);
            }
        }
    }

    pub(super) fn any_copy(&self, addr: ServerAddr) -> Option<&Server> {
        self.internet
            .iter()
            .chain(self.lists.favorites.iter())
            .chain(self.lists.recent.iter())
            .find(|s| s.addr == Some(addr))
    }

    pub fn set_tab(&mut self, tab: ListKind) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.table.select(None);
        self.rebuild_view();
        if self.view.is_empty() {
            self.select_row(None);
        } else {
            self.select_row(Some(0));
        }
        if self.settings.query_lists && matches!(tab, ListKind::Favorites | ListKind::Recent) {
            let addrs: Vec<ServerAddr> = self.list().iter().filter_map(|s| s.addr).collect();
            self.svc.query_basic_many(addrs);
        }
    }

    pub fn status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(StatusLine { text: text.into(), error, at: Instant::now() });
    }

    pub fn message(&mut self, title: impl Into<String>, lines: Vec<String>, error: bool) {
        self.popup = Some(Popup::Message { title: title.into(), lines, error });
    }

    pub(super) fn open_link(&mut self, what: &str, url: Option<String>) {
        let Some(url) = url else {
            self.status(format!("no {what} for this server"), false);
            return;
        };
        match crate::desktop::open_url(&url) {
            Ok(()) => self.status(format!("opened {url}"), false),
            Err(e) => self.status(format!("could not open {url}: {e}"), true),
        }
    }

    pub fn save_all(&mut self) {
        self.settings.filters = self.filters.clone();
        if let Err(e) = self.settings.save(&self.paths) {
            self.status(format!("could not save settings: {e}"), true);
        }
        if let Err(e) = self.lists.save(&self.paths) {
            self.status(format!("could not save lists: {e}"), true);
        }
    }

    pub fn start(&mut self) {
        self.loading = true;
        self.svc.fetch_api();
        if self.settings.check_updates {
            self.svc.check_update(crate::update_url());
        }
        if self.settings.query_lists {
            let addrs: Vec<ServerAddr> =
                self.lists.favorites.iter().chain(self.lists.recent.iter()).filter_map(|s| s.addr).collect();
            self.svc.query_basic_many(addrs);
        }
        if let Some(link) = self.pending_link.clone() {
            self.status(format!("resolving {}…", link.host_port()), false);
            self.svc.resolve(link.host_port(), true);
        }
    }
}

pub fn ping_text(ping: Option<u32>) -> String {
    match ping {
        None => "…".into(),
        Some(p) if p >= UNREACHABLE_PING => "---".into(),
        Some(p) => format!("{p}ms"),
    }
}
