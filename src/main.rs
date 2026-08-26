mod adapter;
mod backup;
mod cloud;
mod error;
mod fsutil;
mod i18n;
mod import;
mod mask;
mod name;
mod paths;
mod settings;
mod store;
mod switch;
mod tui;
mod update;
mod webdav;

use anyhow::Result;
use clap::{Parser, Subcommand};

use adapter::AppId;
use paths::Paths;
use store::{Provider, Store};

#[derive(Debug, Parser)]
#[command(
    name = crate::name::NAME,
    version,
    about = "Lightweight AI CLI provider switcher",
    disable_version_flag = true
)]
struct Cli {
    /// Print version
    #[arg(short = 'V', long = "version", action = clap::ArgAction::SetTrue)]
    print_version: bool,

    /// UI language: en or zh (default: English; also AIMUX_LANG / LANG)
    #[arg(long, global = true, env = crate::name::ENV_LANG)]
    lang: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List providers
    List {
        #[arg(long)]
        app: Option<AppId>,
        /// JSON to stdout. Keys stay masked unless AIMUX_SHOW_SECRETS=1 (dangerous).
        #[arg(long)]
        json: bool,
    },
    /// Show the current provider per app
    Current {
        #[arg(long)]
        app: Option<AppId>,
        /// JSON to stdout. Keys stay masked unless AIMUX_SHOW_SECRETS=1 (dangerous).
        #[arg(long)]
        json: bool,
    },
    /// Switch to a provider
    Use {
        id: String,
        #[arg(long)]
        app: Option<AppId>,
    },
    /// Add a provider
    Add {
        #[arg(long)]
        app: AppId,
        #[arg(long)]
        name: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        api_key: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        id: Option<String>,
        /// Adapter-specific field as key=value (repeatable)
        #[arg(long)]
        extra: Vec<String>,
        /// Merge this provider's snippet when switching
        #[arg(long)]
        apply_snippet: bool,
    },
    /// Edit a provider
    Edit {
        id: String,
        #[arg(long)]
        app: Option<AppId>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, conflicts_with = "model")]
        clear_model: bool,
        #[arg(long)]
        extra: Vec<String>,
        /// Merge this provider's snippet when switching
        #[arg(long, conflicts_with = "no_apply_snippet")]
        apply_snippet: bool,
        /// Stop merging this provider's snippet
        #[arg(long)]
        no_apply_snippet: bool,
    },
    /// Delete a provider
    Delete {
        id: String,
        #[arg(long)]
        app: Option<AppId>,
        #[arg(long)]
        yes: bool,
    },
    /// Write a local backup of store.json
    Backup {
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Restore store.json from a local backup
    Restore {
        name: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        no_apply: bool,
    },
    /// List local backups
    Backups,
    /// Cloud sync (WebDAV)
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Import providers and WebDAV credentials from cc-switch
    Import {
        /// Path to cc-switch.db (default: ~/.cc-switch/cc-switch.db)
        #[arg(long)]
        db: Option<std::path::PathBuf>,
        /// Path to cc-switch settings.json (default: ~/.cc-switch/settings.json)
        #[arg(long)]
        settings: Option<std::path::PathBuf>,
        /// Print mapping, do not write store.json or webdav.json
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing ids, current, and webdav.json (keeps Pi and others)
        #[arg(long)]
        force: bool,
    },
    /// Provider snippet (JSON object)
    Snippet {
        id: String,
        #[arg(long)]
        app: Option<AppId>,
        /// JSON object to store
        #[arg(long)]
        set: Option<String>,
        /// Delete the snippet for this provider
        #[arg(long, conflicts_with = "set")]
        clear: bool,
    },
    /// Replace this binary from GitHub Releases
    Update {
        /// Target version (example: v0.2.0). Defaults to latest.
        #[arg(long, conflicts_with = "check")]
        version: Option<String>,
        /// Only check for updates; do not download or replace the binary
        #[arg(long)]
        check: bool,
        /// JSON to stdout (requires --check)
        #[arg(long, requires = "check")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SyncAction {
    Setup {
        #[arg(long)]
        url: String,
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
    },
    Push {
        #[arg(long)]
        force: bool,
    },
    Pull {
        #[arg(long)]
        force: bool,
    },
    Status,
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            // clap usage/validation → 1; --help/--version (stdout) → 0
            std::process::exit(if e.use_stderr() { 1 } else { 0 });
        }
    };
    if cli.print_version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    i18n::init(cli.lang.as_deref());
    if let Err(err) = run(cli) {
        eprintln!("Error: {err:#}");
        std::process::exit(error::exit_code(&err));
    }
}

