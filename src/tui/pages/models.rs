//! Model picker, catalog editor, slot table, and snippet editor overlays.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver};

use crossterm::event::{KeyCode, KeyEvent};

use crate::adapter::models::{self, CatalogField, ModelUi};
use crate::i18n::t;
use crate::store::{AppId, ModelEntry};
use crate::tui::edit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Catalog,
    Slot { key: &'static str },
}

pub struct ModelPicker {
    pub kind: PickerKind,
    pub ids: Vec<String>,
    pub selected: BTreeSet<usize>,
    /// Known catalog metadata by id, carried through fetch so confirming
    /// keeps label/context_window/max_tokens of previously saved rows.
    pub meta: std::collections::BTreeMap<String, ModelEntry>,
    pub cursor: usize,
    pub filter: String,
    pub filter_cursor: usize,
    pub filtering: bool,
    pub error: Option<String>,
    /// First visible row in `visible()`, updated while drawing.
    pub scroll: usize,
    /// Rows that fit in the list viewport. 0 until the first draw.
    pub page_rows: usize,
}

impl ModelPicker {
    /// Pre-check catalog ids already chosen; for slots, park the cursor on
    /// the first id that equals the slot's current value.
    pub fn with_preselect(kind: PickerKind, ids: Vec<String>, pre: &[String]) -> Self {
        let mut p = Self {
            kind,
            ids,
            selected: BTreeSet::new(),
            meta: Default::default(),
            cursor: 0,
            filter: String::new(),
            filter_cursor: 0,
            filtering: false,
            error: None,
            scroll: 0,
            page_rows: 0,
        };
        let mut placed_cursor = false;
        for (i, id) in p.ids.iter().enumerate() {
            if !pre.iter().any(|s| s == id) {
                continue;
            }
            match p.kind {
                PickerKind::Catalog => {
                    p.selected.insert(i);
                }
                PickerKind::Slot { .. } if !placed_cursor => {
                    p.cursor = i;
                    placed_cursor = true;
                }
                _ => {}
            }
        }
        p
    }

    /// Remember catalog metadata (from the held form) so confirming a
    /// fetched list keeps label/context_window/max_tokens of saved rows.
    pub fn with_meta(mut self, entries: &[ModelEntry]) -> Self {
        self.meta = entries
            .iter()
            .filter(|e| !e.id.trim().is_empty())
            .map(|e| (e.id.clone(), e.clone()))
            .collect();
        self
    }

    pub fn visible(&self) -> Vec<usize> {
        self.ids
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                self.filter.is_empty()
                    || id
                        .to_ascii_lowercase()
                        .contains(&self.filter.to_ascii_lowercase())
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PickerCmd {
        if self.filtering {
            return self.handle_filter_key(key);
        }
        match key.code {
            KeyCode::Esc => PickerCmd::Cancel,
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter_cursor = edit::len(&self.filter);
                PickerCmd::Continue
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_vis(1);
                PickerCmd::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_vis(-1);
                PickerCmd::Continue
            }
            KeyCode::PageDown => {
                self.page(1);
                PickerCmd::Continue
            }
            KeyCode::PageUp => {
                self.page(-1);
                PickerCmd::Continue
            }
            KeyCode::Home => {
                self.jump_vis(0);
                PickerCmd::Continue
            }
            KeyCode::End => {
                self.jump_vis(isize::MAX);
                PickerCmd::Continue
            }
            KeyCode::Char(' ') if matches!(self.kind, PickerKind::Catalog) => {
                if self.selected.contains(&self.cursor) {
                    self.selected.remove(&self.cursor);
                } else {
                    self.selected.insert(self.cursor);
                }
                PickerCmd::Continue
            }
            KeyCode::Enter => match self.kind {
                PickerKind::Catalog => PickerCmd::ConfirmCatalog(
                    self.selected
                        .iter()
                        .filter_map(|i| self.ids.get(*i).cloned())
                        .map(|id| {
                            let mut entry = self.meta.get(&id).cloned().unwrap_or_default();
                            entry.id = id;
                            entry
                        })
                        .collect(),
                ),
                PickerKind::Slot { key } => {
                    let id = self.ids.get(self.cursor).cloned().unwrap_or_default();
                    PickerCmd::ConfirmSlot { key, id }
                }
            },
            _ => PickerCmd::Continue,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> PickerCmd {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.filtering = false;
            return PickerCmd::Continue;
        }
        if edit::key(&mut self.filter, &mut self.filter_cursor, key) {
            self.after_filter();
        } else {
            match key.code {
                KeyCode::Up => self.move_vis(-1),
                KeyCode::Down => self.move_vis(1),
                KeyCode::PageUp => self.page(-1),
                KeyCode::PageDown => self.page(1),
                _ => {}
            }
        }
        PickerCmd::Continue
    }

    fn after_filter(&mut self) {
        let vis = self.visible();
        if vis.is_empty() {
            self.scroll = 0;
            return;
        }
        if !vis.contains(&self.cursor) {
            self.cursor = vis[0];
        }
        self.ensure_visible();
    }

    fn move_vis(&mut self, delta: isize) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let pos = vis.iter().position(|i| *i == self.cursor).unwrap_or(0);
        let n = vis.len() as isize;
        let next = vis[((pos as isize + delta).rem_euclid(n)) as usize];
        self.cursor = next;
        self.ensure_visible();
    }

