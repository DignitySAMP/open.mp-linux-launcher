// Master list client for https://api.open.mp/servers. The API defines the short field names.

use crate::model::{Server, ServerAddr, ServerInfo};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://api.open.mp";
pub const API_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected response from {url}: {msg}")]
    Shape { url: String, msg: String },
}

#[derive(Debug, Deserialize)]
struct ApiCore {
    #[serde(default)]
    ip: String,
    #[serde(default)]
    hn: String,
    #[serde(default)]
    pc: u32,
    #[serde(default)]
    pm: u32,
    #[serde(default)]
    gm: String,
    #[serde(default)]
    la: String,
    #[serde(default)]
    pa: bool,
    #[serde(default)]
    vn: String,
    #[serde(default)]
    omp: bool,
    #[serde(default)]
    pr: bool,
}

#[derive(Debug, Deserialize)]
struct ApiFull {
    core: ApiCore,
    #[serde(default)]
    ru: Option<BTreeMap<String, serde_json::Value>>,
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl ApiCore {
    fn into_server(self) -> Option<Server> {
        let addr: ServerAddr = self.ip.parse().ok()?;
        Some(Server {
            addr: Some(addr),
            host_label: None,
            info: ServerInfo {
                hostname: crate::encoding::sanitize(&self.hn),
                gamemode: crate::encoding::sanitize(&self.gm),
                language: crate::encoding::sanitize(&self.la),
                players: self.pc.min(u16::MAX as u32) as u16,
                max_players: self.pm.min(u16::MAX as u32) as u16,
                password: self.pa,
                version: self.vn,
                omp: self.omp,
                partner: self.pr,
            },
            ..Default::default()
        })
    }
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base: String,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .user_agent(concat!("omp-tui/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        Self { http, base: base_url.trim_end_matches('/').to_owned() }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub(crate) async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        let body = resp.bytes().await?;
        serde_json::from_slice(&body).map_err(|e| ApiError::Shape { url, msg: e.to_string() })
    }

    pub async fn servers(&self) -> Result<Vec<Server>, ApiError> {
        let raw: Vec<ApiCore> = self.get_json("/servers").await?;
        Ok(raw.into_iter().filter_map(ApiCore::into_server).collect())
    }

    pub async fn servers_full(&self) -> Result<Vec<Server>, ApiError> {
        let raw: Vec<ApiFull> = self.get_json("/servers/full").await?;
        Ok(raw
            .into_iter()
            .filter_map(|e| {
                let mut s = e.core.into_server()?;
                if let Some(ru) = e.ru {
                    s.rules = ru.iter().map(|(k, v)| (k.clone(), value_to_string(v))).collect();
                }
                Some(s)
            })
            .collect())
    }
}

pub mod fixtures {
    pub const SERVERS: &str = r#"[
      {"ip":"151.241.103.135:7777","hn":"GoldEagle RPG","pc":5,"pm":100,"gm":"EAGLE GM","la":"RO/EN","pa":false,"vn":"omp 1.5.8.3079","omp":true,"pr":true},
      {"ip":"94.189.183.81:7777","hn":"Old Story open.mp","pc":0,"pm":1000,"gm":"[OS] v1.0.0","la":"Balkanski","pa":true,"vn":"omp 1.5.8.3079","omp":true,"pr":false},
      {"ip":"not-an-ip","hn":"broken","pc":0,"pm":0,"gm":"","la":"","pa":false,"vn":"","omp":false,"pr":false},
      {"ip":"1.2.3.4:7777","hn":"Legacy SA-MP","pc":10,"pm":50,"gm":"DM","la":"English","pa":false,"vn":"0.3.7-R2","omp":false,"pr":false}
    ]"#;

    pub const FULL: &str = r#"[
      {"ip":"151.241.103.135:7777","dm":null,"core":{"ip":"151.241.103.135:7777","hn":"GoldEagle RPG","pc":5,"pm":100,"gm":"EAGLE GM","la":"RO/EN","pa":false,"vn":"omp 1.5.8.3079","omp":true,"pr":true},
       "ru":{"artwork":"No","lagcomp":"On","mapname":"LS/LV","weather":"3","weburl":"panel.goldeagle.ro","worldtime":"9:00","gravity":0.008},"description":null}
    ]"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::{FULL, SERVERS};
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn parses_servers_and_skips_bad_entries() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SERVERS, "application/json"))
            .mount(&mock)
            .await;
        let api = ApiClient::new(&mock.uri());
        let s = api.servers().await.unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].info.hostname, "GoldEagle RPG");
        assert!(s[0].info.partner && s[0].info.omp);
        assert_eq!(s[1].info.max_players, 1000);
        assert!(s[1].info.password);
        assert_eq!(s[2].info.hostname, "Legacy SA-MP");
        assert_eq!(s[2].addr.unwrap().to_string(), "1.2.3.4:7777");
    }

    #[tokio::test]
    async fn parses_full_with_rules() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers/full"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(FULL, "application/json"))
            .mount(&mock)
            .await;
        let api = ApiClient::new(&mock.uri());
        let s = api.servers_full().await.unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].rules["mapname"], "LS/LV");
        assert_eq!(s[0].rules["gravity"], "0.008");
    }

    #[tokio::test]
    async fn http_errors_and_bad_json_are_errors() {
        let mock = MockServer::start().await;
        Mock::given(method("GET")).and(path("/servers")).respond_with(ResponseTemplate::new(500)).mount(&mock).await;
        Mock::given(method("GET"))
            .and(path("/servers/full"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("{not json", "application/json"))
            .mount(&mock)
            .await;
        let api = ApiClient::new(&mock.uri());
        assert!(matches!(api.servers().await.unwrap_err(), ApiError::Http(_)));
        assert!(matches!(api.servers_full().await.unwrap_err(), ApiError::Shape { .. }));
    }
}
