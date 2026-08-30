//! Snippet editor: built-in quick-config checkboxes are children of the JSON body.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::adapter::quick::QuickItem;
use crate::adapter::{parse_snippet, render_snippet, SnippetSyntax};
use crate::store::AppId;
use crate::tui::edit;

pub struct SnippetPage {
    pub app: AppId,
    pub syntax: SnippetSyntax,
    pub text: String,
    pub extras: BTreeMap<String, String>,
    /// 0..items.len()-1 = checkbox; items.len() = JSON body.
    pub cursor: usize,
    pub text_cursor: usize,
    pub error: Option<String>,
}

pub enum SnippetCmd {
    Continue,
    Cancel,
    Save,
    Toggle,
}

impl SnippetPage {
    pub fn open(
        app: AppId,
        snippet: Option<&serde_json::Value>,
        extras: BTreeMap<String, String>,
    ) -> Self {
        let syntax = crate::adapter::get(app)
            .map(|a| a.snippet_syntax())
            .unwrap_or(SnippetSyntax::Json);
        let mut page = Self {
            app,
            syntax,
            text: match snippet {
                Some(v) if !crate::store::is_empty_snippet(v) => render_snippet(syntax, v),
                _ => String::new(),
            },
            extras,
            cursor: 0,
            text_cursor: 0,
            error: None,
        };
        if page.items().is_empty() {
            page.cursor = 0;
        }
        page.text_cursor = edit::len(&page.text);
        page
    }

    /// i18n key for the editor body row.
    pub fn body_label(&self) -> &'static str {
        match self.syntax {
            SnippetSyntax::Json => "quick.json",
            SnippetSyntax::Toml => "quick.toml",
        }
    }

    pub fn items(&self) -> Vec<QuickItem> {
        crate::adapter::get(self.app)
            .map(|a| a.quick_items())
            .unwrap_or(&[])
            .to_vec()
    }

    pub fn json_row(&self) -> usize {
        self.items().len()
    }

    pub fn json_focused(&self) -> bool {
        self.items().is_empty() || self.cursor >= self.json_row()
    }

    pub fn focused_item(&self) -> Option<QuickItem> {
        if self.json_focused() {
            None
        } else {
            self.items().get(self.cursor).copied()
        }
    }

    pub fn parsed_snippet(&self) -> Result<serde_json::Value, String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Ok(serde_json::json!({}));
        }
        parse_snippet(self.syntax, trimmed)
    }

    pub fn set_snippet(&mut self, value: &serde_json::Value) {
        if value.as_object().is_some_and(|o| o.is_empty()) {
            self.text.clear();
        } else {
            self.text = render_snippet(self.syntax, value);
            if matches!(self.syntax, SnippetSyntax::Json) && !self.text.ends_with('\n') {
                self.text.push('\n');
            }
        }
        self.text_cursor = edit::len(&self.text);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SnippetCmd {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(
                key.code,
                KeyCode::Enter | KeyCode::Char('s') | KeyCode::Char('S')
            )
        {
            return SnippetCmd::Save;
        }
        if self.json_focused() {
            return self.handle_json_key(key);
        }
        match key.code {
            KeyCode::Esc => SnippetCmd::Cancel,
            KeyCode::Tab => {
                self.cursor = self.json_row();
                SnippetCmd::Continue
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(self.json_row());
                SnippetCmd::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                SnippetCmd::Continue
            }
            KeyCode::Char(' ') | KeyCode::Enter => SnippetCmd::Toggle,
            _ => SnippetCmd::Continue,
        }
    }

    fn handle_json_key(&mut self, key: KeyEvent) -> SnippetCmd {
        if key.code == KeyCode::Esc {
            return SnippetCmd::Cancel;
        }
        if !self.items().is_empty() && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.cursor = 0;
            return SnippetCmd::Continue;
        }
        if !edit::key(&mut self.text, &mut self.text_cursor, key) && key.code == KeyCode::Enter {
            edit::insert(&mut self.text, &mut self.text_cursor, '\n');
        }
        SnippetCmd::Continue
    }

    /// Paste into the JSON text area, keeping newlines (bracketed paste).
    pub fn paste_json(&mut self, text: &str) {
        if self.json_focused() {
            edit::paste_multiline(&mut self.text, &mut self.text_cursor, text);
        }
    }

    pub fn item_on(&self, item: &QuickItem, snippet: &serde_json::Value) -> bool {
        if item.extra_key.is_some() {
            item.extra_on(&self.extras)
        } else {
            item.snippet_on(snippet)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AppId;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn claude_checkboxes_then_json() {
        let mut page = SnippetPage::open(AppId::Claude, None, Default::default());
        // 5 originals + unknown_model_reactive = 6.
        assert_eq!(page.items().len(), 6);
        assert_eq!(page.syntax, SnippetSyntax::Json);
        assert!(!page.json_focused());
        page.handle_key(key(KeyCode::Tab));
        assert!(page.json_focused());
        page.handle_key(key(KeyCode::Char('{')));
        assert_eq!(page.text, "{");
    }

    #[test]
    fn codex_edits_toml_stores_json() {
        use crate::store::AppId as A;
        // Store value arrives as JSON; Codex renders it as config.toml tables.
        let stored = serde_json::json!({"features": {"goals": true}});
        let page = SnippetPage::open(A::Codex, Some(&stored), Default::default());
        assert_eq!(page.syntax, SnippetSyntax::Toml);
        assert_eq!(page.text, "[features]\ngoals = true\n");
        assert_eq!(page.body_label(), "quick.toml");
        // Round-trip: TOML body parses back into the same JSON SSOT value.
        assert_eq!(page.parsed_snippet().unwrap(), stored);
    }

    #[test]
    fn codex_toggle_composes_toml_fragment() {
        use crate::store::AppId as A;
        let mut page = SnippetPage::open(A::Codex, None, Default::default());
        assert_eq!(page.text, ""); // empty snippet renders blank
                                   // Pin the item under test so list ordering doesn't affect this test.
        let pos = page
            .items()
            .iter()
            .position(|i| i.id == "goal_mode")
            .expect("codex goal_mode quick item");
        page.cursor = pos;
        let item = page.focused_item().unwrap();
        let mut snip = page.parsed_snippet().unwrap();
        item.apply_snippet(&mut snip);
        page.set_snippet(&snip);
        assert_eq!(page.text, "[features]\ngoals = true\n");
        assert!(item.snippet_on(&page.parsed_snippet().unwrap()));
    }
}