    fn page(&mut self, dir: isize) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let pos = vis.iter().position(|i| *i == self.cursor).unwrap_or(0) as isize;
        let n = vis.len() as isize;
        // 0 until the first draw; then jump by the real viewport (no wrap).
        let rows = if self.page_rows == 0 {
            10
        } else {
            self.page_rows
        };
        let step = (rows.min(n.max(1) as usize) as isize) * dir;
        let next = (pos + step).clamp(0, n - 1) as usize;
        self.cursor = vis[next];
        self.ensure_visible();
    }

    fn jump_vis(&mut self, pos: isize) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let i = if pos <= 0 {
            0
        } else {
            vis.len().saturating_sub(1)
        };
        self.cursor = vis[i];
        self.ensure_visible();
    }

    pub fn ensure_visible(&mut self) {
        let vis = self.visible();
        if vis.is_empty() {
            self.scroll = 0;
            return;
        }
        let pos = vis.iter().position(|&i| i == self.cursor).unwrap_or(0);
        let page = self.page_rows.max(1);
        if pos < self.scroll {
            self.scroll = pos;
        } else if pos >= self.scroll.saturating_add(page) {
            self.scroll = pos + 1 - page;
        }
        let max_scroll = vis.len().saturating_sub(page.min(vis.len()));
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }
}

