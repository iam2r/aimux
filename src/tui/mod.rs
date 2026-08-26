mod app;
mod edit;
mod help;
mod keymap;
mod pages;
mod theme;
mod view;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use crate::fsutil;
use crate::paths::Paths;
use crate::store::Store;

pub use app::App;

pub fn run(paths: Paths) -> Result<()> {
    init_file_logger(&paths)?;
    let mut store = Store::load(&paths)?;
    // Same first-run adoption as the CLI path (see load_store in main.rs).
    if !paths.store_file().exists() {
        crate::switch::rescue_from_live(&paths, &mut store)?;
        if !store.providers.is_empty() {
            store.save(&paths)?;
        }
    }
    let mut app = App::new(paths, store);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        let _ = app.poll_sync();
        if let Some(job) = app.take_pending_try() {
            run_try_job(terminal, app, job)?;
        }
        terminal.draw(|f| view::draw(f, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if app.handle_key(key) {
                    break;
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
    Ok(())
}

/// Hand the real terminal over to the trial CLI: leave the alternate screen,
/// run attached to stdio, then come back and force a full repaint. Live
/// configs stay untouched — the CLI sees only the staged temp dir.
fn run_try_job(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    job: crate::try_launch::TryJob,
) -> Result<()> {
    let name = job.provider_name.clone();
    ratatui::restore();
    let result = job.run_detached();
    // resume even when launch failed; report through the status bar
    let resumed = (|| -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        terminal.clear()?;
        Ok(())
    })();
    resumed?;
    app.note_try_result(name, result);
    Ok(())
}

fn init_file_logger(paths: &Paths) -> Result<()> {
    fsutil::ensure_dir_0700(&paths.aimux_dir)?;
    let path = paths.log_file();
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts
        .open(&path)
        .map_err(|e| crate::error::Error::io(&path, e))?;
    fsutil::chmod_file_0600(&path)?;
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"));
    builder.target(env_logger::Target::Pipe(Box::new(file)));
    let _ = builder.try_init();
    Ok(())
}

#[cfg(test)]
mod tests {
    // NOTE: the old <5k LOC budget guard was removed by owner decision —
    // correctness and tests matter more than a line cap.
}
