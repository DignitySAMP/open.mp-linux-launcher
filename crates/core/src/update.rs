// Latest release tag on GitHub, checked once at start-up.

use serde::Deserialize;
use std::time::Duration;

pub const RELEASES_URL: &str = "https://api.github.com/repos/DignitySAMP/open.mp-linux-launcher/releases/latest";
pub const RELEASE_PAGE: &str = "https://github.com/DignitySAMP/open.mp-linux-launcher/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

// Returns the version without the leading v.
pub async fn latest_version(url: &str) -> Result<String, reqwest::Error> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("omp-tui/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let r: Release = http.get(url).send().await?.error_for_status()?.json().await?;
    Ok(r.tag_name.trim_start_matches('v').to_owned())
}

// Numeric compare per component, so 0.1.10 is newer than 0.1.9. Anything else is not "newer".
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u64>().ok()).collect::<Option<Vec<_>>>();
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn version_compare() {
        assert!(is_newer("0.1.4", "0.1.3"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("0.2", "0.1.9"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("0.1.4-rc1", "0.1.3"));
        assert!(!is_newer("", "0.1.3"));
    }

    // cargo test -p omptui-core -- --ignored real_github
    #[tokio::test]
    #[ignore]
    async fn real_github_endpoint_answers() {
        let v = latest_version(RELEASES_URL).await.unwrap();
        assert!(v.split('.').all(|n| n.parse::<u64>().is_ok()), "{v}");
    }

    #[tokio::test]
    async fn reads_tag_from_github_json() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .and(header_exists("user-agent"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(r#"{"tag_name":"v0.9.0","name":"x"}"#, "application/json"),
            )
            .mount(&mock)
            .await;
        assert_eq!(latest_version(&format!("{}/releases/latest", mock.uri())).await.unwrap(), "0.9.0");
        assert!(latest_version(&format!("{}/missing", mock.uri())).await.is_err());
    }
}
