use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    List,
    Data,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SpeedTest,
    TryLaunch,
    Quit,
    ToggleHelp,
    CloseOverlay,
    NextApp,
    PrevApp,
    Up,
    Down,
    Switch,
    Add,
    Edit,
    Delete,
    Backup,
    OpenData,
    OpenSettings,
    ToggleSetting,
    Back,
    Restore,
    SyncTab,
    SyncPush,
    SyncPull,
    SyncSetup,
    None,
}

pub fn map_key(key: KeyEvent, mode: KeyMode) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
        KeyCode::Char('?') => Action::ToggleHelp,
        KeyCode::Esc => {
            if mode == KeyMode::Help {
                Action::CloseOverlay
            } else if matches!(mode, KeyMode::Data | KeyMode::Settings) {
                Action::Back
            } else {
                Action::Quit
            }
        }
        _ if mode == KeyMode::Help => Action::None,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Enter => match mode {
            KeyMode::List => Action::Switch,
            KeyMode::Data => Action::Restore,
            KeyMode::Settings => Action::ToggleSetting,
            _ => Action::None,
        },
        KeyCode::Char('a') | KeyCode::Char('A') if mode == KeyMode::List => Action::Add,
        KeyCode::Char('t') | KeyCode::Char('T') if mode == KeyMode::List => Action::SpeedTest,
        KeyCode::Char('o') | KeyCode::Char('O') if mode == KeyMode::List => Action::TryLaunch,
        KeyCode::Char('e') | KeyCode::Char('E') => match mode {
            KeyMode::List => Action::Edit,
            KeyMode::Data => Action::SyncSetup,
            _ => Action::None,
        },
        KeyCode::Char('d') | KeyCode::Char('D') if mode == KeyMode::List => Action::Delete,
        KeyCode::Char('b') | KeyCode::Char('B') if mode == KeyMode::Data => Action::Backup,
        // Data page: Tab switches the Sync panel between WebDAV and Gist.
        KeyCode::Tab | KeyCode::BackTab if mode == KeyMode::Data => Action::SyncTab,
        KeyCode::Char('r') | KeyCode::Char('R') => Action::OpenData,
        KeyCode::Char('s') | KeyCode::Char('S')
            if mode != KeyMode::Data && mode != KeyMode::Settings =>
        {
            Action::OpenData
        }
        KeyCode::Char('g') | KeyCode::Char('G') if mode != KeyMode::Settings => {
            Action::OpenSettings
        }
        KeyCode::Char(' ') if mode == KeyMode::Settings => Action::ToggleSetting,
        KeyCode::Char('p') | KeyCode::Char('P') if mode == KeyMode::Data => Action::SyncPush,
        KeyCode::Char('u') | KeyCode::Char('U') if mode == KeyMode::Data => Action::SyncPull,
        KeyCode::Char(']') | KeyCode::Char('】') | KeyCode::Char('］') | KeyCode::Tab
            if mode == KeyMode::List =>
        {
            Action::NextApp
        }
        KeyCode::Char('[') | KeyCode::Char('【') | KeyCode::Char('［') | KeyCode::BackTab
            if mode == KeyMode::List =>
        {
            Action::PrevApp
        }
        _ => Action::None,
    }
}

/// One row of a page's key vocabulary: what the user sees (`display`), the
/// i18n key describing it, and — for keys that flow through [`map_key`] —
/// the action the dispatcher must return. The status-bar hint and the help
/// sheet are generated from this table, so shown hints cannot drift from
/// real handlers; `tests::hints_match_dispatcher` locks every check in.
pub(crate) struct HintRow {
    pub display: &'static str,
    pub label: &'static str,
    /// Optional section header (help sheet groups rows under it).
    pub group: Option<&'static str>,
    pub mode: KeyMode,
    /// (key, expected map_key outcome) pairs verified by tests. Only read in
    /// test builds; production uses display/label/group for rendering.
    #[cfg_attr(not(test), expect(dead_code))]
    pub checks: &'static [(KeyCode, Action)],
}

