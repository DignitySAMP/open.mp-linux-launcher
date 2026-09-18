use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    value: String,
    cursor: usize,
    pub masked: bool,
}

impl Input {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor, masked: false }
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set(&mut self, v: impl Into<String>) {
        self.value = v.into();
        self.cursor = self.value.chars().count();
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn display(&self) -> String {
        if self.masked { "•".repeat(self.value.chars().count()) } else { self.value.clone() }
    }

    // ret the visible part of the value and the cursor column in it
    pub fn window(&self, width: usize) -> (String, usize) {
        let mut chars: Vec<char> = self.display().chars().collect();
        if width == 0 || chars.len() < width {
            return (chars.into_iter().collect(), self.cursor);
        }
        let start = (self.cursor + 1).saturating_sub(width);
        let col = self.cursor - start;
        let cut_right = chars.len() > start + width;
        chars = chars.into_iter().skip(start).take(width).collect();
        if start > 0 {
            chars[0] = '…';
        }
        if cut_right && col + 1 < width {
            chars[width - 1] = '…';
        }
        (chars.into_iter().collect(), col)
    }

    fn byte_at(&self, idx: usize) -> usize {
        self.value.char_indices().nth(idx).map(|(b, _)| b).unwrap_or(self.value.len())
    }

    pub fn handle(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                let mut idx = self.cursor;
                let chars: Vec<char> = self.value.chars().collect();
                while idx > 0 && chars[idx - 1].is_whitespace() {
                    idx -= 1;
                }
                while idx > 0 && !chars[idx - 1].is_whitespace() {
                    idx -= 1;
                }
                let (a, b) = (self.byte_at(idx), self.byte_at(self.cursor));
                self.value.replace_range(a..b, "");
                self.cursor = idx;
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) | (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::Char('e'), KeyModifiers::CONTROL) | (KeyCode::End, _) => self.cursor = self.value.chars().count(),
            (KeyCode::Char(c), m) if m.is_empty() || m == KeyModifiers::SHIFT => {
                let b = self.byte_at(self.cursor);
                self.value.insert(b, c);
                self.cursor += 1;
            }
            (KeyCode::Backspace, _) => {
                if self.cursor > 0 {
                    let (a, b) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
                    self.value.replace_range(a..b, "");
                    self.cursor -= 1;
                }
            }
            (KeyCode::Delete, _) => {
                if self.cursor < self.value.chars().count() {
                    let (a, b) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
                    self.value.replace_range(a..b, "");
                }
            }
            (KeyCode::Left, _) => self.cursor = self.cursor.saturating_sub(1),
            (KeyCode::Right, _) => self.cursor = (self.cursor + 1).min(self.value.chars().count()),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn editing() {
        let mut i = Input::new("ab");
        assert!(i.handle(k(KeyCode::Char('c'))));
        assert_eq!(i.value(), "abc");
        i.handle(k(KeyCode::Left));
        i.handle(k(KeyCode::Char('X')));
        assert_eq!(i.value(), "abXc");
        i.handle(k(KeyCode::Backspace));
        assert_eq!(i.value(), "abc");
        i.handle(k(KeyCode::Home));
        i.handle(k(KeyCode::Delete));
        assert_eq!(i.value(), "bc");
        i.handle(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(i.value(), "");
        i.set("hello world");
        i.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(i.value(), "hello ");
        assert!(!i.handle(k(KeyCode::Enter)));
        let m = Input::new("pw").masked();
        assert_eq!(m.display(), "••");
        let long = Input::new("/usr/share/steam/compatibilitytools.d/proton/files/bin/wine");
        assert_eq!(long.window(80), (long.value().to_owned(), 59));
        assert_eq!(long.window(16), ("…files/bin/wine".to_owned(), 15));
        let mut home = long.clone();
        home.handle(k(KeyCode::Home));
        assert_eq!(home.window(16), ("/usr/share/stea…".to_owned(), 0));
        let mut u = Input::new("é");
        u.handle(k(KeyCode::Char('x')));
        assert_eq!(u.value(), "éx");
    }
}
