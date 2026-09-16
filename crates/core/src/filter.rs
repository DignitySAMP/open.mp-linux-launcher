use crate::model::{Server, UNREACHABLE_PING};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortKey {
    #[default]
    None,
    Players,
    Ping,
    Name,
    Gamemode,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [SortKey::None, SortKey::Players, SortKey::Ping, SortKey::Name, SortKey::Gamemode];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::None => "none",
            SortKey::Players => "players",
            SortKey::Ping => "ping",
            SortKey::Name => "name",
            SortKey::Gamemode => "gamemode",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortDir {
    #[default]
    Desc,
    Asc,
}

impl SortDir {
    pub fn toggle(self) -> Self {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }
    pub fn arrow(self) -> &'static str {
        match self {
            SortDir::Asc => "▲",
            SortDir::Desc => "▼",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Filters {
    pub query: String,
    pub gamemode: String,
    pub omp_only: bool,
    pub non_empty: bool,
    pub unpassworded: bool,
    pub languages: BTreeSet<String>,
    pub versions: BTreeSet<String>,
    pub sort: SortKey,
    pub dir: SortDir,
}

impl Filters {
    pub fn active_count(&self) -> usize {
        usize::from(self.omp_only)
            + usize::from(self.non_empty)
            + usize::from(self.unpassworded)
            + usize::from(!self.gamemode.trim().is_empty())
            + usize::from(!self.languages.is_empty())
            + usize::from(!self.versions.is_empty())
    }

    pub fn matches(&self, s: &Server) -> bool {
        if self.omp_only && !s.info.omp {
            return false;
        }
        if self.non_empty && s.info.players == 0 {
            return false;
        }
        if self.unpassworded && s.info.password {
            return false;
        }
        if !self.languages.is_empty() && !self.languages.contains(&normalize_language(&s.info.language)) {
            return false;
        }
        if !self.versions.is_empty() && !self.versions.contains(&version_family(&s.info.version)) {
            return false;
        }
        let gm = self.gamemode.trim();
        if !gm.is_empty() && !s.info.gamemode.to_lowercase().contains(&gm.to_lowercase()) {
            return false;
        }
        let q = self.query.trim();
        if q.is_empty() {
            return true;
        }
        let q = q.to_lowercase();
        s.info.hostname.to_lowercase().contains(&q)
            || s.info.gamemode.to_lowercase().contains(&q)
            || s.address_text().contains(&q)
            || s.host_label.as_deref().is_some_and(|l| l.to_lowercase().contains(&q))
    }

    pub fn apply(&self, servers: &[Server]) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..servers.len()).filter(|&i| self.matches(&servers[i])).collect();
        if self.sort != SortKey::None {
            idx.sort_by(|&a, &b| {
                let o = compare(self.sort, &servers[a], &servers[b]);
                match self.dir {
                    SortDir::Asc => o,
                    SortDir::Desc => o.reverse(),
                }
            });
        }
        idx
    }
}

// Languages are free text like "English/Russian" or "RO/EN", so group them by the first token.
pub fn normalize_language(raw: &str) -> String {
    let first = raw.split(['/', ',', '|', '&']).next().unwrap_or("").trim();
    if first.is_empty() {
        return "Unknown".into();
    }
    let mut c = first.chars();
    let head = c.next().map(|ch| ch.to_uppercase().collect::<String>()).unwrap_or_default();
    head + &c.as_str().to_lowercase()
}

// "omp 1.5.8.3079", "0.3.7-R2" and "0.3.DL-R1" become open.mp, 0.3.7 and 0.3.DL.
pub fn version_family(raw: &str) -> String {
    match raw.split(['-', ' ']).next().unwrap_or("").trim() {
        "" | "unknown" => "Unknown".into(),
        "omp" => "open.mp".into(),
        other => other.to_owned(),
    }
}

fn compare(key: SortKey, a: &Server, b: &Server) -> Ordering {
    match key {
        SortKey::None => Ordering::Equal,
        SortKey::Players => a.info.players.cmp(&b.info.players).then_with(|| b.info.hostname.cmp(&a.info.hostname)),
        SortKey::Ping => {
            let pa = a.ping.unwrap_or(UNREACHABLE_PING);
            let pb = b.ping.unwrap_or(UNREACHABLE_PING);
            // Reversed on purpose: the default direction is descending and the lowest ping should come first.
            pb.cmp(&pa)
        }
        SortKey::Name => b.info.hostname.to_lowercase().cmp(&a.info.hostname.to_lowercase()),
        SortKey::Gamemode => b.info.gamemode.to_lowercase().cmp(&a.info.gamemode.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerAddr;

    fn srv(name: &str, players: u16, ping: Option<u32>, omp: bool, pw: bool, lang: &str) -> Server {
        let mut s = Server::with_addr(ServerAddr::new([1, 2, 3, 4].into(), 7777));
        s.info.hostname = name.into();
        s.info.gamemode = format!("{name} mode");
        s.info.players = players;
        s.ping = ping;
        s.info.omp = omp;
        s.info.password = pw;
        s.info.language = lang.into();
        s.info.version = if omp { "omp 1.5.8.3079".into() } else { "0.3.7-R2".into() };
        s
    }

    fn list() -> Vec<Server> {
        vec![
            srv("Bravo", 10, Some(50), true, false, "English"),
            srv("alpha", 0, None, false, true, "Russian"),
            srv("Charlie", 5, Some(20), true, true, "english/russian"),
        ]
    }

    #[test]
    fn text_search_is_case_insensitive_and_matches_gamemode_or_address() {
        let l = list();
        let f = Filters { query: "ALPHA".into(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![1]);
        let f = Filters { query: "charlie mode".into(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![2]);
        let f = Filters { query: "1.2.3.4".into(), ..Default::default() };
        assert_eq!(f.apply(&l).len(), 3);
    }

    #[test]
    fn toggles() {
        let l = list();
        assert_eq!(Filters { omp_only: true, ..Default::default() }.apply(&l), vec![0, 2]);
        assert_eq!(Filters { non_empty: true, ..Default::default() }.apply(&l), vec![0, 2]);
        assert_eq!(Filters { unpassworded: true, ..Default::default() }.apply(&l), vec![0]);
        let f = Filters { languages: ["English".to_string()].into_iter().collect(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![0, 2]);
    }

    #[test]
    fn sorting() {
        let l = list();
        let f = Filters { sort: SortKey::Players, dir: SortDir::Desc, ..Default::default() };
        assert_eq!(f.apply(&l), vec![0, 2, 1]);
        let f = Filters { sort: SortKey::Players, dir: SortDir::Asc, ..Default::default() };
        assert_eq!(f.apply(&l), vec![1, 2, 0]);
        let f = Filters { sort: SortKey::Ping, dir: SortDir::Desc, ..Default::default() };
        assert_eq!(f.apply(&l), vec![2, 0, 1]);
        let f = Filters { sort: SortKey::Name, dir: SortDir::Desc, ..Default::default() };
        assert_eq!(f.apply(&l), vec![1, 0, 2]);
        let f = Filters { sort: SortKey::Gamemode, dir: SortDir::Asc, ..Default::default() };
        assert_eq!(f.apply(&l), vec![2, 0, 1]);
    }

    #[test]
    fn gamemode_and_version_filters() {
        let l = list();
        let f = Filters { gamemode: "BRAVO".into(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![0]);
        assert_eq!(f.active_count(), 1);
        let f = Filters { versions: ["0.3.7".to_string()].into_iter().collect(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![1]);
        let f = Filters { versions: ["open.mp".to_string()].into_iter().collect(), ..Default::default() };
        assert_eq!(f.apply(&l), vec![0, 2]);
        // settings written before these fields existed still load
        let old: Filters = toml::from_str("query = \"x\"\nomp_only = true\nsort = \"Ping\"\ndir = \"Asc\"").unwrap();
        assert_eq!(
            old,
            Filters { query: "x".into(), omp_only: true, sort: SortKey::Ping, dir: SortDir::Asc, ..Default::default() }
        );
    }

    #[test]
    fn version_families() {
        assert_eq!(version_family("omp 1.5.8.3079"), "open.mp");
        assert_eq!(version_family("0.3.7-R2"), "0.3.7");
        assert_eq!(version_family("0.3.DL-R1"), "0.3.DL");
        assert_eq!(version_family("0.3.7"), "0.3.7");
        assert_eq!(version_family("unknown"), "Unknown");
        assert_eq!(version_family(""), "Unknown");
    }

    #[test]
    fn language_normalization() {
        assert_eq!(normalize_language("english/russian"), "English");
        assert_eq!(normalize_language("RO/EN"), "Ro");
        assert_eq!(normalize_language(""), "Unknown");
        assert_eq!(normalize_language("  Deutsch "), "Deutsch");
    }

    #[test]
    fn sort_key_cycles() {
        assert_eq!(SortKey::None.next(), SortKey::Players);
        assert_eq!(SortKey::Gamemode.next(), SortKey::None);
    }
}
