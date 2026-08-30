//! Character-index caret for TUI text fields.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

pub fn len(s: &str) -> usize {
    s.chars().count()
}

pub fn clamp(s: &str, cursor: usize) -> usize {
    cursor.min(len(s))
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

pub fn insert(s: &mut String, cursor: &mut usize, c: char) -> bool {
    *cursor = clamp(s, *cursor);
    s.insert(byte(s, *cursor), c);
    *cursor += 1;
    true
}

/// Insert a bracketed-paste payload at the cursor, dropping control
/// characters (clipboard newlines/tabs have no meaning in single-line
/// fields). True if anything was inserted.
pub fn paste(s: &mut String, cursor: &mut usize, text: &str) -> bool {
    paste_with(s, cursor, text, false)
}

/// Like `paste` but keeps newlines — for multi-line editors (snippet JSON).
pub fn paste_multiline(s: &mut String, cursor: &mut usize, text: &str) -> bool {
    paste_with(s, cursor, text, true)
}

fn paste_with(s: &mut String, cursor: &mut usize, text: &str, keep_newlines: bool) -> bool {
    *cursor = clamp(s, *cursor);
    let clean: String = text
        .chars()
        .filter(|c| *c == '\n' && keep_newlines || !c.is_control())
        .collect();
    if clean.is_empty() {
        return false;
    }
    let at = byte(s, *cursor);
    s.insert_str(at, &clean);
    *cursor += clean.chars().count();
    true
}

pub fn backspace(s: &mut String, cursor: &mut usize) -> bool {
    *cursor = clamp(s, *cursor);
    if *cursor == 0 {
        return false;
    }
    *cursor -= 1;
    let start = byte(s, *cursor);
    let end = byte(s, *cursor + 1);
    s.replace_range(start..end, "");
    true
}

pub fn delete(s: &mut String, cursor: &mut usize) -> bool {
    *cursor = clamp(s, *cursor);
    if *cursor >= len(s) {
        return false;
    }
    let start = byte(s, *cursor);
    let end = byte(s, *cursor + 1);
    s.replace_range(start..end, "");
    true
}

pub fn left(cursor: &mut usize) -> bool {
    *cursor = cursor.saturating_sub(1);
    true
}

pub fn right(s: &str, cursor: &mut usize) -> bool {
    *cursor = clamp(s, *cursor);
    let moved = *cursor < len(s);
    if moved {
        *cursor += 1;
    }
    moved
}

pub fn home(cursor: &mut usize) -> bool {
    *cursor = 0;
    true
}

pub fn end(s: &str, cursor: &mut usize) -> bool {
    *cursor = len(s);
    true
}

/// Handle text-editing keys (chars/arrows/backspace/delete); true if consumed.
pub fn key(s: &mut String, cursor: &mut usize, k: KeyEvent) -> bool {
    match k.code {
        KeyCode::Backspace => backspace(s, cursor),
        KeyCode::Delete => delete(s, cursor),
        KeyCode::Left => left(cursor),
        KeyCode::Right => right(s, cursor),
        KeyCode::Home => home(cursor),
        KeyCode::End => end(s, cursor),
        KeyCode::Char(c) => insert(s, cursor, c),
        _ => false,
    }
}

/// Split text at the cursor so the character under it can carry an
/// underline style — the caret marks a position instead of occupying one.
/// Past-the-end cursors render as an underlined space (an insertion point,
/// not a logical character).
pub(crate) fn caret_spans(text: &str, cursor: usize, style: Style) -> Vec<Span<'static>> {
    let c = clamp(text, cursor);
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::with_capacity(3);
    if c > 0 {
        let pre: String = chars[..c].iter().collect();
        spans.push(Span::styled(pre, style));
    }
    match chars.get(c) {
        Some(&ch) => {
            spans.push(Span::styled(
                ch.to_string(),
                style.add_modifier(Modifier::UNDERLINED),
            ));
            if c + 1 < chars.len() {
                let post: String = chars[c + 1..].iter().collect();
                spans.push(Span::styled(post, style));
            }
        }
        None => spans.push(Span::styled(" ", style.add_modifier(Modifier::UNDERLINED))),
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_arrows_edit_the_middle() {
        let mut s = String::from("ac");
        let mut c = 1;
        insert(&mut s, &mut c, 'b');
        assert_eq!(s, "abc");
        assert_eq!(c, 2);
        left(&mut c);
        left(&mut c);
        insert(&mut s, &mut c, 'X');
        assert_eq!(s, "Xabc");
        end(&s, &mut c);
        backspace(&mut s, &mut c);
        assert_eq!(s, "Xab");
        home(&mut c);
        delete(&mut s, &mut c);
        assert_eq!(s, "ab");
        // underline caret marks a position without adding characters
        assert_eq!(
            caret_spans("ab", 1, Style::default())
                .iter()
                .map(|sp| sp.content.as_ref())
                .collect::<Vec<_>>()
                .join(""),
            "ab"
        );
        // end-of-text cursor renders the insertion point as one underlined space
        let end_spans = caret_spans("ab", 2, Style::default());
        assert_eq!(end_spans.len(), 2);
        assert_eq!(end_spans[1].content, " ");
        assert_eq!(
            end_spans[1].style.add_modifier,
            ratatui::style::Modifier::UNDERLINED
        );
    }

    #[test]
    fn paste_inserts_in_order_and_skips_control_chars() {
        let mut s = String::from("ad");
        let mut c = 1;
        assert!(paste(&mut s, &mut c, "bc"));
        assert_eq!(s, "abcd");
        assert_eq!(c, 3);
        // clipboard newlines/tabs must not leak into single-line fields
        c = s.chars().count();
        assert!(paste(&mut s, &mut c, "https://x\n\t"));
        assert_eq!(s, "abcdhttps://x");
        // mid-string paste keeps order and advances the caret
        let mut s = String::from("你好");
        let mut c = 1;
        assert!(paste_multiline(&mut s, &mut c, "啊\n啊"));
        assert_eq!(s, "你啊\n啊好");
        assert_eq!(c, 4);
        // nothing but control chars → nothing inserted
        let mut s = String::new();
        let mut c = 0;
        assert!(!paste(&mut s, &mut c, "\r\n"));
        assert_eq!(s, "");
    }

    #[test]
    fn unicode_counts_chars() {
        let mut s = String::from("你好");
        let mut c = 1;
        insert(&mut s, &mut c, '啊');
        assert_eq!(s, "你啊好");
        assert_eq!(c, 2);
        backspace(&mut s, &mut c);
        assert_eq!(s, "你好");
    }
}
