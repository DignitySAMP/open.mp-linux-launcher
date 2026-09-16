use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if self.popup.is_some() {
            return;
        }
        let at = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(kind) = self.hit.tabs.iter().find(|(r, _)| r.contains(at)).map(|(_, k)| *k) {
                    self.search_editing = false;
                    self.set_tab(kind);
                } else if self.hit.search.contains(at) {
                    self.search_editing = true;
                } else if self.hit.rows.contains(at) {
                    let row = self.table.offset() + usize::from(m.row - self.hit.rows.y);
                    if row >= self.view.len() {
                        return;
                    }
                    let again = self.last_click.is_some_and(|(r, t)| r == row && t.elapsed() < DOUBLE_CLICK);
                    self.last_click = Some((row, Instant::now()));
                    self.search_editing = false;
                    self.select_row(Some(row));
                    if again {
                        self.open_join();
                    }
                }
            }
            MouseEventKind::ScrollDown if self.hit.list.contains(at) => self.move_selection(1),
            MouseEventKind::ScrollUp if self.hit.list.contains(at) => self.move_selection(-1),
            _ => {}
        }
    }
}
