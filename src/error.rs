use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("store.json version {found} is newer than this binary supports ({supported})")]
    UnsupportedStoreVersion { found: u32, supported: u32 },

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to parse {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
}

impl Error {
    pub fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            context: path.display().to_string(),
            source,
        }
    }

    pub fn json(path: &Path, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.to_path_buf(),
            source,
        }
    }

    pub fn toml(path: &Path, source: toml_edit::TomlError) -> Self {
        Self::Toml {
            path: path.to_path_buf(),
            source,
        }
    }

    /// 2 = I/O or network; 1 = user/validation (including unsupported schema).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Io { .. } | Self::Json { .. } | Self::Toml { .. } => 2,
            Self::UnsupportedStoreVersion { .. } => 1,
        }
    }
}

/// Map an anyhow chain to the CLI contract: 0 success, 1 user/validation, 2 I/O or network.
pub fn exit_code(err: &anyhow::Error) -> i32 {
    for cause in err.chain() {
        if let Some(e) = cause.downcast_ref::<Error>() {
            return e.exit_code();
        }
        if cause.downcast_ref::<io::Error>().is_some() {
            return 2;
        }
        if cause.downcast_ref::<reqwest::Error>().is_some() {
            return 2;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn user_errors_exit_1() {
        assert_eq!(exit_code(&anyhow::anyhow!("provider not found: x")), 1);
        assert_eq!(
            exit_code(&anyhow::anyhow!("ambiguous provider 'x': a, b")),
            1
        );
        assert_eq!(
            exit_code(&anyhow::Error::from(Error::UnsupportedStoreVersion {
                found: 9,
                supported: 1,
            })),
            1
        );
    }

    #[test]
    fn io_and_parse_errors_exit_2() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(
            exit_code(&anyhow::Error::from(Error::io(Path::new("/x"), io_err))),
            2
        );
        let json_err = serde_json::from_str::<i32>("nope").unwrap_err();
        assert_eq!(
            exit_code(&anyhow::Error::from(Error::json(
                Path::new("store.json"),
                json_err
            ))),
            2
        );
        let wrapped = anyhow::Error::from(Error::io(
            Path::new("/x"),
            io::Error::new(io::ErrorKind::PermissionDenied, "e"),
        ))
        .context("live config updated but failed to save store");
        assert_eq!(exit_code(&wrapped), 2);
        let raw_io = anyhow::Error::from(io::Error::new(io::ErrorKind::NotFound, "gone"))
            .context("create dir");
        assert_eq!(exit_code(&raw_io), 2);
    }

    #[test]
    fn reqwest_errors_exit_2() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(200))
                .build()
                .unwrap()
                .get("http://127.0.0.1:1/")
                .send()
                .await
                .unwrap_err()
        });
        assert_eq!(exit_code(&anyhow::Error::from(err)), 2);
    }
}