fn init_cli_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            let paths = Paths::from_env()?;
            let saved = settings::Settings::load(&paths).ok();
            i18n::init_tui(
                cli.lang.as_deref(),
                saved.as_ref().and_then(|s| s.lang.as_deref()),
            );
            tui::run(paths)
        }
        Some(cmd) => {
            init_cli_logger();
            run_command(cmd)
        }
    }
}

fn run_command(cmd: Commands) -> Result<()> {
    match cmd {
        Commands::List { app, json } => cmd_list(app, json),
        Commands::Current { app, json } => cmd_current(app, json),
        Commands::Use { id, app } => cmd_use(id, app),
        Commands::Add {
            app,
            name,
            base_url,
            api_key,
            model,
            id,
            extra,
            apply_snippet,
        } => cmd_add(switch::AddOpts {
            app,
            name,
            base_url,
            api_key,
            model,
            id,
            extra,
            catalog: Vec::new(),
            slots: Default::default(),
            snippet: None,
            apply_snippet,
        }),
        Commands::Edit {
            id,
            app,
            name,
            base_url,
            api_key,
            model,
            clear_model,
            extra,
            apply_snippet,
            no_apply_snippet,
        } => cmd_edit(switch::EditOpts {
            query: id,
            app,
            name,
            base_url,
            api_key,
            model,
            clear_model,
            extra,
            catalog: None,
            slots: None,
            snippet: None,
            apply_snippet: if apply_snippet {
                Some(true)
            } else if no_apply_snippet {
                Some(false)
            } else {
                None
            },
        }),
        Commands::Delete { id, app, yes } => cmd_delete(id, app, yes),
        Commands::Backup { name } => cmd_backup(name),
        Commands::Restore {
            name,
            yes,
            no_apply,
        } => cmd_restore(name, yes, no_apply),
        Commands::Backups => cmd_backups(),
        Commands::Sync { action } => match action {
            SyncAction::Setup {
                url,
                username,
                password,
            } => cmd_sync_setup(url, username, password),
            SyncAction::Push { force } => cmd_sync_push(force),
            SyncAction::Pull { force } => cmd_sync_pull(force),
            SyncAction::Status => cmd_sync_status(),
        },
        Commands::Import {
            db,
            settings,
            dry_run,
            force,
        } => cmd_import(db, settings, dry_run, force),
        Commands::Snippet {
            id,
            app,
            set,
            clear,
        } => cmd_snippet(id, app, set, clear),
        Commands::Update {
            version,
            check,
            json,
        } => crate::update::run(version, check, json),
    }
}

fn cmd_snippet(query: String, app: Option<AppId>, set: Option<String>, clear: bool) -> Result<()> {
    let (paths, mut store) = load_store()?;
    let id = switch::resolve(&store, &query, app)?.id.clone();
    if !store.providers.contains_key(&id) {
        anyhow::bail!("provider not found: {id}");
    }
    if clear {
        if let Some(provider) = store.providers.get_mut(&id) {
            provider.snippet = None;
        }
        store.save(&paths)?;
        println!("cleared snippet for {id}");
        return Ok(());
    }
    if let Some(raw) = set {
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        if !value.is_object() {
            anyhow::bail!("snippet must be a JSON object");
        }
        if let Some(provider) = store.providers.get_mut(&id) {
            provider.snippet = store::normalize_snippet(Some(value));
        }
        store.save(&paths)?;
        println!("saved snippet for {id}");
        return Ok(());
    }
    match store.providers.get(&id).and_then(|p| p.snippet.as_ref()) {
        Some(value) => println!("{}", serde_json::to_string_pretty(value)?),
        None => println!("(empty)"),
    }
    Ok(())
}

fn load_store() -> Result<(Paths, Store)> {
    let paths = Paths::from_env()?;
    let mut store = Store::load(&paths)?;
    // First run with no store on disk: adopt providers from hand-edited
    // agent configs so users don't re-enter them. Nothing is written when
    // nothing is found.
    if !paths.store_file().exists() {
        switch::rescue_from_live(&paths, &mut store)?;
        if !store.providers.is_empty() {
            store.save(&paths)?;
        }
    }
    Ok((paths, store))
}

fn require_adapter(app: Option<AppId>) -> Result<()> {
    if let Some(app) = app {
        adapter::get(app)?;
    }
    Ok(())
}

