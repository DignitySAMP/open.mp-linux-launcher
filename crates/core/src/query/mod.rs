pub mod fake_server;
pub mod packet;
pub mod packet_encoders;

use crate::model::{ExtraInfo, Player, Server, ServerAddr, UNREACHABLE_PING};
use packet::{InfoPacket, Opcode, PacketError};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;

// Same values as the official launcher (https://github.com/openmultiplayer/launcher).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
pub const MIN_QUERY_INTERVAL: Duration = Duration::from_secs(1);
pub const EXTRA_INFO_COOLDOWN: Duration = Duration::from_secs(3);
pub const DEFAULT_MAX_IN_FLIGHT: usize = 128;

#[derive(Debug, Clone)]
pub struct QueryConfig {
    pub timeout: Duration,
    pub max_in_flight: usize,
    pub bind: SocketAddrV4,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            bind: SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("no answer within {0:?}")]
    Timeout(Duration),
    #[error("socket error: {0}")]
    Io(#[from] io::Error),
    #[error("malformed answer: {0}")]
    Packet(#[from] PacketError),
    #[error("query superseded by a newer one to the same server")]
    Superseded,
}

impl QueryError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, QueryError::Timeout(_))
    }
}

type Key = (SocketAddrV4, u8);
type Waiter = oneshot::Sender<(Vec<u8>, Instant)>;

struct Inner {
    socket: UdpSocket,
    pending: Mutex<HashMap<Key, Waiter>>,
    timeout: Duration,
    sem: Semaphore,
}

#[derive(Clone)]
pub struct Querier {
    inner: Arc<Inner>,
    _rx_task: Arc<AbortOnDrop>,
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Clone, Default)]
pub struct BasicResult {
    pub info: Option<InfoPacket>,
    pub ping: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct FullResult {
    pub basic: BasicResult,
    pub players: Option<Vec<Player>>,
    pub rules: Option<Vec<(String, String)>>,
    pub extra: Option<ExtraInfo>,
}

impl Querier {
    pub async fn bind(cfg: QueryConfig) -> io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::V4(cfg.bind)).await?;
        let inner = Arc::new(Inner {
            socket,
            pending: Mutex::new(HashMap::new()),
            timeout: cfg.timeout,
            sem: Semaphore::new(cfg.max_in_flight.max(1)),
        });
        let rx = {
            let inner = inner.clone();
            tokio::spawn(async move { Self::recv_loop(inner).await })
        };
        Ok(Self { inner, _rx_task: Arc::new(AbortOnDrop(rx)) })
    }

    pub fn timeout(&self) -> Duration {
        self.inner.timeout
    }

    async fn recv_loop(inner: Arc<Inner>) {
        // Player lists can hold up to 1000 entries, so a 1500 byte buffer is not enough.
        let mut buf = vec![0u8; 65535];
        loop {
            let (n, from) = match inner.socket.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("query socket recv error: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };
            let SocketAddr::V4(from) = from else { continue };
            let data = &buf[..n];
            // Match answers by sender address. The echoed header can contain a different
            // address when the server sits behind NAT.
            if data.len() < packet::HEADER_LEN || &data[..4] != packet::MAGIC {
                tracing::debug!("dropping junk datagram from {from}");
                continue;
            }
            let op = data[10];
            let waiter = inner.pending.lock().unwrap().remove(&(from, op));
            if let Some(tx) = waiter {
                let _ = tx.send((data.to_vec(), Instant::now()));
            } else {
                tracing::trace!("unsolicited datagram from {from} op {op:#x}");
            }
        }
    }

    async fn exchange(&self, addr: ServerAddr, op: Opcode, payload: &[u8]) -> Result<(Vec<u8>, Duration), QueryError> {
        let _permit = self.inner.sem.acquire().await.expect("semaphore never closed");
        let key = (addr.socket(), op.byte());
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self.inner.pending.lock().unwrap();
            p.insert(key, tx);
        }
        let req = packet::encode_request(addr, op, payload);
        let sent = Instant::now();
        if let Err(e) = self.inner.socket.send_to(&req, SocketAddr::V4(addr.socket())).await {
            self.inner.pending.lock().unwrap().remove(&key);
            return Err(e.into());
        }
        match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok((data, at))) => {
                let (_, payload) = packet::split_response(&data)?;
                Ok((payload.to_vec(), at.duration_since(sent)))
            }
            Ok(Err(_)) => Err(QueryError::Superseded),
            Err(_) => {
                let mut p = self.inner.pending.lock().unwrap();
                if p.get(&key).is_some_and(|w| w.is_closed()) {
                    p.remove(&key);
                }
                Err(QueryError::Timeout(self.inner.timeout))
            }
        }
    }

    pub async fn info(&self, addr: ServerAddr) -> Result<InfoPacket, QueryError> {
        let (p, _) = self.exchange(addr, Opcode::Info, &[]).await?;
        Ok(packet::decode_info(&p)?)
    }

    pub async fn ping(&self, addr: ServerAddr) -> Result<u32, QueryError> {
        let token: [u8; 4] = rand::random();
        let (p, rtt) = self.exchange(addr, Opcode::Ping, &token).await?;
        packet::check_ping(&p, &token)?;
        Ok(rtt.as_millis().min(u128::from(UNREACHABLE_PING - 1)) as u32)
    }

    pub async fn players(&self, addr: ServerAddr) -> Result<Vec<Player>, QueryError> {
        let (p, _) = self.exchange(addr, Opcode::Players, &[]).await?;
        Ok(packet::decode_players(&p)?)
    }

    pub async fn rules(&self, addr: ServerAddr) -> Result<Vec<(String, String)>, QueryError> {
        let (p, _) = self.exchange(addr, Opcode::Rules, &[]).await?;
        Ok(packet::decode_rules(&p)?)
    }

    pub async fn extra(&self, addr: ServerAddr) -> Result<ExtraInfo, QueryError> {
        let (p, _) = self.exchange(addr, Opcode::Extra, &[]).await?;
        Ok(packet::decode_extra(&p)?)
    }

    pub async fn ping_or_unreachable(&self, addr: ServerAddr) -> u32 {
        self.ping(addr).await.unwrap_or(UNREACHABLE_PING)
    }

    pub async fn query_basic(&self, addr: ServerAddr) -> BasicResult {
        let (info, ping) = tokio::join!(self.info(addr), self.ping(addr));
        BasicResult { info: info.ok(), ping: Some(ping.unwrap_or(UNREACHABLE_PING)) }
    }

    pub async fn query_all(&self, addr: ServerAddr, with_extra: bool) -> FullResult {
        let extra_fut = async { if with_extra { self.extra(addr).await.ok() } else { None } };
        let (basic, players, rules, extra) =
            tokio::join!(self.query_basic(addr), self.players(addr), self.rules(addr), extra_fut);
        FullResult { basic, players: players.ok(), rules: rules.ok(), extra }
    }
}

