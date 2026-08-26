//! Compact Dracula-inspired palette (same family as cc-switch TUI).

use ratatui::style::{Color, Modifier, Style};

use crate::store::AppId;

const CYAN: Color = Color::Rgb(139, 233, 253);
const GREEN: Color = Color::Rgb(80, 250, 123);
const ORANGE: Color = Color::Rgb(255, 184, 108);
const PINK: Color = Color::Rgb(255, 121, 198);
const YELLOW: Color = Color::Rgb(241, 250, 140);
const RED: Color = Color::Rgb(255, 85, 85);
const COMMENT: Color = Color::Rgb(98, 114, 164);
const FG: Color = Color::Rgb(248, 248, 242);

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub dim: Color,
    pub fg: Color,
    pub no_color: bool,
}

impl Theme {
    pub fn for_app(app: AppId) -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let accent = match app {
            AppId::Claude => CYAN,
            AppId::Codex => GREEN,
            AppId::OpenCode => ORANGE,
            AppId::Pi => PINK,
        };
        Self {
            accent,
            ok: GREEN,
            warn: YELLOW,
            err: RED,
            dim: COMMENT,
            fg: FG,
            no_color,
        }
    }

    pub fn fg(&self, color: Color) -> Style {
        if self.no_color {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }

    pub fn accent(&self) -> Style {
        self.fg(self.accent)
    }

    pub fn selected(&self) -> Style {
        if self.no_color {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .fg(Color::Black)
                .bg(self.accent)
                .add_modifier(Modifier::BOLD)
        }
    }

    pub fn current_mark(&self) -> Style {
        self.fg(self.ok).add_modifier(Modifier::BOLD)
    }

    pub fn flash(&self, text: &str) -> Style {
        let lower = text.to_ascii_lowercase();
        if lower.contains("fail") || lower.contains("error") || text.contains("失败") {
            self.fg(self.err)
        } else if lower.contains("skip")
            || lower.contains("not initialized")
            || text.contains("未初始化")
        {
            self.fg(self.warn)
        } else {
            self.fg(self.ok)
        }
    }
}
