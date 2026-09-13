use super::packet::{self, InfoPacket, Opcode};
use super::packet_encoders as enc;
use crate::model::{ExtraInfo, Player, ServerAddr};
use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct FakeServerConfig {
    pub info: InfoPacket,
    pub players: Vec<Player>,
    pub rules: Vec<(String, String)>,
    pub extra: Option<ExtraInfo>,
    // Only these opcodes get an answer, the rest are dropped.
    pub answer: HashSet<Opcode>,
    pub delay: Duration,
    pub corrupt: bool,
    pub bind: SocketAddrV4,
}

impl FakeServerConfig {
    pub fn sample(hostname: &str) -> Self {
        Self {
            info: InfoPacket {
                password: false,
                players: 2,
                max_players: 100,
                hostname: hostname.to_owned(),
                gamemode: "Freeroam".into(),
                language: "English".into(),
            },
            players: vec![Player { name: "johnny".into(), score: 123 }, Player { name: "bigsmoke".into(), score: 69 }],
            rules: vec![
                ("mapname".into(), "San Andreas".into()),
                ("version".into(), "omp 1.5.8.3079".into()),
                ("weather".into(), "10".into()),
                ("weburl".into(), "example.org".into()),
                ("worldtime".into(), "12:00".into()),
            ],
            extra: None,
            answer: [Opcode::Info, Opcode::Rules, Opcode::Players, Opcode::Ping, Opcode::Extra].into_iter().collect(),
            delay: Duration::ZERO,
            corrupt: false,
            bind: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        }
    }
}

pub struct FakeServer {
    addr: ServerAddr,
    requests: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeServer {
    pub async fn start(cfg: FakeServerConfig) -> io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::V4(cfg.bind)).await?;
        let SocketAddr::V4(local) = socket.local_addr()? else { unreachable!() };
        let addr = ServerAddr::new(*local.ip(), local.port());
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let task = tokio::spawn(async move {
            let socket = Arc::new(socket);
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, from)) = socket.recv_from(&mut buf).await else { break };
                let data = buf[..n].to_vec();
                counter.fetch_add(1, Ordering::SeqCst);
                let Ok((hdr, payload)) = packet::split_response(&data) else { continue };
                if !cfg.answer.contains(&hdr.op) {
                    continue;
                }
                let body = if cfg.corrupt {
                    vec![0xff; 4]
                } else {
                    match hdr.op {
                        Opcode::Info => enc::encode_info(&cfg.info),
                        Opcode::Rules => enc::encode_rules(&cfg.rules),
                        Opcode::Players => enc::encode_players(&cfg.players),
                        Opcode::Ping => payload.to_vec(),
                        Opcode::Extra => match &cfg.extra {
                            Some(e) => enc::encode_extra(e),
                            None => continue,
                        },
                    }
                };
                let reply = enc::encode_response(hdr.addr, hdr.op, &body);
                let sock = socket.clone();
                let delay = cfg.delay;
                tokio::spawn(async move {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let _ = sock.send_to(&reply, from).await;
                });
            }
        });
        Ok(Self { addr, requests, task })
    }

    pub fn addr(&self) -> ServerAddr {
        self.addr
    }

    pub fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}
