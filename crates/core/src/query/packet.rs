// Query protocol, see https://sampwiki.blast.hk/wiki/Query_Mechanism
// A request is "SAMP", the IPv4 octets, the port as u16 LE and an opcode byte. The reply echoes
// those 11 bytes and appends the payload.

use crate::encoding::decode_text;
use crate::model::{ExtraInfo, Player, ServerAddr};

pub const HEADER_LEN: usize = 11;
pub const MAGIC: &[u8; 4] = b"SAMP";

// Length caps taken from the official launcher (https://github.com/openmultiplayer/launcher). Longer values are truncated.
pub const MAX_HOSTNAME: usize = 63;
pub const MAX_GAMEMODE: usize = 39;
pub const MAX_LANGUAGE: usize = 39;
pub const MAX_PLAYER_NAME: usize = 32;
pub const MAX_RULE_NAME: usize = 32;
pub const MAX_RULE_VALUE: usize = 70;
pub const MAX_PLAYERS: usize = 1000;
pub const MAX_RULES: usize = 20;
pub const MAX_DISCORD: usize = 50;
pub const MAX_URL: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Opcode {
    Info = b'i',
    Rules = b'r',
    Players = b'c',
    Ping = b'p',
    Extra = b'o',
}

impl Opcode {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            b'i' => Opcode::Info,
            b'r' => Opcode::Rules,
            b'c' => Opcode::Players,
            b'p' => Opcode::Ping,
            b'o' => Opcode::Extra,
            _ => return None,
        })
    }

    pub const fn byte(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("packet too short ({0} bytes)")]
    TooShort(usize),
    #[error("bad magic")]
    BadMagic,
    #[error("unknown opcode {0:#x}")]
    UnknownOpcode(u8),
    #[error("truncated {0} payload")]
    Truncated(&'static str),
    #[error("ping payload mismatch")]
    PingMismatch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoPacket {
    pub password: bool,
    pub players: u16,
    pub max_players: u16,
    pub hostname: String,
    pub gamemode: String,
    pub language: String,
}

pub fn encode_request(addr: ServerAddr, op: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER_LEN + payload.len());
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&addr.ip.octets());
    v.extend_from_slice(&addr.port.to_le_bytes());
    v.push(op.byte());
    v.extend_from_slice(payload);
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub addr: ServerAddr,
    pub op: Opcode,
}

pub fn split_response(packet: &[u8]) -> Result<(Header, &[u8]), PacketError> {
    if packet.len() < HEADER_LEN {
        return Err(PacketError::TooShort(packet.len()));
    }
    if &packet[0..4] != MAGIC {
        return Err(PacketError::BadMagic);
    }
    let ip = std::net::Ipv4Addr::new(packet[4], packet[5], packet[6], packet[7]);
    let port = u16::from_le_bytes([packet[8], packet[9]]);
    let op = Opcode::from_byte(packet[10]).ok_or(PacketError::UnknownOpcode(packet[10]))?;
    Ok((Header { addr: ServerAddr::new(ip, port), op }, &packet[HEADER_LEN..]))
}

struct Cur<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn u8(&mut self, what: &'static str) -> Result<u8, PacketError> {
        let v = *self.b.get(self.pos).ok_or(PacketError::Truncated(what))?;
        self.pos += 1;
        Ok(v)
    }
    fn u16(&mut self, what: &'static str) -> Result<u16, PacketError> {
        let s = self.take(2, what)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self, what: &'static str) -> Result<u32, PacketError> {
        let s = self.take(4, what)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self, what: &'static str) -> Result<i32, PacketError> {
        Ok(self.u32(what)? as i32)
    }
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], PacketError> {
        let end = self.pos.checked_add(n).ok_or(PacketError::Truncated(what))?;
        let s = self.b.get(self.pos..end).ok_or(PacketError::Truncated(what))?;
        self.pos = end;
        Ok(s)
    }
    fn str32(&mut self, what: &'static str, max: usize) -> Result<String, PacketError> {
        let n = self.u32(what)? as usize;
        Ok(truncate_chars(decode_text(self.take(n, what)?), max))
    }
    fn str8(&mut self, what: &'static str, max: usize) -> Result<String, PacketError> {
        let n = self.u8(what)? as usize;
        Ok(truncate_chars(decode_text(self.take(n, what)?), max))
    }
}

fn truncate_chars(mut s: String, max: usize) -> String {
    if let Some((idx, _)) = s.char_indices().nth(max) {
        s.truncate(idx);
    }
    s
}

