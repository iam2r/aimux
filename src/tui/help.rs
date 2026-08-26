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
        Page::Providers => t("help.list").into(),
        Page::Backups => t("help.backups").into(),
        Page::Sync => t("help.sync").into(),
        Page::Settings => t("help.settings").into(),
    }
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