#[derive(Debug)]
pub enum PickerCmd {
    Continue,
    Cancel,
    ConfirmCatalog(Vec<ModelEntry>),
    ConfirmSlot { key: &'static str, id: String },
}

pub struct CatalogEditor {
    pub fields: &'static [CatalogField],
    pub rows: Vec<ModelEntry>,
    pub default_idx: usize,
    pub row: usize,
    pub col: usize,
    pub editing: bool,
    pub buf: String,
    pub buf_cursor: usize,
}

impl CatalogEditor {
    pub fn new(
        fields: &'static [CatalogField],
        rows: Vec<ModelEntry>,
        default_id: Option<&str>,
    ) -> Self {
        let default_idx = default_id
            .and_then(|id| rows.iter().position(|r| r.id == id))
            .unwrap_or(0);
        Self {
            fields,
            rows,
            default_idx,
            row: 0,
            col: 0,
            editing: false,
            buf: String::new(),
            buf_cursor: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> CatalogCmd {
        if self.editing {
            match key.code {
                KeyCode::Esc => {
                    self.editing = false;
                }
                KeyCode::Enter => self.commit_edit(),
                _ => {
                    edit::key(&mut self.buf, &mut self.buf_cursor, key);
                }
            }
            CatalogCmd::Continue
        } else {
            match key.code {
                KeyCode::Esc => CatalogCmd::Cancel,
                KeyCode::Enter if key.modifiers.is_empty() => CatalogCmd::Save,
                KeyCode::Char('j') | KeyCode::Down => {
                    if !self.rows.is_empty() {
                        self.row = (self.row + 1) % self.rows.len();
                    }
                    CatalogCmd::Continue
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if !self.rows.is_empty() {
                        self.row = (self.row + self.rows.len() - 1) % self.rows.len();
                    }
                    CatalogCmd::Continue
                }
                KeyCode::Tab | KeyCode::Right => {
                    if !self.fields.is_empty() {
                        self.col = (self.col + 1) % self.fields.len();
                    }
                    CatalogCmd::Continue
                }
                KeyCode::BackTab | KeyCode::Left => {
                    if !self.fields.is_empty() {
                        self.col = (self.col + self.fields.len() - 1) % self.fields.len();
                    }
                    CatalogCmd::Continue
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    self.begin_edit();
                    CatalogCmd::Continue
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    if self.row < self.rows.len() {
                        self.rows.remove(self.row);
                        if self.default_idx >= self.rows.len() && !self.rows.is_empty() {
                            self.default_idx = self.rows.len() - 1;
                        }
                        self.row = self.row.min(self.rows.len().saturating_sub(1));
                    }
                    CatalogCmd::Continue
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.rows.push(ModelEntry::default());
                    self.row = self.rows.len() - 1;
                    CatalogCmd::Continue
                }
                KeyCode::Char('*') => {
                    if self.row < self.rows.len() {
                        self.default_idx = self.row;
                    }
                    CatalogCmd::Continue
                }
                _ => CatalogCmd::Continue,
            }
        }
    }

    fn begin_edit(&mut self) {
        let Some(row) = self.rows.get(self.row) else {
            return;
        };
        let Some(field) = self.fields.get(self.col) else {
            return;
        };
        self.buf_cursor = 0;
        self.buf = match field {
            CatalogField::Id => row.id.clone(),
            CatalogField::Label => row.label.clone().unwrap_or_default(),
            CatalogField::ContextWindow => row
                .context_window
                .map(|n| n.to_string())
                .unwrap_or_default(),
            CatalogField::MaxTokens => row.max_tokens.map(|n| n.to_string()).unwrap_or_default(),
        };
        self.buf_cursor = edit::len(&self.buf);
        self.editing = true;
    }

    fn commit_edit(&mut self) {
        self.editing = false;
        let Some(row) = self.rows.get_mut(self.row) else {
            return;
        };
        let Some(field) = self.fields.get(self.col) else {
            return;
        };
        let v = self.buf.trim();
        match field {
            CatalogField::Id => row.id = v.to_string(),
            CatalogField::Label => {
                row.label = if v.is_empty() {
                    None
                } else {
                    Some(v.to_string())
                };
            }
            CatalogField::ContextWindow => {
                row.context_window = v.parse().ok();
            }
            CatalogField::MaxTokens => {
                row.max_tokens = v.parse().ok();
            }
        }
    }

    pub fn default_id(&self) -> Option<String> {
        self.rows
            .get(self.default_idx)
            .map(|r| r.id.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.rows
                    .iter()
                    .map(|r| r.id.trim())
                    .find(|s| !s.is_empty())
                    .map(str::to_string)
            })
    }
}

pub enum CatalogCmd {
    Continue,
    Cancel,
    Save,
}

pub struct SlotEditor {
    pub slots: &'static [models::SlotSpec],
    pub values: std::collections::BTreeMap<String, String>,
    pub default_model: String,
    pub row: usize, // 0 = default, then slots
    pub editing: bool,
    pub buf: String,
    pub buf_cursor: usize,
}

impl SlotEditor {
    pub fn from_values(
        default_model: String,
        values: std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            slots: models::CLAUDE_SLOTS,
            values,
            default_model,
            row: 0,
            editing: false,
            buf: String::new(),
            buf_cursor: 0,
        }
    }

    pub fn row_count(&self) -> usize {
        1 + self.slots.len()
    }

