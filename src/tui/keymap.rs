use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    List,
    Backups,
    Sync,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
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
    OpenBackups,
    OpenSync,
    OpenSettings,
    ToggleSetting,
    Back,
    Restore,
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
            } else if matches!(mode, KeyMode::Backups | KeyMode::Sync | KeyMode::Settings) {
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
            KeyMode::Backups => Action::Restore,
            KeyMode::Settings => Action::ToggleSetting,
            _ => Action::None,
        },
        KeyCode::Char('a') | KeyCode::Char('A') if mode == KeyMode::List => Action::Add,
        KeyCode::Char('e') | KeyCode::Char('E') => match mode {
            KeyMode::List => Action::Edit,
            KeyMode::Sync => Action::SyncSetup,
            _ => Action::None,
        },
        KeyCode::Char('d') | KeyCode::Char('D') if mode == KeyMode::List => Action::Delete,
        KeyCode::Char('b') | KeyCode::Char('B') => Action::Backup,
        KeyCode::Char('r') | KeyCode::Char('R') => Action::OpenBackups,
        KeyCode::Char('s') | KeyCode::Char('S')
            if mode != KeyMode::Sync && mode != KeyMode::Settings =>
        {
            Action::OpenSync
        }
        KeyCode::Char('g') | KeyCode::Char('G') if mode != KeyMode::Settings => {
            Action::OpenSettings
        }
        KeyCode::Char(' ') if mode == KeyMode::Settings => Action::ToggleSetting,
        KeyCode::Char('p') | KeyCode::Char('P') if mode == KeyMode::Sync => Action::SyncPush,
        KeyCode::Char('u') | KeyCode::Char('U') if mode == KeyMode::Sync => Action::SyncPull,
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
            map_key(key(KeyCode::Char(']')), KeyMode::Backups),
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
        assert_eq!(map_key(key(KeyCode::Esc), KeyMode::Backups), Action::Back);
        assert_eq!(map_key(key(KeyCode::Esc), KeyMode::Sync), Action::Back);
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
            Action::Backup
        );
        assert_eq!(
            map_key(key(KeyCode::Char('r')), KeyMode::List),
            Action::OpenBackups
        );
        assert_eq!(
            map_key(key(KeyCode::Char('s')), KeyMode::List),
            Action::OpenSync
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
    fn backups_and_sync_keys() {
        assert_eq!(
            map_key(key(KeyCode::Enter), KeyMode::Backups),
            Action::Restore
        );
        assert_eq!(
            map_key(key(KeyCode::Char('b')), KeyMode::Backups),
            Action::Backup
        );
        assert_eq!(
            map_key(key(KeyCode::Char('p')), KeyMode::Sync),
            Action::SyncPush
        );
        assert_eq!(
            map_key(key(KeyCode::Char('u')), KeyMode::Sync),
            Action::SyncPull
        );
        assert_eq!(
            map_key(key(KeyCode::Char('e')), KeyMode::Sync),
            Action::SyncSetup
        );
        assert_eq!(
            map_key(key(KeyCode::Char('p')), KeyMode::List),
            Action::None
        );
        assert_eq!(
            map_key(key(KeyCode::Char('s')), KeyMode::Sync),
            Action::None
        );
    }
}
