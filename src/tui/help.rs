use crate::i18n::{t, tf};

use super::app::{App, Overlay, Page};
use super::pages::form::FormKind;

pub fn text(app: &App) -> String {
    if matches!(app.overlay, Overlay::Syncing | Overlay::FetchingModels) {
        return t("help.syncing").into();
    }
    if matches!(
        app.overlay,
        Overlay::ConfirmDelete { .. } | Overlay::ConfirmRestore { .. }
    ) {
        return t("help.confirm").into();
    }
    if let Overlay::Form(form) = &app.overlay {
        return form_help(form.kind);
    }
    match &app.overlay {
        Overlay::ModelPicker(_) => return t("help.picker").into(),
        Overlay::CatalogEditor { .. } => return t("help.catalog").into(),
        Overlay::SlotEditor { .. } => return t("help.slots").into(),
        Overlay::SnippetEditor(_) => return t("help.snippet").into(),
        _ => {}
    }
    match app.page {
        Page::Providers => page_sheet(super::keymap::KeyMode::List, None),
        Page::Data => page_sheet(super::keymap::KeyMode::Data, Some(t("help.data_footnote"))),
        Page::Settings => page_sheet(
            super::keymap::KeyMode::Settings,
            Some(t("help.settings_footnote")),
        ),
    }
}

/// Build a page's key sheet from the shared hint table (single source of
/// truth with the status bar and the dispatcher). Rows may declare a group;
/// a header line is emitted when the group changes.
fn page_sheet(mode: super::keymap::KeyMode, footnote: Option<&str>) -> String {
    let mut out = format!("{}\n\n", t("help.keys_title"));
    let mut last_group: Option<Option<&str>> = None;
    for (display, label, group) in super::keymap::hint_rows(mode) {
        if let Some(group) = group.filter(|g| last_group.is_none_or(|prev| prev != Some(g))) {
            out.push_str(group);
            out.push_str(":\n");
        }
        last_group = Some(group);
        let pad = " ".repeat(21usize.saturating_sub(display.chars().count()));
        out.push_str(&format!("{display}{pad}{label}\n"));
    }
    if let Some(foot) = footnote {
        if !foot.is_empty() {
            out.push('\n');
            out.push_str(foot);
            out.push('\n');
        }
    }
    out
}

fn form_help(kind: FormKind) -> String {
    match kind {
        FormKind::SyncSetup => t("help.sync_setup").into(),
        FormKind::Add { .. } | FormKind::Edit { .. } => {
            let keep = if matches!(kind, FormKind::Edit { .. }) {
                t("help.form_keep")
            } else {
                ""
            };
            tf("help.form", &[keep])
        }
    }
}

#[cfg(test)]
mod sheet_tests {
    use super::*;

    #[test]
    fn sheets_render_with_groups() {
        let data = page_sheet(
            super::super::keymap::KeyMode::Data,
            Some(t("help.data_footnote")),
        );
        assert!(data.starts_with("Keys\n\n"), "{data}"); // default test lang is en
        assert!(data.contains("Backups:\n"), "{data}");
        assert!(data.contains("Sync:\n"), "{data}");
        assert!(data.contains("b                    snapshot"), "{data}");
        assert!(data.contains("apmux-sync"), "{data}");
        // group header appears once
        assert_eq!(data.matches("Backups:").count(), 1);
    }

    #[test]
    fn list_sheet_has_no_groups() {
        let list = page_sheet(super::super::keymap::KeyMode::List, None);
        assert!(list.starts_with("Keys"), "{list}");
        // the only colon-free sheet: no group headers on Providers
        assert!(
            !list.lines().any(|l| l.ends_with(':')),
            "unexpected group in {list}"
        );
        assert!(list.contains("[ ] / Tab"), "{list}");
        assert!(list.contains("q                    quit"), "{list}");
    }
}