fn cmd_list(app: Option<AppId>, json: bool) -> Result<()> {
    require_adapter(app)?;
    let (_paths, store) = load_store()?;
    print_list(&store, app, json, mask::show_secrets())
}

fn cmd_current(app: Option<AppId>, json: bool) -> Result<()> {
    require_adapter(app)?;
    let (_paths, store) = load_store()?;
    print_current(&store, app, json, mask::show_secrets())
}

fn print_list(store: &Store, app: Option<AppId>, json: bool, show_secrets: bool) -> Result<()> {
    print!("{}", render_list(store, app, json, show_secrets)?);
    Ok(())
}

fn print_current(store: &Store, app: Option<AppId>, json: bool, show_secrets: bool) -> Result<()> {
    print!("{}", render_current(store, app, json, show_secrets)?);
    Ok(())
}

fn render_list(
    store: &Store,
    app: Option<AppId>,
    json: bool,
    show_secrets: bool,
) -> Result<String> {
    let rows: Vec<&Provider> = store
        .providers
        .values()
        .filter(|p| app.is_none_or(|a| p.app == a))
        .collect();
    if json {
        let values: Vec<serde_json::Value> = rows
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "app": p.app,
                    "name": p.name,
                    "api_key": mask::display_key(&p.api_key, show_secrets),
                    "model": p.model,
                    "current": store.current.get(&p.app).is_some_and(|c| c == &p.id),
                })
            })
            .collect();
        return pretty_json(&values);
    }
    if rows.is_empty() {
        return Ok("No providers configured.\n".into());
    }
    let mut out = format!(
        "{:<16} {:<8} {:<24} {:<16} CURRENT\n",
        "ID", "APP", "NAME", "KEY"
    );
    for p in rows {
        let current = store.current.get(&p.app).is_some_and(|c| c == &p.id);
        let mark = if current { "*" } else { "" };
        out.push_str(&format!(
            "{:<16} {:<8} {:<24} {:<16} {mark}\n",
            p.id,
            p.app,
            p.name,
            mask::display_key(&p.api_key, show_secrets)
        ));
    }
    Ok(out)
}

fn render_current(
    store: &Store,
    app: Option<AppId>,
    json: bool,
    show_secrets: bool,
) -> Result<String> {
    if json {
        if let Some(app) = app {
            let value = match store.current.get(&app) {
                Some(id) => current_json(store, app, id, show_secrets),
                None => serde_json::Value::Null,
            };
            return pretty_json(&value);
        }
        let mut map = serde_json::Map::new();
        for (app, id) in &store.current {
            map.insert(app.to_string(), current_json(store, *app, id, show_secrets));
        }
        return pretty_json(&serde_json::Value::Object(map));
    }
    if let Some(app) = app {
        return Ok(match store.current.get(&app) {
            Some(id) => current_line(store, app, id, show_secrets),
            None => format!("{app}: (none)\n"),
        });
    }
    if store.current.is_empty() {
        return Ok("No current provider.\n".into());
    }
    let mut out = String::new();
    for (app, id) in &store.current {
        out.push_str(&current_line(store, *app, id, show_secrets));
    }
    Ok(out)
}

fn current_json(store: &Store, app: AppId, id: &str, show_secrets: bool) -> serde_json::Value {
    match store.providers.get(id) {
        Some(p) => serde_json::json!({
            "id": id,
            "app": app,
            "name": p.name,
            "api_key": mask::display_key(&p.api_key, show_secrets),
        }),
        None => serde_json::json!({ "id": id, "app": app }),
    }
}

fn current_line(store: &Store, app: AppId, id: &str, show_secrets: bool) -> String {
    match store.providers.get(id) {
        Some(p) => format!(
            "{app}: {id}  {}  {}\n",
            p.name,
            mask::display_key(&p.api_key, show_secrets)
        ),
        None => format!("{app}: {id}\n"),
    }
}

fn pretty_json<T: serde::Serialize>(value: &T) -> Result<String> {
    let mut s = serde_json::to_string_pretty(value)?;
    if !s.ends_with('\n') {
        s.push('\n');
    }
    Ok(s)
}

fn cmd_use(id: String, app: Option<AppId>) -> Result<()> {
    if let Some(app) = app {
        adapter::get(app)?;
    }
    let (paths, mut store) = load_store()?;
    let switched = switch::use_provider(&paths, &mut store, &id, app)?;
    println!("switched {switched}");
    Ok(())
}

