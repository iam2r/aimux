#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn install_script() -> PathBuf {
    repo_root().join("install.sh")
}

fn write_exec(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

struct Harness {
    _temp: TempDir,
    home: PathBuf,
    fakebin: PathBuf,
    install_dir: PathBuf,
    logs_dir: PathBuf,
}

impl Harness {
    fn new(os: &str, arch: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let fakebin = temp.path().join("fakebin");
        let install_dir = temp.path().join("install");
        let logs_dir = temp.path().join("logs");
        let payload = temp.path().join("payload");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&fakebin).unwrap();
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&logs_dir).unwrap();
        fs::create_dir_all(&payload).unwrap();
        write_exec(
            &payload.join("apmux"),
            "#!/usr/bin/env bash\necho apmux-test-build\n",
        );

        let archive = temp.path().join("apmux.tar.gz");
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(&payload)
            .arg("apmux")
            .status()
            .unwrap();
        assert!(status.success());

        let uname = format!(
            r#"#!/usr/bin/env bash
set -eu
case "${{1:-}}" in
  -s) printf '{os}\n' ;;
  -m) printf '{arch}\n' ;;
  *) /usr/bin/uname "$@" ;;
esac
"#
        );
        write_exec(&fakebin.join("uname"), &uname);

        let curl = r#"#!/usr/bin/env bash
set -eu
output=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    --fail|--location|--silent|--show-error) shift ;;
    *) url="$1"; shift ;;
  esac
done
printf '%s' "$url" > "${APMUX_TEST_LOG_DIR}/last-url"
cp "${APMUX_TEST_ARCHIVE}" "$output"
"#;
        write_exec(&fakebin.join("curl"), curl);
        Self {
            _temp: temp,
            home,
            fakebin,
            install_dir,
            logs_dir,
        }
    }

    fn archive_path(&self) -> PathBuf {
        self._temp.path().join("apmux.tar.gz")
    }

    fn run(&self, extra: &[(&str, &str)], path_extra: &str) -> Output {
        let path = format!(
            "{}:{path_extra}:{}",
            self.fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new("bash");
        cmd.arg(install_script())
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("APMUX_INSTALL_DIR", &self.install_dir)
            .env("APMUX_TEST_LOG_DIR", &self.logs_dir)
            .env("APMUX_TEST_ARCHIVE", self.archive_path())
            .env("SHELL", "/bin/bash")
            .env("APMUX_SKIP_PATH", "1");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn linux_x64_downloads_musl_asset() {
    let h = Harness::new("Linux", "x86_64");
    let out = h.run(&[], "/usr/bin");
    assert_ok(&out);
    let url = fs::read_to_string(h.logs_dir.join("last-url")).unwrap();
    assert!(url.ends_with("apmux-linux-x64-musl.tar.gz"), "url={url}");
    let installed = h.install_dir.join("apmux");
    assert!(installed.is_file());
    let body = fs::read_to_string(&installed).unwrap();
    assert!(body.contains("apmux-test-build"));
}

#[test]
fn linux_arm64_and_darwin_assets() {
    let h = Harness::new("Linux", "aarch64");
    let out = h.run(&[], "/usr/bin");
    assert_ok(&out);
    let url = fs::read_to_string(h.logs_dir.join("last-url")).unwrap();
    assert!(url.ends_with("apmux-linux-arm64-musl.tar.gz"), "url={url}");

    let h = Harness::new("Darwin", "arm64");
    let out = h.run(&[], "/usr/bin");
    assert_ok(&out);
    let url = fs::read_to_string(h.logs_dir.join("last-url")).unwrap();
    assert!(url.ends_with("apmux-darwin-universal.tar.gz"), "url={url}");
}

#[test]
fn writes_path_block_when_install_dir_missing_from_path() {
    let h = Harness::new("Linux", "x86_64");
    let path = format!(
        "{}:{}",
        h.fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(install_script())
        .env("HOME", &h.home)
        .env("PATH", path)
        .env("APMUX_INSTALL_DIR", &h.install_dir)
        .env("APMUX_TEST_LOG_DIR", &h.logs_dir)
        .env("APMUX_TEST_ARCHIVE", h.archive_path())
        .env("SHELL", "/bin/bash")
        .output()
        .unwrap();
    assert_ok(&out);
    let rc = fs::read_to_string(h.home.join(".bashrc")).unwrap();
    assert!(rc.contains("# apmux PATH"), "{rc}");
    assert!(rc.contains(&h.install_dir.display().to_string()), "{rc}");
}

#[test]
fn skip_path_does_not_touch_rc() {
    let h = Harness::new("Linux", "x86_64");
    let out = h.run(&[("APMUX_SKIP_PATH", "1")], "/usr/bin");
    assert_ok(&out);
    assert!(!h.home.join(".bashrc").exists());
}

#[test]
fn version_arg_is_prefixed() {
    let h = Harness::new("Linux", "x86_64");
    let path = format!(
        "{}:{}",
        h.fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(install_script())
        .arg("0.1.0")
        .env("HOME", &h.home)
        .env("PATH", path)
        .env("APMUX_INSTALL_DIR", &h.install_dir)
        .env("APMUX_TEST_LOG_DIR", &h.logs_dir)
        .env("APMUX_TEST_ARCHIVE", h.archive_path())
        .env("SHELL", "/bin/bash")
        .env("APMUX_SKIP_PATH", "1")
        .output()
        .unwrap();
    assert_ok(&out);
    let url = fs::read_to_string(h.logs_dir.join("last-url")).unwrap();
    assert!(url.contains("/download/v0.1.0/"), "url={url}");
}
