use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::i18n::{t, tf};
use crate::mask;

use super::app::{App, Overlay, Page};
use super::edit;
use super::help;
use super::pages::form;
use super::pages::models::{self, CatalogEditor, ModelPicker, PickerKind, SlotEditor};
use super::pages::{backups, settings as settings_page, sync};
use super::theme::Theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let theme = Theme::for_app(app.current_app());
    match app.page {
        Page::Providers => {
            draw_tabs(frame, app, chunks[0], theme);
            draw_list(frame, app, chunks[1], theme);
        }
        Page::Backups => {
            draw_title(frame, chunks[0], t("ui.backups"), theme);
            draw_backups(frame, app, chunks[1], theme);
        }
        Page::Sync => {
            draw_title(frame, chunks[0], t("ui.sync"), theme);
            draw_sync(frame, app, chunks[1], theme);
        }
        Page::Settings => {
            draw_title(frame, chunks[0], t("ui.settings"), theme);
            draw_settings(frame, app, chunks[1], theme);
        }
    }
    draw_status(frame, app, chunks[2], theme);
    match &mut app.overlay {
        Overlay::Form(form) => draw_form(frame, form, frame.area(), theme),
        Overlay::ConfirmDelete { id, name } => draw_confirm(
            frame,
            frame.area(),
            t("ui.delete"),
            &tf("confirm.delete", &[id, name]),
            theme,
        ),
        Overlay::ConfirmRestore { name } => draw_confirm(
            frame,
            frame.area(),
            t("ui.restore"),
            &tf("confirm.restore", &[name]),
            theme,
        ),
        Overlay::Syncing => draw_busy(frame, frame.area(), t("ui.sync"), sync::syncing(), theme),
        Overlay::FetchingModels => draw_busy(
            frame,
            frame.area(),
            t("ui.models"),
            t("ui.fetching_models"),
            theme,
        ),
        Overlay::ModelPicker(picker) => draw_picker(frame, picker, frame.area(), theme),
        Overlay::CatalogEditor { editor, .. } => draw_catalog(frame, editor, frame.area(), theme),
        Overlay::SlotEditor { editor, .. } => draw_slots(frame, editor, frame.area(), theme),
        Overlay::SnippetEditor(page) => draw_snippet(frame, page, frame.area(), theme),
        Overlay::None => {}
    }
    if app.help {
        draw_help(frame, app, frame.area(), theme);
    }
}

fn draw_title(frame: &mut Frame, area: Rect, title: &str, theme: Theme) {
    frame.render_widget(
        Block::bordered()
            .border_style(theme.accent())
            .title(Span::styled(
                title,
                theme.accent().add_modifier(Modifier::BOLD),
            )),
        area,
    );
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let titles = app.tab_titles();
    let tabs = Tabs::new(titles)
        .select(app.app_idx)
        .block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(t("ui.apps"), theme.accent())),
        )
        .style(theme.fg(theme.dim))
        .highlight_style(theme.selected());
    frame.render_widget(tabs, area);
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let current = app.current_id().map(str::to_string);
    let items: Vec<ListItem> = app
        .providers()
        .into_iter()
        .map(|p| {
            let is_current = current.as_deref() == Some(p.id.as_str());
            let mark = if is_current { " *" } else { "  " };
            let secret = if p.official {
                t("list.official").to_string()
            } else {
                mask::mask_key(&p.api_key)
            };
            let line = Line::from(vec![
                Span::raw(format!("{:<24} ", p.name)),
                Span::styled(secret, theme.fg(theme.dim)),
                Span::styled(
                    mark,
                    if is_current {
                        theme.current_mark()
                    } else {
                        Style::default()
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();
    let title = tf("ui.providers_title", &[adapter_label(app)]);
    let list = List::new(items)
        .block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(title, theme.accent())),
        )
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn adapter_label(app: &App) -> &'static str {
    crate::adapter::get(app.current_app())
        .map(|a| a.display_name())
        .unwrap_or("?")
}

fn draw_backups(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let items: Vec<ListItem> = app
        .backups
        .iter()
        .map(|e| ListItem::new(backups::row(e)))
        .collect();
    let list = List::new(items)
        .block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(t("ui.timestamp_named"), theme.accent())),
        )
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.backup_state);
}

