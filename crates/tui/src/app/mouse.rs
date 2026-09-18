use super::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if self.popup.is_some() {
            self.handle_popup_mouse(m);
            return;
        }
        let at = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.hit.update.contains(at) {
                    self.open_link("release page", Some(omptui_core::update::RELEASE_PAGE.to_string()));
                } else if let Some(kind) = self.hit.tabs.iter().find(|(r, _)| r.contains(at)).map(|(_, k)| *k) {
                    self.search_editing = false;
                    self.set_tab(kind);
                } else if let Some(url) = self.hit.links.iter().find(|(r, _)| r.contains(at)).map(|(_, u)| u.clone()) {
                    self.open_link("link", Some(url));
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

    fn handle_popup_mouse(&mut self, m: MouseEvent) {
        let press = |app: &mut App, code: KeyCode| app.handle_popup_key(KeyEvent::new(code, KeyModifiers::NONE));
        let at = Position::new(m.column, m.row);
        let listy = matches!(self.popup, Some(Popup::Settings(_) | Popup::Filters(_)));
        match m.kind {
            MouseEventKind::ScrollDown if listy => press(self, KeyCode::Down),
            MouseEventKind::ScrollUp if listy => press(self, KeyCode::Up),
            MouseEventKind::Down(MouseButton::Left) => {
                if !self.hit.popup.area.contains(at) {
                    press(self, KeyCode::Esc);
                    return;
                }
                let row = self.hit.popup.rows.iter().find(|(r, _)| r.contains(at)).map(|(_, i)| *i);
                match (&mut self.popup, row) {
                    (Some(Popup::Message { .. } | Popup::Help), _) => press(self, KeyCode::Enter),
                    (Some(Popup::Confirm { .. }), Some(i)) => {
                        press(self, if i == 0 { KeyCode::Enter } else { KeyCode::Esc })
                    }
                    (Some(Popup::Join(form)), Some(i)) => {
                        form.field = i;
                        // enter on a text field joins, only focus it
                        if i > 1 {
                            press(self, KeyCode::Enter);
                        }
                    }
                    (Some(Popup::ServerSettings(form)), Some(i)) => {
                        form.field = i;
                        if i == 2 {
                            press(self, KeyCode::Right);
                        }
                    }
                    (Some(Popup::Filters(form)), Some(i)) if form.editing.is_none() => {
                        let again = form.cursor == i;
                        form.cursor = i;
                        if i != 3 {
                            press(self, KeyCode::Char(' '));
                        } else if again {
                            press(self, KeyCode::Enter);
                        }
                    }
                    (Some(Popup::Settings(form)), Some(i)) if form.editing.is_none() => {
                        let again = form.cursor == i;
                        form.cursor = i;
                        let row = form.row();
                        // text and actions need a second click
                        if again || !(row.is_text() || row.is_action()) {
                            press(self, KeyCode::Enter);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
