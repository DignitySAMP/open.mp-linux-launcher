use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};

pub fn decode_text(bytes: &[u8]) -> String {
    let s = if bytes.is_ascii() {
        String::from_utf8_lossy(bytes).into_owned()
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_owned()
    } else {
        let mut det = EncodingDetector::new(Iso2022JpDetection::Allow);
        det.feed(bytes, true);
        let enc = det.guess(None, Utf8Detection::Allow);
        let (cow, _, _) = enc.decode(bytes);
        cow.into_owned()
    };
    sanitize(&s)
}

pub fn sanitize(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\t').collect::<String>().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough() {
        assert_eq!(decode_text(b"Hello [LS] server"), "Hello [LS] server");
    }

    #[test]
    fn utf8_passthrough() {
        assert_eq!(decode_text("Сервер ✓".as_bytes()), "Сервер ✓");
    }

    #[test]
    fn cp1251_detected() {
        // "Русский сервер", cp1251
        let bytes = [0xD0, 0xF3, 0xF1, 0xF1, 0xEA, 0xE8, 0xE9, 0x20, 0xF1, 0xE5, 0xF0, 0xE2, 0xE5, 0xF0];
        assert_eq!(decode_text(&bytes), "Русский сервер");
    }

    #[test]
    fn cp1252_detected() {
        // "Café • Bar", cp1252
        let bytes = [b'C', b'a', b'f', 0xE9, b' ', 0x95, b' ', b'B', b'a', b'r'];
        assert_eq!(decode_text(&bytes), "Café • Bar");
    }

    #[test]
    fn strips_control_chars() {
        assert_eq!(decode_text(b"ab\x01c\x1b[31md\n"), "abc[31md");
    }
}
