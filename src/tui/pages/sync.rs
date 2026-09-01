use std::sync::mpsc::{self, Receiver};

use anyhow::Result;

use crate::cloud;
use crate::gist;
use crate::paths::Paths;

pub fn syncing() -> &'static str {
    crate::i18n::t("ui.syncing")
}

pub enum Job {
    /// WebDAV: URL / username / password.
    Setup {
        url: String,
        username: String,
        password: String,
    },
    /// Gist: fresh token (None = keep the stored one), optional pinned gist.
    GistSetup {
        token: Option<String>,
        gist: Option<String>,
    },
    Push,
    Pull,
    GistPush,
    GistPull,
}

pub enum Outcome {
    Setup(Result<()>),
    GistSetup(Result<String>),
    Push(Result<String>),
    Pull(Result<String>),
    GistPush(Result<String>),
    GistPull(Result<String>),
}

pub fn spawn(paths: Paths, job: Job) -> Receiver<Outcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run(&paths, job));
    });
    rx
}

fn run(paths: &Paths, job: Job) -> Outcome {
    match job {
        Job::Setup {
            url,
            username,
            password,
        } => Outcome::Setup(cloud::setup(paths, url, username, password)),
        Job::GistSetup { token, gist } => Outcome::GistSetup(gist::setup_with(paths, token, gist)),
        Job::Push => Outcome::Push(cloud::push(paths, false)),
        Job::Pull => Outcome::Pull(cloud::pull_quiet(paths, false)),
        Job::GistPush => Outcome::GistPush(gist::push(paths, false)),
        Job::GistPull => Outcome::GistPull(gist::pull_quiet(paths, false)),
    }
}
