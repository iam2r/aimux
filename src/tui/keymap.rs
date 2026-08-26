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
        KeyCode::Char('e') | KeyCode::Char('E') => match mode {
            KeyMode::List => Action::Edit,
            KeyMode::Data => Action::SyncSetup,
            _ => Action::None,
        },
        KeyCode::Char('d') | KeyCode::Char('D') if mode == KeyMode::List => Action::Delete,
        KeyCode::Char('b') | KeyCode::Char('B') => Action::Backup,
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
            Action::Backup
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
