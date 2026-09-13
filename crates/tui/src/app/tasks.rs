use omptui_core::api::ApiClient;
use omptui_core::launch::{self, HelperEvent, Prepared};
use omptui_core::query::{BasicResult, FullResult, Querier};
use omptui_core::wine::WineEnv;
use omptui_core::{Server, ServerAddr};
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    ApiLoaded(Result<Vec<Server>, String>),
    Basic { addr: ServerAddr, result: BasicResult },
    Full { addr: ServerAddr, result: FullResult },
    Ping { addr: ServerAddr, ms: u32 },
    LaunchPrepared(Result<Box<Prepared>, String>),
    Launch(HelperEvent),
    LaunchFinished(Result<Option<i32>, String>),
    Resolved { result: Result<(ServerAddr, String), String>, join: bool },
    ImportedFavorites(Result<Vec<Server>, String>),
    TaskDone { title: String, result: Result<Vec<String>, String> },
    Tick,
}

pub type Tx = mpsc::UnboundedSender<AppEvent>;

#[derive(Clone)]
pub struct Services {
    pub api: ApiClient,
    pub querier: Querier,
    pub tx: Tx,
    pub helper: &'static [u8],
}

impl Services {
    pub fn fetch_api(&self) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let r = api.servers().await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::ApiLoaded(r));
        });
    }

    pub fn query_basic_many(&self, addrs: Vec<ServerAddr>) {
        for addr in addrs {
            let q = self.querier.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = q.query_basic(addr).await;
                let _ = tx.send(AppEvent::Basic { addr, result });
            });
        }
    }

    pub fn query_full(&self, addr: ServerAddr, with_extra: bool) {
        let q = self.querier.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = q.query_all(addr, with_extra).await;
            let _ = tx.send(AppEvent::Full { addr, result });
        });
    }

    pub fn ping(&self, addr: ServerAddr) {
        let q = self.querier.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let ms = q.ping_or_unreachable(addr).await;
            let _ = tx.send(AppEvent::Ping { addr, ms });
        });
    }

    pub fn resolve(&self, input: String, join: bool) {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = omptui_core::validation::resolve_host(&input).map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Resolved { result, join });
        });
    }

    pub fn launch(&self, req: launch::LaunchRequest, files: omptui_core::resources::ClientFiles) {
        let tx = self.tx.clone();
        let helper = self.helper;
        tokio::spawn(async move {
            let files2 = files.clone();
            let req2 = req.clone();
            let prepared =
                tokio::task::spawn_blocking(move || launch::prepare(&req2, &files2, helper).map_err(|e| e.to_string()))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
            let prepared = match prepared {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(AppEvent::LaunchPrepared(Err(e)));
                    return;
                }
            };
            let _ = tx.send(AppEvent::LaunchPrepared(Ok(Box::new(prepared.clone()))));
            let (etx, mut erx) = mpsc::channel(64);
            let tx2 = tx.clone();
            let forward = tokio::spawn(async move {
                while let Some(ev) = erx.recv().await {
                    let _ = tx2.send(AppEvent::Launch(ev));
                }
            });
            let r = launch::run(&prepared, etx).await.map_err(|e| e.to_string());
            let _ = forward.await;
            let _ = tx.send(AppEvent::LaunchFinished(r));
        });
    }

    pub fn download_client_files(&self, files: omptui_core::resources::ClientFiles, assets_base: String) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            let mut steps = Vec::new();
            let result =
                omptui_core::download::download_client_files(&files, &api, &assets_base, &mut |s| steps.push(s))
                    .await
                    .map(|report| steps.iter().cloned().chain(report).collect())
                    .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone { title: "Download client files".into(), result });
        });
    }

    pub fn init_prefix(&self, env: WineEnv) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = env.init_prefix().await.map(|out| {
                let mut lines = vec![format!("prefix ready: {}", env.prefix.display())];
                lines.extend(out.lines().filter(|l| !l.trim().is_empty()).take(20).map(str::to_owned));
                lines
            });
            let _ = tx.send(AppEvent::TaskDone { title: "Wine prefix".into(), result });
        });
    }

    pub fn install_d3dx9(&self, env: WineEnv) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = crate::install_d3dx9(&env).await.map(|()| vec!["d3dx9_25.dll installed".to_string()]);
            let _ = tx.send(AppEvent::TaskDone { title: "Install d3dx9".into(), result });
        });
    }

    pub fn import_userdata(&self, path: PathBuf) {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = (|| {
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let entries = omptui_core::import::parse_userdata(&bytes).map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for e in entries {
                    match omptui_core::validation::resolve_host(&format!("{}:{}", e.host, e.port)) {
                        Ok((addr, label)) => {
                            let mut s = Server::with_addr(addr);
                            s.host_label = Some(label);
                            s.info.hostname = if e.name.is_empty() { e.host.clone() } else { e.name.clone() };
                            s.info.password = !e.password.is_empty();
                            out.push(s);
                        }
                        Err(err) => tracing::warn!("skipping {}:{}: {err}", e.host, e.port),
                    }
                }
                Ok(out)
            })();
            let _ = tx.send(AppEvent::ImportedFavorites(result));
        });
    }

    pub fn blocking_task(
        &self,
        title: impl Into<String>,
        f: impl FnOnce() -> Result<Vec<String>, String> + Send + 'static,
    ) {
        let tx = self.tx.clone();
        let title = title.into();
        tokio::task::spawn_blocking(move || {
            let result = f();
            let _ = tx.send(AppEvent::TaskDone { title, result });
        });
    }
}