    pub fn focused_slot_key(&self) -> Option<&'static str> {
        if self.row == 0 {
            None
        } else {
            self.slots.get(self.row - 1).map(|s| s.key)
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SlotCmd {
        if self.editing {
            match key.code {
                KeyCode::Esc => {
                    self.editing = false;
                }
                KeyCode::Enter => self.commit(),
                _ => {
                    edit::key(&mut self.buf, &mut self.buf_cursor, key);
                }
            }
            SlotCmd::Continue
        } else {
            match key.code {
                KeyCode::Esc => SlotCmd::Cancel,
                KeyCode::Enter => SlotCmd::Save,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.row = (self.row + 1) % self.row_count();
                    SlotCmd::Continue
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.row = (self.row + self.row_count() - 1) % self.row_count();
                    SlotCmd::Continue
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    self.buf = self.current_value().to_string();
                    self.buf_cursor = edit::len(&self.buf);
                    self.editing = true;
                    SlotCmd::Continue
                }
                KeyCode::Char(' ') => SlotCmd::Fetch,
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    let id = self.current_value().to_string();
                    if !id.is_empty() {
                        self.default_model = id.clone();
                        for s in self.slots {
                            self.values.insert(s.key.to_string(), id.clone());
                        }
                    }
                    SlotCmd::Continue
                }
                _ => SlotCmd::Continue,
            }
        }
    }

    fn current_value(&self) -> &str {
        if self.row == 0 {
            &self.default_model
        } else if let Some(s) = self.slots.get(self.row - 1) {
            self.values.get(s.key).map(String::as_str).unwrap_or("")
        } else {
            ""
        }
    }

    fn commit(&mut self) {
        self.editing = false;
        let v = self.buf.trim().to_string();
        if self.row == 0 {
            self.default_model = v;
        } else if let Some(s) = self.slots.get(self.row - 1) {
            if v.is_empty() {
                self.values.remove(s.key);
            } else {
                self.values.insert(s.key.to_string(), v);
            }
        }
    }
}

pub enum SlotCmd {
    Continue,
    Cancel,
    Save,
    Fetch,
}

pub fn spawn_fetch(
    base_url: String,
    api_key: String,
    protocol: Option<String>,
) -> Receiver<Result<Vec<String>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let res = models::fetch_models(&base_url, &api_key, protocol.as_deref())
            .map_err(|e| format!("{e:#}"));
        let _ = tx.send(res);
    });
    rx
}

pub fn model_ui_for(app: AppId) -> ModelUi {
    crate::adapter::get(app)
        .map(|a| a.model_ui())
        .unwrap_or(ModelUi::Catalog {
            fields: models::OPENCODE_FIELDS,
        })
}