fn draw_sync(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let body = match &app.sync_local {
        None => t("ui.webdav_unconfigured").to_string(),
        Some(s) => {
            let last = if s.last_sync_at.is_empty() {
                "-"
            } else {
                s.last_sync_at.as_str()
            };
            format!(
                "url: {}\nnamespace: {}\nusername: {}\nlast_pulled: {}\nlast_pushed: {}\nlast_sync_at: {last}",
                crate::webdav::redact_url(&s.url),
                crate::webdav::NAMESPACE,
                s.username,
                s.last_pulled_sha256,
                s.last_pushed_sha256
            )
        }
    };
    frame.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .style(theme.fg(theme.fg))
            .block(
                Block::bordered()
                    .border_style(theme.accent())
                    .title(Span::styled(t("ui.status"), theme.accent())),
            ),
        area,
    );
}

fn draw_settings(frame: &mut Frame, app: &mut App, area: Rect, theme: Theme) {
    let detected = app.detected_apps();
    let apps = crate::settings::all_apps();
    let mut items = vec![
        setting_item(t("settings.language"), settings_page::lang_value(), theme),
        setting_item(
            t("settings.apps_mode"),
            settings_page::mode_value(&app.settings),
            theme,
        ),
    ];
    for app_id in &apps {
        let name = crate::adapter::get(*app_id)
            .map(|a| a.display_name())
            .unwrap_or("?");
        let value = settings_page::app_value(&app.settings, detected.contains(app_id), *app_id);
        items.push(setting_item(name, &value, theme));
    }
    let list = List::new(items)
        .block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(t("ui.settings"), theme.accent())),
        )
        .highlight_style(theme.selected())
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.settings_state);
}

fn setting_item(label: &str, value: &str, theme: Theme) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw(format!("{label:<16} ")),
        Span::styled(value.to_string(), theme.fg(theme.ok)),
    ]))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let hint = app.hint();
    let flash = if app.status.is_empty() {
        Line::from("")
    } else {
        Line::from(Span::styled(app.status.as_str(), theme.flash(&app.status)))
    };
    let p = Paragraph::new(flash).wrap(Wrap { trim: true }).block(
        Block::bordered()
            .border_style(theme.accent())
            .title(Span::styled(hint, theme.fg(theme.dim))),
    );
    frame.render_widget(p, area);
}

fn draw_form(frame: &mut Frame, form: &form::Form, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();
    for (i, f) in form.fields.iter().enumerate() {
        let mark = if i == form.focus && !f.readonly {
            ">"
        } else {
            " "
        };
        let req = if f.required { "*" } else { " " };
        let text = format!(
            "{mark}{req}{}: {}",
            crate::i18n::t(f.label),
            form::display_value(f, i == form.focus)
        );
        let style = if f.readonly {
            theme.fg(theme.dim)
        } else {
            theme.fg(theme.fg)
        };
        lines.push(Line::from(Span::styled(text, style)));
    }
    if let Some(err) = &form.error {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("! {err}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(t("ui.form_hint")));
    let h = (lines.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(6);
    let w = 64.min(area.width.saturating_sub(4)).max(24);
    let popup = centered(area, w, h);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(form.title(), theme.accent())),
        ),
        popup,
    );
}

fn draw_confirm(frame: &mut Frame, area: Rect, title: &str, msg: &str, theme: Theme) {
    let text = format!("{msg}\n\n{}", t("ui.confirm_hint"));
    let popup = centered(area, 52.min(area.width.saturating_sub(4)).max(24), 7);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_style(theme.fg(theme.warn))
                .title(Span::styled(title, theme.fg(theme.warn))),
        ),
        popup,
    );
}

