use std::env;

/// Mask a secret for display: first 4 + last 4 chars with `…` in between.
/// Keys shorter than 8 characters are all `*`.
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let n = chars.len();
    if n < 8 {
        return "*".repeat(n);
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[n - 4..].iter().collect();
    format!("{prefix}…{suffix}")
}

/// `AIMUX_SHOW_SECRETS=1` prints full keys. Dangerous: keys land in terminal
/// scrollback, logs, and CI artifacts. Default is masked.
pub fn show_secrets() -> bool {
    matches!(env::var(crate::name::ENV_SHOW_SECRETS), Ok(v) if v == "1")
}

pub fn display_key(key: &str, show_secrets: bool) -> String {
    if show_secrets {
        key.to_string()
    } else {
        mask_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_keys_are_all_stars() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("short"), "*****");
        assert_eq!(mask_key("1234567"), "*******");
    }

    #[test]
    fn long_keys_keep_prefix_and_suffix() {
        assert_eq!(mask_key("sk-ant-secretabcd"), "sk-a…abcd");
        assert_eq!(mask_key("abcdefgh"), "abcd…efgh");
    }

    #[test]
    fn unicode_counts_chars() {
        assert_eq!(mask_key("ééééééééxxxx"), "éééé…xxxx");
    }

    #[test]
    fn display_key_respects_show_secrets() {
        assert_eq!(display_key("sk-ant-secretabcd", false), "sk-a…abcd");
        assert_eq!(display_key("sk-ant-secretabcd", true), "sk-ant-secretabcd");
    }
}
