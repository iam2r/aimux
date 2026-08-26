use std::sync::mpsc::{self, Receiver};

use anyhow::Result;

use crate::cloud;
use crate::paths::Paths;

pub fn syncing() -> &'static str {
    crate::i18n::t("ui.syncing")
}

pub enum Job {
    Setup {
        url: String,
        username: String,
        password: String,
    },
    Push,
    Pull,
}

pub enum Outcome {
    Setup(Result<()>),
    Push(Result<String>),
    Pull(Result<String>),
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
        Job::Push => Outcome::Push(cloud::push(paths, false)),
        Job::Pull => Outcome::Pull(cloud::pull_quiet(paths, false)),
    }
}