fn cmd_add(opts: switch::AddOpts) -> Result<()> {
    adapter::get(opts.app)?;
    let (paths, mut store) = load_store()?;
    let id = switch::add_provider(&paths, &mut store, opts)?;
    println!("added {id}");
    Ok(())
}

fn cmd_edit(opts: switch::EditOpts) -> Result<()> {
    let (paths, mut store) = load_store()?;
    let id = switch::edit_provider(&paths, &mut store, opts)?;
    println!("updated {id}");
    Ok(())
}

fn cmd_delete(query: String, app: Option<AppId>, yes: bool) -> Result<()> {
    let (paths, mut store) = load_store()?;
    let id = switch::delete_provider(&paths, &mut store, &query, app, yes)?;
    println!("deleted {id}");
    Ok(())
}

fn cmd_backup(name: Option<String>) -> Result<()> {
    let paths = Paths::from_env()?;
    let stem = backup::create(&paths, name.as_deref())?;
    println!("backed up {stem}");
    Ok(())
}

fn cmd_restore(name: String, yes: bool, no_apply: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    backup::restore(&paths, &name, yes, no_apply)?;
    println!("restored {name}");
    Ok(())
}

fn cmd_backups() -> Result<()> {
    let paths = Paths::from_env()?;
    let entries = backup::list(&paths)?;
    if entries.is_empty() {
        println!("No backups.");
        return Ok(());
    }
    for entry in entries {
        println!("{}", entry.name);
    }
    Ok(())
}

fn cmd_sync_setup(url: String, username: String, password: String) -> Result<()> {
    let paths = Paths::from_env()?;
    cloud::setup(&paths, url, username, password)?;
    println!("webdav configured");
    Ok(())
}

fn cmd_sync_push(force: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    let sha = cloud::push(&paths, force)?;
    println!("pushed {sha}");
    Ok(())
}

fn cmd_sync_pull(force: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    let sha = cloud::pull(&paths, force)?;
    println!("pulled {sha}");
    Ok(())
}

fn cmd_sync_status() -> Result<()> {
    let paths = Paths::from_env()?;
    print!("{}", cloud::status(&paths)?);
    Ok(())
}

