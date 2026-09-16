use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;

// The official launcher shows 9999 for servers that did not answer.
pub const UNREACHABLE_PING: u32 = 9999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerAddr {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl ServerAddr {
    pub const fn new(ip: Ipv4Addr, port: u16) -> Self {
        Self { ip, port }
    }

    pub fn socket(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.ip, self.port)
    }
}

impl fmt::Display for ServerAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AddrParseError {
    #[error("expected ip:port")]
    Shape,
    #[error("invalid IPv4 address")]
    Ip,
    #[error("invalid port (1-65535)")]
    Port,
}

impl FromStr for ServerAddr {
    type Err = AddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (host, port) = s.trim().rsplit_once(':').ok_or(AddrParseError::Shape)?;
        let ip = host.parse::<Ipv4Addr>().map_err(|_| AddrParseError::Ip)?;
        let port = port.parse::<u16>().map_err(|_| AddrParseError::Port)?;
        if port == 0 {
            return Err(AddrParseError::Port);
        }
        Ok(Self { ip, port })
    }
}

impl Serialize for ServerAddr {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ServerAddr {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub score: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraInfo {
    pub discord: String,
    pub light_banner: String,
    pub dark_banner: String,
    pub logo: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub hostname: String,
    pub gamemode: String,
    pub language: String,
    pub players: u16,
    pub max_players: u16,
    pub password: bool,
    pub version: String,
    pub omp: bool,
    pub partner: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub addr: Option<ServerAddr>,
    // The address as the user typed it, for servers added by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_label: Option<String>,
    #[serde(flatten)]
    pub info: ServerInfo,
    #[serde(default)]
    pub rules: BTreeMap<String, String>,
    #[serde(default)]
    pub player_list: Vec<Player>,
    #[serde(default)]
    pub extra: Option<ExtraInfo>,
    #[serde(default)]
    pub ping: Option<u32>,
    #[serde(default)]
    pub queried: bool,
}

impl Server {
    pub fn with_addr(addr: ServerAddr) -> Self {
        Self { addr: Some(addr), ..Default::default() }
    }

    pub fn address_text(&self) -> String {
        match (&self.addr, &self.host_label) {
            (Some(a), _) => a.to_string(),
            (None, Some(l)) => l.clone(),
            (None, None) => String::from("?"),
        }
    }

    pub fn is_unreachable(&self) -> bool {
        self.ping == Some(UNREACHABLE_PING)
    }

    pub fn display_name(&self) -> String {
        if self.info.hostname.is_empty() { self.address_text() } else { self.info.hostname.clone() }
    }

    // weburl is the rule SA-MP servers set for this; the others turn up in the wild.
    pub fn website(&self) -> Option<String> {
        ["weburl", "website", "url"].iter().find_map(|k| self.rules.get(*k)).and_then(|v| http_link(v))
    }

    pub fn discord(&self) -> Option<String> {
        self.extra.as_ref().and_then(|e| http_link(&e.discord))
    }
}

// Rules are free text, so only http(s) links go to the browser. A bare "example.com" gets https://.
pub fn http_link(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(s.to_owned());
    }
    if s.contains("://") || !s.contains('.') {
        return None;
    }
    Some(format!("https://{s}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListKind {
    Favorites,
    Internet,
    Partners,
    Recent,
}

impl ListKind {
    pub const ALL: [ListKind; 4] = [ListKind::Favorites, ListKind::Internet, ListKind::Partners, ListKind::Recent];

    pub fn title(self) -> &'static str {
        match self {
            ListKind::Favorites => "Favorites",
            ListKind::Internet => "Internet",
            ListKind::Partners => "Partners",
            ListKind::Recent => "Recent",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addr() {
        let a: ServerAddr = "1.2.3.4:7777".parse().unwrap();
        assert_eq!(a, ServerAddr::new(Ipv4Addr::new(1, 2, 3, 4), 7777));
        assert_eq!(a.to_string(), "1.2.3.4:7777");
        assert_eq!("1.2.3.4".parse::<ServerAddr>(), Err(AddrParseError::Shape));
        assert_eq!("host:7777".parse::<ServerAddr>(), Err(AddrParseError::Ip));
        assert_eq!("1.2.3.4:0".parse::<ServerAddr>(), Err(AddrParseError::Port));
        assert_eq!("1.2.3.4:70000".parse::<ServerAddr>(), Err(AddrParseError::Port));
    }

    #[test]
    fn links_from_rules_and_extra() {
        assert_eq!(http_link("www.example.com/forum"), Some("https://www.example.com/forum".into()));
        assert_eq!(http_link(" HTTP://x.y "), Some("HTTP://x.y".into()));
        assert_eq!(http_link("discord.gg/abc"), Some("https://discord.gg/abc".into()));
        assert_eq!(http_link("none"), None);
        assert_eq!(http_link("-"), None);
        assert_eq!(http_link("file:///etc/passwd"), None);
        assert_eq!(http_link("two words.com"), None);
        let mut s = Server::with_addr("1.2.3.4:7777".parse().unwrap());
        assert_eq!(s.website(), None);
        assert_eq!(s.discord(), None);
        s.rules.insert("weburl".into(), "example.com".into());
        s.extra = Some(ExtraInfo { discord: "https://discord.gg/abc".into(), ..Default::default() });
        assert_eq!(s.website().as_deref(), Some("https://example.com"));
        assert_eq!(s.discord().as_deref(), Some("https://discord.gg/abc"));
    }

    #[test]
    fn addr_serde_roundtrip() {
        let a: ServerAddr = "10.0.0.1:7000".parse().unwrap();
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(j, "\"10.0.0.1:7000\"");
        let b: ServerAddr = serde_json::from_str(&j).unwrap();
        assert_eq!(a, b);
    }
}