pub fn decode_info(payload: &[u8]) -> Result<InfoPacket, PacketError> {
    let mut c = Cur::new(payload);
    let password = c.u8("password")? != 0;
    let players = c.u16("players")?;
    let max_players = c.u16("max_players")?;
    let hostname = c.str32("hostname", MAX_HOSTNAME)?;
    let gamemode = c.str32("gamemode", MAX_GAMEMODE)?;
    let language = c.str32("language", MAX_LANGUAGE)?;
    Ok(InfoPacket { password, players, max_players, hostname, gamemode, language })
}

pub fn decode_players(payload: &[u8]) -> Result<Vec<Player>, PacketError> {
    let mut c = Cur::new(payload);
    let count = (c.u16("count")? as usize).min(MAX_PLAYERS);
    let mut out = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        let name = c.str8("player name", MAX_PLAYER_NAME)?;
        let score = c.i32("player score")?;
        out.push(Player { name, score });
    }
    Ok(out)
}

pub fn decode_rules(payload: &[u8]) -> Result<Vec<(String, String)>, PacketError> {
    let mut c = Cur::new(payload);
    let count = (c.u16("count")? as usize).min(MAX_RULES);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name = c.str8("rule name", MAX_RULE_NAME)?;
        let value = c.str8("rule value", MAX_RULE_VALUE)?;
        out.push((name, value));
    }
    Ok(out)
}

// The extra info opcode is only answered by open.mp servers. Fields are discord invite, light
// banner URL, dark banner URL and logo URL.
pub fn decode_extra(payload: &[u8]) -> Result<ExtraInfo, PacketError> {
    let mut c = Cur::new(payload);
    let discord = c.str32("discord", MAX_DISCORD)?;
    let light_banner = c.str32("light banner", MAX_URL)?;
    let dark_banner = c.str32("dark banner", MAX_URL)?;
    let logo = c.str32("logo", MAX_URL).unwrap_or_default();
    Ok(ExtraInfo { discord, light_banner, dark_banner, logo })
}

