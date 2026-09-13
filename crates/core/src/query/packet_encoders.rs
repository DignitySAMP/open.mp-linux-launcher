use super::packet::{InfoPacket, Opcode, encode_request};
use crate::model::{ExtraInfo, Player, ServerAddr};

pub fn encode_response(addr: ServerAddr, op: Opcode, payload: &[u8]) -> Vec<u8> {
    encode_request(addr, op, payload)
}

fn push_str32(v: &mut Vec<u8>, s: &[u8]) {
    v.extend_from_slice(&(s.len() as u32).to_le_bytes());
    v.extend_from_slice(s);
}

fn push_str8(v: &mut Vec<u8>, s: &[u8]) {
    let s = &s[..s.len().min(255)];
    v.push(s.len() as u8);
    v.extend_from_slice(s);
}

pub fn encode_info(i: &InfoPacket) -> Vec<u8> {
    let mut v = vec![u8::from(i.password)];
    v.extend_from_slice(&i.players.to_le_bytes());
    v.extend_from_slice(&i.max_players.to_le_bytes());
    push_str32(&mut v, i.hostname.as_bytes());
    push_str32(&mut v, i.gamemode.as_bytes());
    push_str32(&mut v, i.language.as_bytes());
    v
}

pub fn encode_players(players: &[Player]) -> Vec<u8> {
    let mut v = (players.len() as u16).to_le_bytes().to_vec();
    for p in players {
        push_str8(&mut v, p.name.as_bytes());
        v.extend_from_slice(&p.score.to_le_bytes());
    }
    v
}

pub fn encode_rules(rules: &[(String, String)]) -> Vec<u8> {
    let mut v = (rules.len() as u16).to_le_bytes().to_vec();
    for (k, val) in rules {
        push_str8(&mut v, k.as_bytes());
        push_str8(&mut v, val.as_bytes());
    }
    v
}

pub fn encode_extra(e: &ExtraInfo) -> Vec<u8> {
    let mut v = Vec::new();
    push_str32(&mut v, e.discord.as_bytes());
    push_str32(&mut v, e.light_banner.as_bytes());
    push_str32(&mut v, e.dark_banner.as_bytes());
    push_str32(&mut v, e.logo.as_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::super::packet::{decode_extra, decode_info, decode_players, decode_rules};
    use super::*;

    #[test]
    fn info_roundtrip() {
        let i = InfoPacket {
            password: true,
            players: 12,
            max_players: 500,
            hostname: "Test".into(),
            gamemode: "Freeroam".into(),
            language: "English".into(),
        };
        assert_eq!(decode_info(&encode_info(&i)).unwrap(), i);
    }

    #[test]
    fn players_rules_extra_roundtrip() {
        let p = vec![Player { name: "a".into(), score: -5 }, Player { name: "b".into(), score: 7 }];
        assert_eq!(decode_players(&encode_players(&p)).unwrap(), p);
        let r = vec![("k".to_string(), "v".to_string())];
        assert_eq!(decode_rules(&encode_rules(&r)).unwrap(), r);
        let e = ExtraInfo { discord: "d".into(), light_banner: "l".into(), dark_banner: "k".into(), logo: "g".into() };
        assert_eq!(decode_extra(&encode_extra(&e)).unwrap(), e);
    }
}