fn draw_busy(frame: &mut Frame, area: Rect, title: &str, body: &str, theme: Theme) {
    let popup = centered(area, 28.min(area.width.saturating_sub(4)).max(12), 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(title, theme.accent())),
        ),
        popup,
    );
}

fn draw_picker(frame: &mut Frame, picker: &mut ModelPicker, area: Rect, theme: Theme) {
    let w = 72.min(area.width.saturating_sub(4)).max(24);
    let h = 16.min(area.height.saturating_sub(2)).max(6);
    let popup = centered(area, w, h);
    // Viewport = popup inner minus filter/error rows reserved below.
    let inner_h = popup.height.saturating_sub(2) as usize;
    let reserve = usize::from(picker.filtering || !picker.filter.is_empty())
        + 2 * usize::from(picker.error.is_some());
    picker.page_rows = inner_h.saturating_sub(reserve).max(1);
    picker.ensure_visible();
    let vis = picker.visible();
    let start = picker.scroll.min(vis.len());
    let end = (start + picker.page_rows).min(vis.len());
    let mut lines: Vec<Line> = Vec::new();
    if picker.filtering || !picker.filter.is_empty() {
        lines.push(Line::from(format!(
            "/ {}",
            edit::with_caret(&picker.filter, picker.filter_cursor, picker.filtering)
        )));
    }
    for &idx in &vis[start..end] {
        let cur = if idx == picker.cursor { ">" } else { " " };
        let id = &picker.ids[idx];
        let text = match picker.kind {
            PickerKind::Catalog => {
                let mark = if picker.selected.contains(&idx) {
                    "[x]"
                } else {
                    "[ ]"
                };
                format!("{cur}{mark} {id}")
            }
            PickerKind::Slot { .. } => format!("{cur} {id}"),
        };
        lines.push(Line::from(text));
    }
    if let Some(err) = &picker.error {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("! {err}")));
    }
    let title = match vis.iter().position(|&i| i == picker.cursor) {
        Some(pos) => format!("{} {}/{}", t("ui.model_picker"), pos + 1, vis.len()),
        None => t("ui.model_picker").to_string(),
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(title, theme.accent())),
        ),
        popup,
    );
}

fn draw_catalog(frame: &mut Frame, editor: &CatalogEditor, area: Rect, theme: Theme) {
    let mut header = String::from("  ");
    for field in editor.fields {
        header.push_str(&format!("{:<16}", models::field_label(*field)));
    }
    let mut lines = vec![Line::from(header)];
    for (i, row) in editor.rows.iter().enumerate() {
        let star = if i == editor.default_idx { "*" } else { " " };
        let cur = if i == editor.row { ">" } else { " " };
        let mut line = format!("{cur}{star}");
        for (c, field) in editor.fields.iter().enumerate() {
            let raw = if editor.editing && i == editor.row && c == editor.col {
                edit::with_caret(&editor.buf, editor.buf_cursor, true)
            } else {
                match field {
                    crate::adapter::models::CatalogField::Id => row.id.clone(),
                    crate::adapter::models::CatalogField::Label => {
                        row.label.clone().unwrap_or_default()
                    }
                    crate::adapter::models::CatalogField::ContextWindow => row
                        .context_window
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    crate::adapter::models::CatalogField::MaxTokens => {
                        row.max_tokens.map(|n| n.to_string()).unwrap_or_default()
                    }
                }
            };
            let cell = if i == editor.row && c == editor.col && !editor.editing {
                format!("[{:<14}]", raw)
            } else {
                format!("{raw:<16}")
            };
            line.push_str(&cell);
        }
        lines.push(Line::from(line));
    }
    popup_lines(frame, area, t("ui.catalog"), lines, theme, 18);
}