impl Server {
    pub fn apply_info(&mut self, i: &InfoPacket) {
        self.info.hostname = i.hostname.clone();
        self.info.gamemode = i.gamemode.clone();
        self.info.language = i.language.clone();
        self.info.players = i.players;
        self.info.max_players = i.max_players;
        self.info.password = i.password;
        self.queried = true;
    }

    pub fn apply_basic(&mut self, b: &BasicResult) {
        if let Some(i) = &b.info {
            self.apply_info(i);
        }
        if let Some(p) = b.ping {
            self.ping = Some(p);
        }
    }

    pub fn apply_rules(&mut self, rules: &[(String, String)]) {
        self.rules = rules.iter().cloned().collect();
        if let Some(v) = self.rules.get("version") {
            self.info.version = v.clone();
            if v.to_ascii_lowercase().starts_with("omp") {
                self.info.omp = true;
            }
        }
    }

    pub fn apply_full(&mut self, f: &FullResult) {
        self.apply_basic(&f.basic);
        if let Some(p) = &f.players {
            self.player_list = p.clone();
        }
        if let Some(r) = &f.rules {
            self.apply_rules(r);
        }
        if let Some(e) = &f.extra {
            self.extra = Some(e.clone());
            self.info.omp = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake_server::{FakeServer, FakeServerConfig};
    use super::*;
    use std::collections::HashSet;

    fn cfg() -> QueryConfig {
        QueryConfig {
            timeout: Duration::from_millis(500),
            bind: SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn queries_all_levels_against_fake_server() {
        let mut fc = FakeServerConfig::sample("Fake Server");
        fc.extra = Some(ExtraInfo { discord: "https://discord.gg/x".into(), ..Default::default() });
        let fake = FakeServer::start(fc).await.unwrap();
        let q = Querier::bind(cfg()).await.unwrap();
        let addr = fake.addr();

        let i = q.info(addr).await.unwrap();
        assert_eq!(i.hostname, "Fake Server");
        let ping = q.ping(addr).await.unwrap();
        assert!(ping < 500);
        let players = q.players(addr).await.unwrap();
        assert_eq!(players.len(), 2);
        let rules = q.rules(addr).await.unwrap();
        assert_eq!(rules[0].0, "mapname");
        let e = q.extra(addr).await.unwrap();
        assert_eq!(e.discord, "https://discord.gg/x");

        let full = q.query_all(addr, true).await;
        let mut s = Server::with_addr(addr);
        s.apply_full(&full);
        assert_eq!(s.info.hostname, "Fake Server");
        assert!(s.ping.unwrap() < 500);
        assert_eq!(s.player_list.len(), 2);
        assert_eq!(s.rules.get("mapname").unwrap(), "San Andreas");
        assert!(s.info.omp);
        assert_eq!(fake.request_count(), 5 + 5);
    }

    #[tokio::test]
    async fn timeout_when_server_is_silent() {
        let mut fc = FakeServerConfig::sample("Silent");
        fc.answer = HashSet::new();
        let fake = FakeServer::start(fc).await.unwrap();
        let q = Querier::bind(cfg()).await.unwrap();
        let err = q.info(fake.addr()).await.unwrap_err();
        assert!(err.is_timeout(), "{err}");
        assert_eq!(q.ping_or_unreachable(fake.addr()).await, UNREACHABLE_PING);
        let b = q.query_basic(fake.addr()).await;
        assert!(b.info.is_none());
        assert_eq!(b.ping, Some(UNREACHABLE_PING));
    }

    #[tokio::test]
    async fn partial_answers_only_fail_the_missing_parts() {
        let mut fc = FakeServerConfig::sample("Old SA-MP");
        fc.answer.remove(&Opcode::Extra);
        fc.answer.remove(&Opcode::Players);
        let fake = FakeServer::start(fc).await.unwrap();
        let q = Querier::bind(cfg()).await.unwrap();
        let full = q.query_all(fake.addr(), true).await;
        assert!(full.basic.info.is_some());
        assert!(full.players.is_none());
        assert!(full.rules.is_some());
        assert!(full.extra.is_none());
    }

    #[tokio::test]
    async fn bad_packets_are_reported_not_panicked() {
        let mut fc = FakeServerConfig::sample("Broken");
        fc.corrupt = true;
        let fake = FakeServer::start(fc).await.unwrap();
        let q = Querier::bind(cfg()).await.unwrap();
        let err = q.info(fake.addr()).await.unwrap_err();
        assert!(matches!(err, QueryError::Packet(_)), "{err}");
        let err = q.ping(fake.addr()).await.unwrap_err();
        assert!(matches!(err, QueryError::Packet(PacketError::PingMismatch)), "{err}");
    }

    #[tokio::test]
    async fn concurrency_limit_is_respected_and_many_servers_work() {
        let mut fakes = Vec::new();
        for n in 0..20 {
            fakes.push(FakeServer::start(FakeServerConfig::sample(&format!("S{n}"))).await.unwrap());
        }
        let q = Querier::bind(QueryConfig { max_in_flight: 4, ..cfg() }).await.unwrap();
        let mut handles = Vec::new();
        for f in &fakes {
            let q = q.clone();
            let addr = f.addr();
            handles.push(tokio::spawn(async move { q.query_basic(addr).await }));
        }
        for (n, h) in handles.into_iter().enumerate() {
            let b = h.await.unwrap();
            assert_eq!(b.info.unwrap().hostname, format!("S{n}"));
            assert!(b.ping.unwrap() < UNREACHABLE_PING);
        }
    }

    #[tokio::test]
    async fn slow_server_still_answers_within_timeout() {
        let mut fc = FakeServerConfig::sample("Slow");
        fc.delay = Duration::from_millis(200);
        let fake = FakeServer::start(fc).await.unwrap();
        let q = Querier::bind(cfg()).await.unwrap();
        let p = q.ping(fake.addr()).await.unwrap();
        assert!((150..500).contains(&p), "ping {p}");
    }
}
