// Downloads the client files from the same places as the official launcher (https://github.com/openmultiplayer/launcher):
// https://assets.open.mp/samp_clients.7z and the omp-client.dll URL and checksum published by
// https://api.open.mp/launcher.

use crate::api::{ApiClient, ApiError};
use crate::resources::{ClientFiles, SHARED_FILES, SampVersion, md5_bytes, md5_file};
use crate::store::write_atomic;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

pub const DEFAULT_ASSETS_URL: &str = "https://assets.open.mp";
pub const SAMP_CLIENTS_ARCHIVE: &str = "samp_clients.7z";
pub const SAMP_CLIENTS_MD5: &str = "5572377f1c6f9fbcb673a8cf26c19984";

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("download of {url} failed: {msg}")]
    Http { url: String, msg: String },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("cannot extract {0}: {1}")]
    Archive(String, String),
    #[error("{file} downloaded from {url} has checksum {actual}, expected {expected}")]
    Checksum { file: String, url: String, actual: String, expected: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherInfo {
    pub omp_client_url: String,
    pub omp_client_md5: String,
}

#[derive(Debug, Deserialize)]
struct LauncherEntry {
    #[serde(rename = "ompPluginDownload")]
    download: String,
    #[serde(rename = "ompPluginChecksum")]
    checksum: String,
}

#[derive(Debug, Deserialize)]
struct LauncherResponse {
    #[serde(flatten)]
    latest: LauncherEntry,
    #[serde(default)]
    versions: BTreeMap<String, LauncherEntry>,
}

impl ApiClient {
    // The response has one entry per launcher build number. The highest one is current.
    pub async fn launcher_info(&self) -> Result<LauncherInfo, ApiError> {
        let r: LauncherResponse = self.get_json("/launcher").await?;
        let newest =
            r.versions.iter().filter_map(|(k, v)| k.parse::<u32>().ok().map(|n| (n, v))).max_by_key(|(n, _)| *n);
        let entry = newest.map(|(_, v)| v).unwrap_or(&r.latest);
        Ok(LauncherInfo { omp_client_url: entry.download.clone(), omp_client_md5: entry.checksum.to_lowercase() })
    }
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, DownloadError> {
    let err = |msg: String| DownloadError::Http { url: url.to_owned(), msg };
    let resp = http.get(url).send().await.map_err(|e| err(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(err(format!("HTTP {}", resp.status())));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| err(e.to_string()))
}

fn safe_relative(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    if name.is_empty()
        || name.starts_with('/')
        || name.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return None;
    }
    Some(name)
}

pub fn extract_7z(bytes: &[u8], dest: &Path) -> Result<Vec<String>, DownloadError> {
    let arch = |e: sevenz_rust2::Error| DownloadError::Archive(SAMP_CLIENTS_ARCHIVE.into(), e.to_string());
    let mut reader =
        sevenz_rust2::ArchiveReader::new(Cursor::new(bytes), sevenz_rust2::Password::empty()).map_err(arch)?;
    let mut written = Vec::new();
    let mut failure: Option<DownloadError> = None;
    reader
        .for_each_entries(|entry, data| {
            if entry.is_directory {
                return Ok(true);
            }
            let Some(rel) = safe_relative(&entry.name) else { return Ok(true) };
            let mut buf = Vec::new();
            data.read_to_end(&mut buf)?;
            if let Err(e) = write_atomic(&dest.join(&rel), &buf) {
                failure = Some(e.into());
                return Ok(false);
            }
            written.push(rel);
            Ok(true)
        })
        .map_err(arch)?;
    match failure {
        Some(e) => Err(e),
        None => Ok(written),
    }
}

pub async fn download_client_files(
    files: &ClientFiles,
    api: &ApiClient,
    assets_base: &str,
    progress: &mut (dyn FnMut(String) + Send),
) -> Result<Vec<String>, DownloadError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent(concat!("omp-tui/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client");
    let mut report = Vec::new();
    let samp_dir = files.data_dir.join("samp");
    let archive = samp_dir.join(SAMP_CLIENTS_ARCHIVE);

    let complete = SampVersion::ALL.iter().filter_map(|v| files.samp_dll(*v)).all(|p| p.is_file())
        && SHARED_FILES.iter().all(|f| files.shared_dir().join(f.rel).is_file());
    if complete {
        report.push("SA-MP client files already present".into());
    } else {
        let url = format!("{}/{}", assets_base.trim_end_matches('/'), SAMP_CLIENTS_ARCHIVE);
        let bytes = match md5_file(&archive) {
            Ok(sum) if sum == SAMP_CLIENTS_MD5 => std::fs::read(&archive)?,
            _ => {
                progress(format!("downloading {url}"));
                let bytes = fetch(&http, &url).await?;
                write_atomic(&archive, &bytes)?;
                bytes
            }
        };
        let sum = md5_bytes(&bytes);
        if sum != SAMP_CLIENTS_MD5 {
            report.push(format!(
                "{SAMP_CLIENTS_ARCHIVE} has checksum {sum}, expected {SAMP_CLIENTS_MD5}; unpacking anyway"
            ));
        }
        progress(format!("unpacking {SAMP_CLIENTS_ARCHIVE}"));
        let written = extract_7z(&bytes, &samp_dir)?;
        report.push(format!("unpacked {} files from {SAMP_CLIENTS_ARCHIVE}", written.len()));
        for v in SampVersion::ALL {
            if let Some(p) = files.samp_dll(v)
                && let (Some(expected), Ok(actual)) = (v.dll_md5(), md5_file(&p))
                && actual != expected
            {
                report.push(format!("samp.dll {} has checksum {actual}, expected {expected}", v.label()));
            }
        }
    }

    progress("checking the open.mp client version".into());
    let info = api.launcher_info().await?;
    let dll = files.omp_client_dll();
    if md5_file(&dll).ok().as_deref() == Some(info.omp_client_md5.as_str()) {
        report.push("omp-client.dll is up to date".into());
    } else {
        progress(format!("downloading {}", info.omp_client_url));
        let bytes = fetch(&http, &info.omp_client_url).await?;
        let actual = md5_bytes(&bytes);
        if actual != info.omp_client_md5 {
            return Err(DownloadError::Checksum {
                file: "omp-client.dll".into(),
                url: info.omp_client_url,
                actual,
                expected: info.omp_client_md5,
            });
        }
        write_atomic(&dll, &bytes)?;
        report.push(format!("omp-client.dll downloaded ({} bytes)", bytes.len()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::FileState;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fake_archive(dir: &Path) -> Vec<u8> {
        for v in SampVersion::ALL {
            if let Some(d) = v.dir_name() {
                std::fs::create_dir_all(dir.join(d)).unwrap();
                std::fs::write(dir.join(d).join("samp.dll"), format!("dll {}", v.id())).unwrap();
            }
        }
        for f in SHARED_FILES {
            let p = dir.join("shared").join(f.rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f.rel).unwrap();
        }
        let out = dir.parent().unwrap().join("clients.7z");
        sevenz_rust2::compress_to_path(dir, &out).unwrap();
        std::fs::read(out).unwrap()
    }

    #[tokio::test]
    async fn downloads_unpacks_and_verifies() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = fake_archive(&tmp.path().join("src"));
        let mock = MockServer::start().await;
        let dll_body = b"the omp client".to_vec();
        let launcher = format!(
            r#"{{"download":"x","ompPluginChecksum":"old","ompPluginDownload":"{0}/old.dll","version":"5",
                "versions":{{"5":{{"download":"x","ompPluginChecksum":"old","ompPluginDownload":"{0}/old.dll"}},
                             "6":{{"download":"x","ompPluginChecksum":"{1}","ompPluginDownload":"{0}/omp-client-6.dll"}}}}}}"#,
            mock.uri(),
            md5_bytes(&dll_body)
        );
        Mock::given(method("GET"))
            .and(path("/launcher"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(launcher, "application/json"))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/samp_clients.7z"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/omp-client-6.dll"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(dll_body.clone()))
            .mount(&mock)
            .await;

        let files = ClientFiles::new(tmp.path().join("data"));
        let api = ApiClient::new(&mock.uri());
        let mut steps = Vec::new();
        let report = download_client_files(&files, &api, &mock.uri(), &mut |s| steps.push(s)).await.unwrap();
        assert!(steps.iter().any(|s| s.contains("samp_clients.7z")), "{steps:?}");
        assert!(report.iter().any(|l| l.starts_with("unpacked 23 files")), "{report:?}");
        assert!(report.iter().any(|l| l.contains("expected 5572377f")), "archive checksum warning: {report:?}");
        assert_eq!(std::fs::read(files.omp_client_dll()).unwrap(), dll_body);
        assert_eq!(std::fs::read_to_string(files.samp_dll(SampVersion::DL).unwrap()).unwrap(), "dll 03DL");
        assert!(files.shared_dir().join("SAMP/SAMP.img").is_file());
        let st = files.check(SampVersion::R5, true);
        assert!(st.iter().all(|s| !matches!(s.state, FileState::Missing)), "{st:?}");

        let mut steps = Vec::new();
        let report = download_client_files(&files, &api, &mock.uri(), &mut |s| steps.push(s)).await.unwrap();
        assert!(report.contains(&"SA-MP client files already present".to_string()));
        assert!(report.contains(&"omp-client.dll is up to date".to_string()));
        assert!(!steps.iter().any(|s| s.starts_with("downloading")), "{steps:?}");
    }

    #[tokio::test]
    async fn bad_dll_checksum_is_rejected_and_http_errors_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = fake_archive(&tmp.path().join("src"));
        let mock = MockServer::start().await;
        let launcher = format!(
            r#"{{"download":"x","ompPluginChecksum":"{}","ompPluginDownload":"{}/omp-client.dll","version":"6"}}"#,
            "0".repeat(32),
            mock.uri()
        );
        Mock::given(method("GET"))
            .and(path("/launcher"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(launcher, "application/json"))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/samp_clients.7z"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/omp-client.dll"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"wrong".to_vec()))
            .mount(&mock)
            .await;
        let files = ClientFiles::new(tmp.path().join("data"));
        let api = ApiClient::new(&mock.uri());
        let err = download_client_files(&files, &api, &mock.uri(), &mut |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Checksum { .. }), "{err}");
        assert!(!files.omp_client_dll().exists());

        let files2 = ClientFiles::new(tmp.path().join("data2"));
        let err =
            download_client_files(&files2, &api, &format!("{}/missing", mock.uri()), &mut |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Http { .. }), "{err}");
    }

    #[test]
    fn archive_paths_are_sanitised() {
        assert_eq!(safe_relative("shared/SAMP/SAMP.img").as_deref(), Some("shared/SAMP/SAMP.img"));
        assert_eq!(safe_relative("shared\\bass.dll").as_deref(), Some("shared/bass.dll"));
        assert!(safe_relative("../etc/passwd").is_none());
        assert!(safe_relative("/abs").is_none());
        assert!(safe_relative("a//b").is_none());
        assert!(safe_relative("").is_none());
    }

    #[test]
    fn real_archive_unpacks_with_known_checksums() {
        let mut candidates = vec![];
        if let Some(p) = std::env::var_os("OMPTUI_TEST_ARCHIVE") {
            candidates.push(std::path::PathBuf::from(p));
        }
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            candidates.push(home.join(".local/share/omp-tui/samp/samp_clients.7z"));
        }
        let Some(archive) = candidates.iter().find(|p| p.is_file()) else {
            eprintln!("skipped: no samp_clients.7z on this machine (set OMPTUI_TEST_ARCHIVE)");
            return;
        };
        let bytes = std::fs::read(archive).unwrap();
        assert_eq!(md5_bytes(&bytes), SAMP_CLIENTS_MD5);
        let tmp = tempfile::tempdir().unwrap();
        let written = extract_7z(&bytes, tmp.path()).unwrap();
        assert_eq!(written.len(), 23);
        for f in SHARED_FILES {
            assert_eq!(md5_file(&tmp.path().join("shared").join(f.rel)).unwrap(), f.md5.unwrap(), "{}", f.rel);
        }
        for v in SampVersion::ALL {
            if let Some(d) = v.dir_name() {
                assert_eq!(md5_file(&tmp.path().join(d).join("samp.dll")).unwrap(), v.dll_md5().unwrap(), "{d}");
            }
        }
    }
}
