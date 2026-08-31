//! Single source of truth for the product name and everything derived from it.
//!
//! # Rename policy
//!
//! To rename the tool, change **only** the two package macros at the top:
//! `pkg!()` (lowercase, used in file/dir names, namespace, assets) and
//! `envpref!()` (uppercase, used in environment variable prefixes). Everything
//! else below is derived via `concat!` at compile time.
//!
//! Data-layer values (store schema, manifest format, slot keys, catalog file)
//! are deliberately **package-name-free**: they ride along across renames and
//! are reads-compatible with every prior format.

// --- single editable source --------------------------------------------------

macro_rules! pkg {
    () => {
        "apmux"
    };
}

macro_rules! envpref {
    () => {
        "APMUX"
    };
}

// --- derived constants (binary name, files, namespaces, env keys) ------------

/// Binary and display name (`apmux use <id>`, help header).
pub const NAME: &str = pkg!();

/// Config directory under `$HOME` (`~/.apmux`), overridable via
/// [`ENV_CONFIG_DIR`].
pub const DOT_DIR: &str = concat!(".", pkg!());

/// The default config dir used by releases before the rename. Read-only
/// compatibility for the one-time local migration; never created.
pub const LEGACY_DOT_DIR: &str = "aimux";

/// Log file inside the config dir.
pub const LOG_FILE: &str = concat!(pkg!(), ".log");

/// WebDAV collection namespace under the user-provided root. This is the
/// only cloud-side name coupled to the package name.
pub const SYNC_NAMESPACE: &str = concat!(pkg!(), "-sync");

/// The namespace used by releases before the rename (`api?`). Kept for the
/// one-time automatic cloud migration; never written.
pub const LEGACY_SYNC_NAMESPACE: &str = "aimux-sync";

// --- environment variables (derived with the uppercase prefix) ---------------

/// Overrides the config directory location.
pub const ENV_CONFIG_DIR: &str = concat!(envpref!(), "_CONFIG_DIR");

/// UI language override.
pub const ENV_LANG: &str = concat!(envpref!(), "_LANG");

/// Set to `1` to print full API keys (dangerous).
pub const ENV_SHOW_SECRETS: &str = concat!(envpref!(), "_SHOW_SECRETS");

/// The env var name used by installers and shells before the rename.
/// Read as a fallback so existing scripts keep working after an upgrade.
pub const LEGACY_ENV_CONFIG_DIR: &str = "AIMUX_CONFIG_DIR";

/// Pre-rename language env var, read as a fallback like [`LEGACY_ENV_CONFIG_DIR`].
pub const LEGACY_ENV_LANG: &str = "AIMUX_LANG";

/// Pre-rename secrets-display env var, read as a fallback.
pub const LEGACY_ENV_SHOW_SECRETS: &str = "AIMUX_SHOW_SECRETS";

/// Read a runtime env var preferring the new name and falling back to the
/// pre-rename one (empty strings count as unset, matching the env helpers).
pub fn read_env(new: &str, legacy: &str) -> Option<String> {
    let pick = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty());
    match (pick(new), pick(legacy)) {
        (Some(v), _) => Some(v),
        (None, Some(v)) => {
            log::warn!("env {legacy} is deprecated; rename it to {new}");
            Some(v)
        }
        (None, None) => None,
    }
}

// --- data-layer values (package-name-free, stable across renames) ------------

/// Format tag stored inside the cloud `manifest.json`. Package-name-free;
/// reads also accept [`LEGACY_MANIFEST_FORMAT`] from pre-rename releases.
pub const MANIFEST_FORMAT: &str = "webdav-sync";

/// Manifest format tag written by releases before the rename. Read-only
/// compatibility; never written.
pub const LEGACY_MANIFEST_FORMAT: &str = "aimux-webdav-sync";

/// Codex model catalog file written next to `config.toml`. No product-name
/// prefix: the file already lives inside the app's own config dir.
pub const CODEX_CATALOG_FILE: &str = "model-catalog.json";

/// Fallback slot key when a provider has no usable display name.
pub const DEFAULT_SLOT_KEY: &str = "managed";