fn draw_slots(frame: &mut Frame, editor: &SlotEditor, area: Rect, theme: Theme) {
    let mut lines = Vec::new();
    for i in 0..editor.row_count() {
        let cur = if i == editor.row { ">" } else { " " };
        let (label, value) = if i == 0 {
            (t("slot.default"), editor.default_model.as_str())
        } else if let Some(s) = editor.slots.get(i - 1) {
            (
                t(s.label),
                editor.values.get(s.key).map(String::as_str).unwrap_or(""),
            )
        } else {
            ("", "")
        };
        let shown = if editor.editing && i == editor.row {
            edit::with_caret(&editor.buf, editor.buf_cursor, true)
        } else {
            value.to_string()
        };
        lines.push(Line::from(format!("{cur}{label:<12} {shown}")));
    }
    popup_lines(frame, area, t("ui.slots"), lines, theme, 14);
}

fn draw_snippet(
    frame: &mut Frame,
    page: &super::pages::quick::SnippetPage,
    area: Rect,
    theme: Theme,
) {
    let snippet = page
        .parsed_snippet()
        .unwrap_or_else(|_| serde_json::json!({}));
    let items = page.items();
    let mut lines: Vec<Line> = Vec::new();
    if !items.is_empty() {
        lines.push(Line::from(Span::styled(
            t("quick.builtin"),
            theme.fg(theme.dim),
        )));
        for (i, item) in items.iter().enumerate() {
            let cur = if page.cursor == i { ">" } else { " " };
            let mark = if page.item_on(item, &snippet) {
                "[x]"
            } else {
                "[ ]"
            };
            lines.push(Line::from(format!("{cur}{mark} {}", t(item.label))));
        }
        lines.push(Line::from(""));
    }
    let json_cur = if page.json_focused() { ">" } else { " " };
    lines.push(Line::from(Span::styled(
        format!("{json_cur}{}", t(page.body_label())),
        theme.fg(theme.dim),
    )));
    let body = edit::with_caret(&page.text, page.text_cursor, page.json_focused());
    for line in body.lines() {
        lines.push(Line::from(line.to_string()));
    }
    if let Some(err) = &page.error {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("! {err}")));
    }
    popup_lines(frame, area, t("ui.snippet"), lines, theme, 16);
}

fn popup_lines(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line>,
    theme: Theme,
    min_h: u16,
) {
    let h = (lines.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(min_h.min(area.height.saturating_sub(2)).max(6));
    let w = 72.min(area.width.saturating_sub(4)).max(24);
    let popup = centered(area, w, h);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_style(theme.accent())
                .title(Span::styled(title, theme.accent())),
        ),
        popup,
    );
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let text = help::text(app);
    let lines = text.lines().count() as u16;
    let w = 52.min(area.width.saturating_sub(4)).max(24);
    let h = (lines + 2).min(area.height.saturating_sub(2)).max(8);
    let popup = centered(area, w, h);
    frame.render_widget(Clear, popup);
    let help = Paragraph::new(text).wrap(Wrap { trim: false }).block(
        Block::bordered()
            .border_style(theme.accent())
            .title(Span::styled(t("ui.help"), theme.accent())),
    );
    frame.render_widget(help, popup);
}

pub(crate) fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AppId;
    use ratatui::{backend::TestBackend, Terminal};

    fn row(term: &Terminal<TestBackend>, y: u16) -> String {
        let buf = term.backend().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn picker_draws_one_page_with_filter_caret() {
        let mut picker = ModelPicker::with_preselect(
            PickerKind::Catalog,
            (0..30).map(|i| format!("m{i:02}")).collect(),
            &[],
        );
        picker.filter = "m0".into();
        picker.filter_cursor = 1;
        picker.filtering = true;
        let mut term = Terminal::new(TestBackend::new(40, 10)).unwrap();
        term.draw(|f| draw_picker(f, &mut picker, f.area(), Theme::for_app(AppId::Claude)))
            .unwrap();
        assert_eq!(picker.page_rows, 5); // popup inner 6 - filter row
        let joined: String = (0..10)
            .map(|y| row(&term, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("/ m_0")); // caret sits mid-filter
        assert!(joined.contains("[ ] m00"));
        assert!(!joined.contains("m05")); // second page stays hidden
        assert!(joined.contains("1/10")); // title position
    }
}