pub fn field_label(field: CatalogField) -> &'static str {
    match field {
        CatalogField::Id => "id",
        CatalogField::Label => t("field.label"),
        CatalogField::ContextWindow => t("field.context_window"),
        CatalogField::MaxTokens => t("field.max_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Provider;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn catalog_picker_toggles_and_confirms() {
        let mut p =
            ModelPicker::with_preselect(PickerKind::Catalog, vec!["a".into(), "b".into()], &[]);
        p.handle_key(key(KeyCode::Char(' ')));
        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char(' ')));
        match p.handle_key(key(KeyCode::Enter)) {
            PickerCmd::ConfirmCatalog(rows) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].id, "a");
                assert_eq!(rows[1].id, "b");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn slot_picker_assigns_one() {
        let mut p = ModelPicker::with_preselect(
            PickerKind::Slot { key: "haiku" },
            vec!["x".into(), "y".into()],
            &[],
        );
        p.handle_key(key(KeyCode::Char('j')));
        match p.handle_key(key(KeyCode::Enter)) {
            PickerCmd::ConfirmSlot { key, id } => {
                assert_eq!(key, "haiku");
                assert_eq!(id, "y");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn slot_copy_all() {
        let mut p = Provider::blank(AppId::Claude);
        p.model = Some("sonnet".into());
        let mut ed = SlotEditor::from_values(p.model.clone().unwrap_or_default(), p.slots.clone());
        ed.row = 0;
        ed.handle_key(key(KeyCode::Char('a')));
        assert_eq!(ed.default_model, "sonnet");
        assert_eq!(ed.values.get("haiku").map(String::as_str), Some("sonnet"));
        assert_eq!(ed.values.get("opus").map(String::as_str), Some("sonnet"));
        assert_eq!(
            ed.values.get("subagent").map(String::as_str),
            Some("sonnet")
        );
    }

    #[test]
    fn picker_preselects_held_ids() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut p = ModelPicker::with_preselect(
            PickerKind::Catalog,
            ids.clone(),
            &["c".into(), "gone".into()],
        );
        assert_eq!(p.selected, BTreeSet::from([2])); // unknown ids ignored
        match p.handle_key(key(KeyCode::Enter)) {
            PickerCmd::ConfirmCatalog(rows) => assert_eq!(rows[0].id, "c"),
            other => panic!("{other:?}"),
        }
        let s =
            ModelPicker::with_preselect(PickerKind::Slot { key: "opus" }, ids, &["b".to_string()]);
        assert_eq!(s.cursor, 1); // cursor parked on the slot's current id
    }

    #[test]
    fn picker_confirm_keeps_saved_catalog_metadata() {
        // Re-picking from a fetched list must not wipe context windows
        // (or labels) that were already saved on those ids.
        let saved = ModelEntry {
            id: "b".into(),
            label: Some("B".into()),
            context_window: Some(200_000),
            max_tokens: None,
        };
        let mut p = ModelPicker::with_preselect(
            PickerKind::Catalog,
            vec!["a".into(), "b".into()],
            &["b".into()],
        )
        .with_meta(&[saved]);
        match p.handle_key(key(KeyCode::Enter)) {
            PickerCmd::ConfirmCatalog(rows) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].id, "b");
                assert_eq!(rows[0].context_window, Some(200_000));
                assert_eq!(rows[0].label.as_deref(), Some("B"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picker_pages_without_wrap() {
        let ids: Vec<String> = (0..12).map(|i| format!("m{i:02}")).collect();
        let mut p = ModelPicker::with_preselect(PickerKind::Catalog, ids, &[]);
        p.page_rows = 5;
        p.handle_key(key(KeyCode::PageDown));
        assert_eq!(p.cursor, 5); // jumped by one page
        assert!(p.scroll > 0); // scroll follows the cursor
        p.handle_key(key(KeyCode::PageDown));
        assert_eq!(p.cursor, 10); // clamped near the end, no wrap
        p.handle_key(key(KeyCode::PageUp));
        assert_eq!(p.cursor, 5);
    }

    #[test]
    fn filter_caret_edits_middle() {
        let mut p = ModelPicker::with_preselect(
            PickerKind::Slot { key: "default" },
            vec!["a".into(), "b".into()],
            &[],
        );
        p.handle_key(key(KeyCode::Char('/')));
        assert!(p.filtering);
        for c in ['a', 'b', 'c'] {
            p.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(p.filter, "abc");
        p.handle_key(key(KeyCode::Left));
        p.handle_key(key(KeyCode::Left));
        assert_eq!(p.filter_cursor, 1);
        p.handle_key(key(KeyCode::Char('X')));
        assert_eq!(p.filter, "aXbc"); // inserted before 'b'
        assert!(p.visible().is_empty());
        p.handle_key(key(KeyCode::Backspace));
        assert_eq!(p.filter, "abc");
        assert_eq!(edit::with_caret(&p.filter, p.filter_cursor, true), "a_bc");
    }

    #[test]
    fn catalog_edit_and_default() {
        let mut ed = CatalogEditor::new(
            crate::adapter::models::OPENCODE_FIELDS,
            vec![ModelEntry {
                id: "gpt-4o".into(),
                ..ModelEntry::default()
            }],
            Some("gpt-4o"),
        );
        ed.handle_key(key(KeyCode::Char('n')));
        assert_eq!(ed.rows.len(), 2);
        ed.handle_key(key(KeyCode::Char('*')));
        assert_eq!(ed.default_idx, 1);
        ed.handle_key(key(KeyCode::Char('d')));
        assert_eq!(ed.rows.len(), 1);
        assert_eq!(ed.default_id().as_deref(), Some("gpt-4o"));
    }
}