pub fn check_ping(payload: &[u8], sent: &[u8; 4]) -> Result<(), PacketError> {
    if payload.len() < 4 {
        return Err(PacketError::Truncated("ping"));
    }
    if &payload[..4] != sent { Err(PacketError::PingMismatch) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn addr() -> ServerAddr {
        ServerAddr::new(Ipv4Addr::new(87, 98, 241, 143), 7777)
    }

    const HDR: &[u8] = b"SAMPWb\xf1\x8fa\x1e";

    #[test]
    fn encodes_request() {
        let p = encode_request(addr(), Opcode::Ping, &[1, 2, 3, 4]);
        assert_eq!(&p[..], b"SAMPWb\xf1\x8fa\x1ep\x01\x02\x03\x04");
        let p = encode_request(addr(), Opcode::Info, &[]);
        assert_eq!(p.len(), 11);
        assert_eq!(p[10], b'i');
    }

    #[test]
    fn splits_header() {
        let mut pkt = HDR.to_vec();
        pkt.push(b'i');
        pkt.extend_from_slice(&[9, 9]);
        let (h, payload) = split_response(&pkt).unwrap();
        assert_eq!(h.addr, addr());
        assert_eq!(h.op, Opcode::Info);
        assert_eq!(payload, &[9, 9]);
        assert_eq!(split_response(b"SAMP"), Err(PacketError::TooShort(4)));
        assert_eq!(split_response(b"XAMPWb\xf1\x8fa\x1ei"), Err(PacketError::BadMagic));
        assert_eq!(split_response(b"SAMPWb\xf1\x8fa\x1ez"), Err(PacketError::UnknownOpcode(b'z')));
    }

    #[test]
    fn golden_info() {
        let payload = b"\x00\x03\x00\x8f\x01\"\x00\x00\x00German Nova-eSports | Version v3.0\x16\x00\x00\x00German RealLife by NeS\x0e\x00\x00\x00Deutsch/German";
        let i = decode_info(payload).unwrap();
        assert_eq!(
            i,
            InfoPacket {
                password: false,
                players: 3,
                max_players: 399,
                hostname: "German Nova-eSports | Version v3.0".into(),
                gamemode: "German RealLife by NeS".into(),
                language: "Deutsch/German".into(),
            }
        );
    }

    #[test]
    fn golden_rules() {
        let payload = b"\x08\x00\x0fallowed_clients\x060.3.DL\x07artwork\x03Yes\x07lagcomp\x02On\x07mapname\x0bSan Andreas\x07version\x0eomp 1.4.0.2783\x07weather\x0210\x06weburl\x0fnova-esports.de\tworldtime\x0512:00";
        let r = decode_rules(payload).unwrap();
        assert_eq!(r.len(), 8);
        assert_eq!(r[0], ("allowed_clients".to_string(), "0.3.DL".to_string()));
        assert_eq!(r[4], ("version".to_string(), "omp 1.4.0.2783".to_string()));
        assert_eq!(r[7], ("worldtime".to_string(), "12:00".to_string()));
    }

    #[test]
    fn golden_players() {
        let payload = b"\x03\x00\x06Abgehn\x14\x00\x00\x00\x04EDDY\"\x00\x00\x00\x06Aromat\r\x00\x00\x00";
        let p = decode_players(payload).unwrap();
        assert_eq!(
            p,
            vec![
                Player { name: "Abgehn".into(), score: 20 },
                Player { name: "EDDY".into(), score: 34 },
                Player { name: "Aromat".into(), score: 13 },
            ]
        );
    }

    #[test]
    fn negative_score() {
        let payload = b"\x01\x00\x03bob\xff\xff\xff\xff";
        let p = decode_players(payload).unwrap();
        assert_eq!(p[0].score, -1);
    }

    #[test]
    fn golden_extra() {
        let payload = b"%\x00\x00\x00https://discord.com/invite/38FXbCf7c48\x00\x00\x00https://cdn.nova-network.one/images/omp-nova-banner.webp8\x00\x00\x00https://cdn.nova-network.one/images/omp-nova-banner.webp\x00\x00\x00\x00";
        let e = decode_extra(payload).unwrap();
        assert_eq!(e.discord, "https://discord.com/invite/38FXbCf7c4");
        assert_eq!(e.light_banner, "https://cdn.nova-network.one/images/omp-nova-banner.webp");
        assert_eq!(e.dark_banner, e.light_banner);
        assert_eq!(e.logo, "");
    }

    #[test]
    fn golden_ping() {
        assert_eq!(check_ping(b"\x01\x02\x03\x04", &[1, 2, 3, 4]), Ok(()));
        assert_eq!(check_ping(b"\x01\x02\x03", &[1, 2, 3, 4]), Err(PacketError::Truncated("ping")));
        assert_eq!(check_ping(b"\x09\x02\x03\x04", &[1, 2, 3, 4]), Err(PacketError::PingMismatch));
    }

    #[test]
    fn truncated_inputs_do_not_panic() {
        let full = b"\x00\x03\x00\x8f\x01\x05\x00\x00\x00Hello\x02\x00\x00\x00gm\x02\x00\x00\x00en";
        for n in 0..full.len() {
            assert!(decode_info(&full[..n]).is_err(), "prefix {n} should fail");
        }
        assert!(decode_info(full).is_ok());
        assert!(decode_players(b"\x02\x00\x03bob").is_err());
        assert!(decode_rules(b"\x01\x00\x03abc").is_err());
        assert!(decode_extra(b"\x01").is_err());
        assert!(decode_info(b"\x00\x00\x00\x00\x00\xff\xff\xff\xff").is_err());
    }

    #[test]
    fn oversized_strings_are_truncated() {
        let long = "x".repeat(200);
        let mut payload = vec![0u8, 1, 0, 2, 0];
        payload.extend_from_slice(&(long.len() as u32).to_le_bytes());
        payload.extend_from_slice(long.as_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        let i = decode_info(&payload).unwrap();
        assert_eq!(i.hostname.len(), MAX_HOSTNAME);
        let mut players = vec![1u8, 0, 40];
        players.extend_from_slice(&[b'n'; 40]);
        players.extend_from_slice(&5i32.to_le_bytes());
        let p = decode_players(&players).unwrap();
        assert_eq!(p[0].name.len(), MAX_PLAYER_NAME);
        let mut rules = vec![0xff, 0xff];
        for _ in 0..MAX_RULES {
            rules.extend_from_slice(b"\x01a\x01b");
        }
        assert_eq!(decode_rules(&rules).unwrap().len(), MAX_RULES);
    }

    #[test]
    fn non_utf8_strings_decode() {
        // "Русский", cp1251
        let host = [0xD0u8, 0xF3, 0xF1, 0xF1, 0xEA, 0xE8, 0xE9];
        let mut payload = vec![0u8, 0, 0, 0, 0];
        payload.extend_from_slice(&(host.len() as u32).to_le_bytes());
        payload.extend_from_slice(&host);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(decode_info(&payload).unwrap().hostname, "Русский");
    }
}
