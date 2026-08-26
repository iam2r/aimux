use crate::i18n::t;
use crate::settings::{AppsMode, Settings};
use crate::store::AppId;

pub const ROW_LANG: usize = 0;
pub const ROW_MODE: usize = 1;
pub const ROW_APPS_START: usize = 2;

pub fn row_count() -> usize {
    ROW_APPS_START + crate::adapter::registry().len()
}

pub fn lang_value() -> &'static str {
    match crate::i18n::lang() {
        crate::i18n::Lang::En => "English",
        crate::i18n::Lang::Zh => "中文",
    }
}

pub fn mode_value(settings: &Settings) -> &'static str {
    match settings.apps_mode {
        AppsMode::Auto => t("settings.mode_auto"),
        AppsMode::Manual => t("settings.mode_manual"),
    }
}

pub fn app_value(settings: &Settings, detected: bool, app: AppId) -> String {
    match settings.apps_mode {
        AppsMode::Auto => {
            if detected {
                t("settings.detected").to_string()
            } else {
                t("settings.hidden").to_string()
            }
        }
        AppsMode::Manual => {
            if settings.is_manually_visible(app) {
                t("settings.on").to_string()
            } else {
                t("settings.off").to_string()
            }
        }
    }
}
