//! Character-index caret for TUI text fields.

use crossterm::event::{KeyCode, KeyEvent};

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

/// Insert a `_` caret at the character index.
pub fn with_caret(s: &str, cursor: usize, show: bool) -> String {
    if !show {
        return s.to_string();
    }
    let c = clamp(s, cursor);
    let mut out = String::with_capacity(s.len() + 1);
    for (i, ch) in s.chars().enumerate() {
        if i == c {
            out.push('_');
        }
        out.push(ch);
    }
    if c == len(s) {
        out.push('_');
    }
    out
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
        assert_eq!(with_caret("ab", 1, true), "a_b");
        assert_eq!(with_caret("ab", 2, true), "ab_");
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