const HINTS: &[HintRow] = &[
    // ---- Providers list -------------------------------------------------
    HintRow {
        display: "[ ] / Tab",
        label: "hint.switch_app",
        group: None,
        mode: KeyMode::List,
        checks: &[
            (KeyCode::Char(']'), Action::NextApp),
            (KeyCode::Char('['), Action::PrevApp),
        ],
    },
    HintRow {
        display: "j/k or ↑↓",
        label: "hint.move",
        group: None,
        mode: KeyMode::List,
        checks: &[
            (KeyCode::Char('j'), Action::Down),
            (KeyCode::Up, Action::Up),
        ],
    },
    HintRow {
        display: "Enter",
        label: "hint.use",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Enter, Action::Switch)],
    },
    HintRow {
        display: "a",
        label: "hint.add",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('a'), Action::Add)],
    },
    HintRow {
        display: "e",
        label: "hint.edit",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('e'), Action::Edit)],
    },
    HintRow {
        display: "d",
        label: "hint.delete",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('d'), Action::Delete)],
    },
    HintRow {
        display: "r/s",
        label: "hint.data",
        group: None,
        mode: KeyMode::List,
        checks: &[
            (KeyCode::Char('r'), Action::OpenData),
            (KeyCode::Char('s'), Action::OpenData),
        ],
    },
    HintRow {
        display: "g",
        label: "hint.settings",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('g'), Action::OpenSettings)],
    },
    HintRow {
        display: "t",
        label: "hint.speed_test",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('t'), Action::SpeedTest)],
    },
    HintRow {
        display: "o",
        label: "hint.try",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('o'), Action::TryLaunch)],
    },
    HintRow {
        display: "?",
        label: "hint.help",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('?'), Action::ToggleHelp)],
    },
    HintRow {
        display: "q",
        label: "hint.quit",
        group: None,
        mode: KeyMode::List,
        checks: &[(KeyCode::Char('q'), Action::Quit)],
    },
    // ---- Data page ------------------------------------------------------
    HintRow {
        display: "j/k or ↑↓",
        label: "hint.select",
        group: Some("Backups"),
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('j'), Action::Down)],
    },
    HintRow {
        display: "Enter",
        label: "hint.restore",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Enter, Action::Restore)],
    },
    HintRow {
        display: "b",
        label: "hint.snapshot",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('b'), Action::Backup)],
    },
    HintRow {
        display: "e",
        label: "hint.setup",
        group: Some("Sync"),
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('e'), Action::SyncSetup)],
    },
    HintRow {
        display: "Tab",
        label: "hint.sync_tab",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Tab, Action::SyncTab)],
    },
    HintRow {
        display: "p",
        label: "hint.push",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('p'), Action::SyncPush)],
    },
    HintRow {
        display: "u",
        label: "hint.pull",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('u'), Action::SyncPull)],
    },
    HintRow {
        display: "Esc",
        label: "hint.back",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Esc, Action::Back)],
    },
    HintRow {
        display: "q",
        label: "hint.quit",
        group: None,
        mode: KeyMode::Data,
        checks: &[(KeyCode::Char('q'), Action::Quit)],
    },
    // ---- Settings page --------------------------------------------------
    HintRow {
        display: "j/k or ↑↓",
        label: "hint.move",
        group: None,
        mode: KeyMode::Settings,
        checks: &[(KeyCode::Char('k'), Action::Up)],
    },
    HintRow {
        display: "Space / Enter",
        label: "hint.toggle",
        group: None,
        mode: KeyMode::Settings,
        checks: &[
            (KeyCode::Char(' '), Action::ToggleSetting),
            (KeyCode::Enter, Action::ToggleSetting),
        ],
    },
    HintRow {
        display: "Esc",
        label: "hint.back",
        group: None,
        mode: KeyMode::Settings,
        checks: &[(KeyCode::Esc, Action::Back)],
    },
    HintRow {
        display: "q",
        label: "hint.quit",
        group: None,
        mode: KeyMode::Settings,
        checks: &[(KeyCode::Char('q'), Action::Quit)],
    },
];

/// Hints for one page as (display, translated-label, section-group) rows.
pub(crate) fn hint_rows(mode: KeyMode) -> Vec<(&'static str, String, Option<&'static str>)> {
    HINTS
        .iter()
        .filter(|h| h.mode == mode)
        .map(|h| (h.display, crate::i18n::t(h.label).to_string(), h.group))
        .collect()
}

