//! Single source of truth for every machine-read occurrence of the product
//! name — config dir, env vars, config keys, file names, asset names.
//!
//! Renaming the tool means editing this file plus:
//! `Cargo.toml` (`package.name`, repository), the GitHub repo itself,
//! `install.sh` / `install.ps1`, and prose docs (`README*.md`,
//! `docs/design*.md`). Everything the binary reads or writes at runtime
//! flows through the constants below.
//!
//! Doc comments and i18n prose are not constant-expressible; grep for the
//! old name once when renaming.

/// Binary and display name (`aimux use <id>`, help header).
pub const NAME: &str = "aimux";

/// Config directory under `$HOME` (`~/.aimux`), overridable via
/// [`ENV_CONFIG_DIR`].
pub const DOT_DIR: &str = ".aimux";

/// Log file inside the config dir.
pub const LOG_FILE: &str = "aimux.log";

// --- environment variables -------------------------------------------------

/// Overrides the config directory location.
pub const ENV_CONFIG_DIR: &str = "AIMUX_CONFIG_DIR";

/// UI language override.
pub const ENV_LANG: &str = "AIMUX_LANG";

/// Set to `1` to print full API keys (dangerous).
pub const ENV_SHOW_SECRETS: &str = "AIMUX_SHOW_SECRETS";

// --- written into live configs ----------------------------------------------

/// WebDAV collection namespace under the user-provided root.
pub const SYNC_NAMESPACE: &str = "aimux-sync";

/// Format tag stored inside the cloud `manifest.json`.
pub const MANIFEST_FORMAT: &str = "aimux-webdav-sync";

/// Codex model catalog file written next to `config.toml`. No product-name
/// prefix: the file already lives inside the app's own config dir.
pub const CODEX_CATALOG_FILE: &str = "model-catalog.json";

/// Fallback slot key when a provider has no usable display name.
pub const DEFAULT_SLOT_KEY: &str = "managed";
