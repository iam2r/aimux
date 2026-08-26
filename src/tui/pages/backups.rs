use crate::backup::{self, BackupEntry};
use crate::paths::Paths;

pub fn load(paths: &Paths) -> Vec<BackupEntry> {
    backup::list(paths).unwrap_or_default()
}

pub fn row(entry: &BackupEntry) -> String {
    let kind = if entry.timestamp {
        crate::i18n::t("ui.timestamp")
    } else {
        crate::i18n::t("ui.named")
    };
    format!("{:<20} {kind}", entry.name)
}