fn cmd_import(
    db: Option<std::path::PathBuf>,
    settings: Option<std::path::PathBuf>,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let (paths, mut store) = load_store()?;
    let mut opts = import::ImportOpts::from_home(&paths.home);
    if let Some(db) = db {
        opts.db = db;
    }
    if let Some(settings) = settings {
        opts.settings = settings;
    }
    opts.dry_run = dry_run;
    opts.force = force;
    let report = import::run(&paths, &mut store, &opts)?;
    println!("{}", report.format(&paths.store_file(), dry_run));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_subcommand_is_none() {
        let cli = Cli::try_parse_from([crate::name::NAME]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.print_version);
    }

    #[test]
    fn version_flag_is_not_an_error() {
        let cli = Cli::try_parse_from([crate::name::NAME, "--version"]).unwrap();
        assert!(cli.print_version);
        let cli = Cli::try_parse_from([crate::name::NAME, "-V"]).unwrap();
        assert!(cli.print_version);
    }

    #[test]
    fn list_with_app() {
        let cli = Cli::try_parse_from([crate::name::NAME, "list", "--app", "claude"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::List {
                app: Some(AppId::Claude),
                json: false
            })
        ));
    }

    #[test]
    fn list_and_current_json_flag() {
        let cli = Cli::try_parse_from([crate::name::NAME, "list", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::List {
                app: None,
                json: true
            })
        ));
        let cli =
            Cli::try_parse_from([crate::name::NAME, "current", "--app", "pi", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Current {
                app: Some(AppId::Pi),
                json: true
            })
        ));
    }

    #[test]
    fn add_shape() {
        let cli = Cli::try_parse_from([
            "aimux",
            "add",
            "--app",
            "opencode",
            "--name",
            "Packy",
            "--base-url",
            "https://example.com",
            "--api-key",
            "sk-test",
            "--extra",
            "protocol=anthropic",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Add {
                app, name, extra, ..
            }) => {
                assert_eq!(app, AppId::OpenCode);
                assert_eq!(name, "Packy");
                assert_eq!(extra, vec!["protocol=anthropic"]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn edit_clear_model_conflicts_with_model() {
        let err = Cli::try_parse_from([
            crate::name::NAME,
            "edit",
            "packy",
            "--model",
            "foo",
            "--clear-model",
        ])
        .unwrap_err();
        let s = err.to_string();
        assert!(
            s.contains("cannot be used with") || s.contains("conflict"),
            "{s}"
        );
    }

    #[test]
    fn snippet_shape() {
        let cli = Cli::try_parse_from([
            "aimux",
            "snippet",
            "packy",
            "--set",
            r#"{"includeCoAuthoredBy":false}"#,
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Snippet {
                id,
                app,
                set,
                clear,
            }) => {
                assert_eq!(id, "packy");
                assert_eq!(app, None);
                assert_eq!(set.as_deref(), Some(r#"{"includeCoAuthoredBy":false}"#));
                assert!(!clear);
            }
            other => panic!("unexpected {other:?}"),
        }
        let cli = Cli::try_parse_from([
            "aimux",
            "add",
            "--app",
            "claude",
            "--name",
            "X",
            "--base-url",
            "https://x.example",
            "--api-key",
            "k",
            "--apply-snippet",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Add { apply_snippet, .. }) => assert!(apply_snippet),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn update_flags() {
        let cli = Cli::try_parse_from([crate::name::NAME, "update", "--check"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Update {
                version: None,
                check: true,
                json: false
            })
        ));
        let cli = Cli::try_parse_from([crate::name::NAME, "update", "--check", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Update {
                check: true,
                json: true,
                ..
            })
        ));
        let cli =
            Cli::try_parse_from([crate::name::NAME, "update", "--version", "v0.3.0"]).unwrap();
        match cli.command {
            Some(Commands::Update {
                version,
                check,
                json,
            }) => {
                assert_eq!(version.as_deref(), Some("v0.3.0"));
                assert!(!check);
                assert!(!json);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(Cli::try_parse_from([crate::name::NAME, "update", "--json"]).is_err());
        assert!(Cli::try_parse_from([
            crate::name::NAME,
            "update",
            "--check",
            "--version",
            "v0.3.0"
        ])
        .is_err());
    }

    #[test]
    fn sync_setup_requires_url() {
        let err = Cli::try_parse_from([
            crate::name::NAME,
            "sync",
            "setup",
            "--username",
            "u",
            "--password",
            "p",
        ])
        .unwrap_err();
        assert!(err.to_string().contains("url") || err.to_string().contains("required"));
    }

    #[test]
    fn value_enum_includes_all_apps() {
        for app in ["claude", "codex", "opencode", "pi"] {
            let cli = Cli::try_parse_from([crate::name::NAME, "list", "--app", app]).unwrap();
            assert!(matches!(
                cli.command,
                Some(Commands::List { app: Some(_), .. })
            ));
        }
        let err = Cli::try_parse_from([crate::name::NAME, "list", "--app", "gemini"]).unwrap_err();
        assert!(err.to_string().contains("invalid") || err.to_string().contains("gemini"));
        let err =
            Cli::try_parse_from([crate::name::NAME, "list", "--app", "open-code"]).unwrap_err();
        assert!(err.to_string().contains("invalid") || err.to_string().contains("open-code"));
    }

    #[test]
    fn list_and_current_accept_all_registered_apps() {
        let store = Store::empty();
        for app in [AppId::Claude, AppId::Codex, AppId::OpenCode, AppId::Pi] {
            require_adapter(Some(app)).unwrap();
            print_list(&store, Some(app), false, false).unwrap();
            print_current(&store, Some(app), false, false).unwrap();
        }
    }

    fn sample_store() -> Store {
        let mut store = Store::empty();
        store.providers.insert(
            "packy".into(),
            Provider {
                id: "packy".into(),
                name: "PackyCode".into(),
                app: AppId::Claude,
                base_url: "https://example.com".into(),
                api_key: "sk-test-key-abcd".into(),
                model: Some("sonnet".into()),
                extras: Default::default(),
                ..Provider::blank(AppId::Claude)
            },
        );
        store.providers.insert(
            "packy-codex".into(),
            Provider {
                id: "packy-codex".into(),
                name: "Packy Codex".into(),
                app: AppId::Codex,
                base_url: "https://example.com".into(),
                api_key: "sk-codex-keyxx".into(),
                model: None,
                extras: Default::default(),
                ..Provider::blank(AppId::Codex)
            },
        );
        store.current.insert(AppId::Claude, "packy".into());
        store
    }

    #[test]
    fn list_json_masks_keys_unless_show_secrets() {
        let store = sample_store();
        let masked = render_list(&store, None, true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&masked).unwrap();
        assert_eq!(v.as_array().map(Vec::len), Some(2));
        assert_eq!(v[0]["id"], "packy");
        assert_eq!(v[0]["api_key"], "sk-t…abcd");
        assert_eq!(v[0]["current"], true);
        assert_eq!(v[0]["model"], "sonnet");
        assert!(!masked.contains("sk-test-key-abcd"));
        assert!(!masked.contains("sk-codex-keyxx"));

        let full = render_list(&store, None, true, true).unwrap();
        assert!(full.contains("sk-test-key-abcd"));
        assert!(full.contains("sk-codex-keyxx"));

        let claude_only = render_list(&store, Some(AppId::Claude), true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&claude_only).unwrap();
        assert_eq!(v.as_array().map(Vec::len), Some(1));
        assert_eq!(v[0]["app"], "claude");

        let empty = render_list(&store, Some(AppId::Pi), true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&empty).unwrap();
        assert_eq!(v, serde_json::json!([]));
    }

    #[test]
    fn current_json_masks_keys() {
        let store = sample_store();
        let masked = render_current(&store, None, true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&masked).unwrap();
        assert_eq!(v["claude"]["id"], "packy");
        assert_eq!(v["claude"]["api_key"], "sk-t…abcd");
        assert!(v.get("codex").is_none());
        assert!(!masked.contains("sk-test-key-abcd"));

        let none = render_current(&store, Some(AppId::Codex), true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&none).unwrap();
        assert!(v.is_null());

        let one = render_current(&store, Some(AppId::Claude), true, true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&one).unwrap();
        assert_eq!(v["api_key"], "sk-test-key-abcd");
    }

    #[test]
    fn list_table_masks_keys() {
        let store = sample_store();
        let text = render_list(&store, None, false, false).unwrap();
        assert!(text.contains("packy"));
        assert!(text.contains("sk-t…abcd"));
        assert!(!text.contains("sk-test-key-abcd"));
    }

    #[test]
    fn clap_usage_errors_go_to_stderr() {
        let err = Cli::try_parse_from([crate::name::NAME, "list", "--app"]).unwrap_err();
        assert!(err.use_stderr());
        let help = Cli::try_parse_from([crate::name::NAME, "list", "--help"]).unwrap_err();
        assert!(!help.use_stderr());
        let help_text = help.to_string();
        assert!(help_text.contains("AIMUX_SHOW_SECRETS=1"));
        assert!(help_text.contains("dangerous"));
    }

    #[test]
    fn backup_restore_clap_shape() {
        let cli = Cli::try_parse_from([crate::name::NAME, "backup", "--name", "snap"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Backup {
                name: Some(ref n)
            }) if n == "snap"
        ));
        let cli =
            Cli::try_parse_from([crate::name::NAME, "restore", "snap", "--yes", "--no-apply"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Restore {
                name,
                yes: true,
                no_apply: true
            }) if name == "snap"
        ));
        let cli = Cli::try_parse_from([crate::name::NAME, "backups"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Backups)));
    }

    #[test]
    fn sync_push_pull_force_and_status() {
        let cli = Cli::try_parse_from([crate::name::NAME, "sync", "push", "--force"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Sync {
                action: SyncAction::Push { force: true }
            })
        ));
        let cli = Cli::try_parse_from([crate::name::NAME, "sync", "pull"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Sync {
                action: SyncAction::Pull { force: false }
            })
        ));
        let cli = Cli::try_parse_from([crate::name::NAME, "sync", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Sync {
                action: SyncAction::Status
            })
        ));
        let cli = Cli::try_parse_from([
            "aimux",
            "sync",
            "setup",
            "--url",
            "https://webdav.example.com/",
            "--username",
            "u",
            "--password",
            "p",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Sync {
                action: SyncAction::Setup { .. }
            })
        ));
        assert!(Cli::try_parse_from([
            "aimux",
            "sync",
            "setup",
            "--url",
            "https://webdav.example.com/",
            "--username",
            "u",
            "--password",
            "p",
            "--jianguoyun",
        ])
        .is_err());
    }

    #[test]
    fn import_clap_shape() {
        let cli = Cli::try_parse_from([crate::name::NAME, "import", "--dry-run"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Import {
                dry_run: true,
                force: false,
                ..
            })
        ));
        let cli = Cli::try_parse_from([
            crate::name::NAME,
            "import",
            "--force",
            "--db",
            "/tmp/cc-switch.db",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Import {
                force: true,
                db: Some(path),
                ..
            }) => assert!(path.ends_with("cc-switch.db")),
            other => panic!("unexpected {other:?}"),
        }
    }
}
