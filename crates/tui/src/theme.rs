// Colours follow the open.mp purple, see https://open.mp.

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(0x8b, 0x5c, 0xf6);
pub const ACCENT_DIM: Color = Color::Rgb(0x6d, 0x45, 0xc4);
pub const ACCENT_LIGHT: Color = Color::Rgb(0xc4, 0xb5, 0xfd);
pub const FG: Color = Color::Rgb(0xe6, 0xe1, 0xf5);
pub const DIM: Color = Color::Rgb(0x8a, 0x86, 0x9a);
pub const BG_SEL: Color = Color::Rgb(0x2e, 0x22, 0x4d);
pub const OK: Color = Color::Rgb(0x4a, 0xde, 0x80);
pub const WARN: Color = Color::Rgb(0xfb, 0xbf, 0x24);
pub const ERR: Color = Color::Rgb(0xf8, 0x71, 0x71);
pub const OMP: Color = Color::Rgb(0xa7, 0x8b, 0xfa);
pub const SAMP: Color = Color::Rgb(0xf9, 0x73, 0x16);

pub fn title() -> Style {
    Style::default().fg(ACCENT_LIGHT).add_modifier(Modifier::BOLD)
}
pub fn border() -> Style {
    Style::default().fg(ACCENT_DIM)
}
pub fn border_focus() -> Style {
    Style::default().fg(ACCENT)
}
pub fn header() -> Style {
    Style::default().fg(ACCENT_LIGHT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}
pub fn text() -> Style {
    Style::default().fg(FG)
}
pub fn dim() -> Style {
    Style::default().fg(DIM)
}
pub fn selected() -> Style {
    Style::default().bg(BG_SEL).fg(FG).add_modifier(Modifier::BOLD)
}
pub fn key() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
pub fn ok() -> Style {
    Style::default().fg(OK)
}
pub fn warn() -> Style {
    Style::default().fg(WARN)
}
pub fn err() -> Style {
    Style::default().fg(ERR).add_modifier(Modifier::BOLD)
}
pub fn tab_active() -> Style {
    Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
}
pub fn tab_inactive() -> Style {
    Style::default().fg(ACCENT_LIGHT)
}

// Same ping thresholds as the official launcher.
pub fn ping_style(ping: Option<u32>) -> Style {
    match ping {
        None => dim(),
        Some(p) if p >= omptui_core::UNREACHABLE_PING => Style::default().fg(DIM),
        Some(p) if p < 80 => ok(),
        Some(p) if p < 160 => warn(),
        Some(_) => Style::default().fg(ERR),
    }
}