/// The single-line status-bar hint for one page.
pub(crate) fn hint_bar(mode: KeyMode) -> String {
    hint_rows(mode)
        .into_iter()
        .map(|(k, v, _)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn digits_do_not_select_apps() {
        assert_eq!(
            map_key(key(KeyCode::Char('1')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('4')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char(']')), KeyMode::List),
            Action::NextApp
        );
        assert_eq!(
            map_key(key(KeyCode::Char('[')), KeyMode::List),
            Action::PrevApp
        );
        assert_eq!(map_key(key(KeyCode::Tab), KeyMode::List), Action::NextApp);
        assert_eq!(
            map_key(key(KeyCode::BackTab), KeyMode::List),
            Action::PrevApp
        );
        assert_eq!(
            map_key(key(KeyCode::Char(']')), KeyMode::Data),
            Action::None
        );
    }

    #[test]
    fn list_keys_match_design() {
        assert_eq!(
            map_key(key(KeyCode::Char('q')), KeyMode::List),
            Action::Quit
        );
        assert_eq!(
            map_key(key(KeyCode::Char('?')), KeyMode::List),
            Action::ToggleHelp
        );
        assert_eq!(
            map_key(key(KeyCode::Esc), KeyMode::Help),
            Action::CloseOverlay
        );
        assert_eq!(map_key(key(KeyCode::Esc), KeyMode::List), Action::Quit);
        assert_eq!(map_key(key(KeyCode::Esc), KeyMode::Data), Action::Back);
        assert_eq!(map_key(key(KeyCode::Enter), KeyMode::List), Action::Switch);
        assert_eq!(map_key(key(KeyCode::Enter), KeyMode::Help), Action::None);
        assert_eq!(
            map_key(key(KeyCode::Char('j')), KeyMode::List),
            Action::Down
        );
        assert_eq!(map_key(key(KeyCode::Down), KeyMode::List), Action::Down);
        assert_eq!(map_key(key(KeyCode::Char('k')), KeyMode::List), Action::Up);
        assert_eq!(map_key(key(KeyCode::Up), KeyMode::List), Action::Up);
        assert_eq!(map_key(key(KeyCode::Char('a')), KeyMode::List), Action::Add);
        assert_eq!(
            map_key(key(KeyCode::Char('e')), KeyMode::List),
            Action::Edit
        );
        assert_eq!(
            map_key(key(KeyCode::Char('d')), KeyMode::List),
            Action::Delete
        );
        assert_eq!(
            map_key(key(KeyCode::Char('m')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('b')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('r')), KeyMode::List),
            Action::OpenData
        );
        assert_eq!(
            map_key(key(KeyCode::Char('s')), KeyMode::List),
            Action::OpenData
        );
        assert_eq!(
            map_key(key(KeyCode::Char('g')), KeyMode::List),
            Action::OpenSettings
        );
        assert_eq!(
            map_key(key(KeyCode::Char(' ')), KeyMode::Settings),
            Action::ToggleSetting
        );
        assert_eq!(map_key(key(KeyCode::Esc), KeyMode::Settings), Action::Back);
        assert_eq!(
            map_key(key(KeyCode::Char('a')), KeyMode::Help),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('h')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('l')), KeyMode::List),
            Action::None
        );
    }

    #[test]
    fn hints_match_dispatcher() {
        for h in HINTS {
            assert!(
                !h.display.trim().is_empty(),
                "empty display in {:?}",
                h.label
            );
            assert!(
                !crate::i18n::t(h.label).is_empty(),
                "untranslated {}",
                h.label
            );
            assert!(!h.checks.is_empty(), "no checks for {}", h.label);
            for &(code, expected) in h.checks {
                assert_eq!(
                    map_key(key(code), h.mode),
                    expected,
                    "{code:?} on {:?} must dispatch to {:?}",
                    h.mode,
                    expected
                );
            }
        }
    }

    #[test]
    fn data_page_keys() {
        assert_eq!(map_key(key(KeyCode::Enter), KeyMode::Data), Action::Restore);
        assert_eq!(
            map_key(key(KeyCode::Char('b')), KeyMode::Data),
            Action::Backup
        );
        assert_eq!(
            map_key(key(KeyCode::Char('p')), KeyMode::Data),
            Action::SyncPush
        );
        assert_eq!(
            map_key(key(KeyCode::Char('u')), KeyMode::Data),
            Action::SyncPull
        );
        assert_eq!(
            map_key(key(KeyCode::Char('e')), KeyMode::Data),
            Action::SyncSetup
        );
        assert_eq!(map_key(key(KeyCode::Tab), KeyMode::Data), Action::SyncTab);
        assert_eq!(
            map_key(key(KeyCode::BackTab), KeyMode::Data),
            Action::SyncTab
        );
        // Tab only switches the sync tab on the Data page; on Providers it
        // still moves between apps.
        assert_eq!(map_key(key(KeyCode::Tab), KeyMode::List), Action::NextApp);
        assert_eq!(
            map_key(key(KeyCode::Char('p')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('s')), KeyMode::Data),
            Action::None
        );
    }
}
