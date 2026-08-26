use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::webdav;

const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
const BINARY_NAME: &str = crate::name::NAME;
const CHECKSUMS_FILE_NAME: &str = "checksums.txt";
const HTTP_TIMEOUT_SECS: u64 = 30;
const MAX_ASSET_BYTES: u64 = 100 * 1024 * 1024;
const GITHUB_API_ACCEPT: &str = "application/vnd.github+json";
const USER_AGENT_VALUE: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "-updater/",
    env!("CARGO_PKG_VERSION")
);

pub fn run(version: Option<String>, check: bool, json: bool) -> Result<()> {
    webdav::block_on(run_async(version, check, json))
}

async fn run_async(version: Option<String>, check: bool, json: bool) -> Result<()> {
    if check {
        return check_only(json).await;
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let explicit = version.as_deref().is_some_and(|v| !v.trim().is_empty());
    let homebrew = is_homebrew_install();

    if homebrew && explicit {
        println!(
            "aimux looks like a Homebrew install. Self-update to a specific version is not supported.\nPlease use: brew upgrade aimux"
        );
        return Ok(());
    }

    let client = http_client()?;
    let release = fetch_target_release(&client, REPO_URL, version.as_deref()).await?;
    let target_tag = release.tag_name.clone();
    let target_version = target_tag.trim_start_matches('v');

    if target_version == current_version {
        println!("Already on latest version: {current_version}");
        return Ok(());
    }

    if should_skip_implicit_downgrade(current_version, target_version, explicit) {
        println!(
            "Current version {current_version} is newer than target {target_tag}; skipping automatic downgrade. Use `aimux update --version {target_tag}` to force."
        );
        return Ok(());
    }

    if homebrew {
        println!(
            "Update {target_tag} is available (current {current_version}).\nPlease update with: brew upgrade aimux"
        );
        return Ok(());
    }

    println!("Current version: {current_version}");
    println!("Updating to: {target_tag}");

    let expected = current_asset_candidates()?;
    let asset = select_release_asset(&release.assets, &target_tag, &expected).ok_or_else(|| {
        anyhow!(
            "Release {target_tag} does not include any expected assets {expected:?} (or tagged variants)."
        )
    })?;

    println!("Downloading: {}", asset.browser_download_url);
    let downloaded = download_asset(&client, asset).await?;
    verify_checksum(&client, REPO_URL, &target_tag, asset, &downloaded.path).await?;
    let extracted = extract_binary(&downloaded.path)?;
    replace_current_binary(&extracted)?;

    println!("Updated successfully to {target_tag}");
    println!("Run `aimux --version` to verify the installed version.");
    Ok(())
}

async fn check_only(json: bool) -> Result<()> {
    let info = check_for_update(REPO_URL).await?;
    if json {
        let mut s = serde_json::to_string_pretty(&info)?;
        if !s.ends_with('\n') {
            s.push('\n');
        }
        print!("{s}");
        return Ok(());
    }
    if info.is_already_latest {
        println!("Already on latest version: {}", info.current_version);
    } else if info.is_homebrew_managed {
        println!(
            "Update {} is available (current {}).\nPlease update with: brew upgrade aimux",
            info.target_tag, info.current_version
        );
    } else if info.is_downgrade {
        println!(
            "Current version {} is newer than target {}; skipping automatic downgrade. Use `aimux update --version {}` to force.",
            info.current_version, info.target_tag, info.target_tag
        );
    } else {
        println!(
            "Update {} is available (current {}).",
            info.target_tag, info.current_version
        );
        println!("Run `aimux update` to download and apply it.");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckInfo {
    pub current_version: String,
    pub target_tag: String,
    pub is_already_latest: bool,
    pub is_downgrade: bool,
    pub is_homebrew_managed: bool,
}

async fn check_for_update(repo_url: &str) -> Result<UpdateCheckInfo> {
    let client = http_client()?;
    let release = fetch_target_release(&client, repo_url, None).await?;
    Ok(build_update_check_info(
        env!("CARGO_PKG_VERSION"),
        release.tag_name,
        is_homebrew_install(),
    ))
}

fn build_update_check_info(
    current_version: &str,
    target_tag: String,
    is_homebrew_managed: bool,
) -> UpdateCheckInfo {
    let target_version = target_tag.trim_start_matches('v').to_string();
    UpdateCheckInfo {
        current_version: current_version.to_string(),
        is_already_latest: target_version == current_version,
        is_downgrade: should_skip_implicit_downgrade(current_version, &target_version, false),
        is_homebrew_managed,
        target_tag,
    }
}

#[derive(Debug, Deserialize, Clone)]
struct ReleaseInfo {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

struct DownloadedAsset {
    _temp_dir: tempfile::TempDir,
    path: PathBuf,
}

fn http_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    if let Some(token) = github_token() {
        let value = format!("Bearer {token}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&value).context("invalid GitHub token")?,
        );
    }
    Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context("http client")
}

fn github_token() -> Option<String> {
    for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn fetch_target_release(
    client: &Client,
    repo_url: &str,
    version: Option<&str>,
) -> Result<ReleaseInfo> {
    match version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(version) => {
            let tag = normalize_tag(version);
            validate_target_tag(&tag)?;
            fetch_release_by_tag(client, repo_url, &tag).await
        }
        None => fetch_latest_release(client, repo_url).await,
    }
}

async fn fetch_latest_release(client: &Client, repo_url: &str) -> Result<ReleaseInfo> {
    let url = release_api_url(repo_url, "latest")?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, GITHUB_API_ACCEPT)
        .send()
        .await
        .context("query latest release")?;

    if matches!(
        response.status(),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        let tag = fetch_latest_tag_from_release_page(client, repo_url).await?;
        return fetch_release_by_tag(client, repo_url, &tag).await;
    }

    response
        .error_for_status()
        .context("latest release API")?
        .json::<ReleaseInfo>()
        .await
        .context("parse latest release")
}

async fn fetch_release_by_tag(client: &Client, repo_url: &str, tag: &str) -> Result<ReleaseInfo> {
    let url = release_api_url(repo_url, &format!("tags/{tag}"))?;
    client
        .get(url)
        .header(reqwest::header::ACCEPT, GITHUB_API_ACCEPT)
        .send()
        .await
        .with_context(|| format!("query release {tag}"))?
        .error_for_status()
        .with_context(|| format!("release {tag} not found"))?
        .json::<ReleaseInfo>()
        .await
        .with_context(|| format!("parse release {tag}"))
}

async fn fetch_latest_tag_from_release_page(client: &Client, repo_url: &str) -> Result<String> {
    let url = release_page_url(repo_url, "latest")?;
    let response = client
        .get(url)
        .send()
        .await
        .context("query latest release page")?
        .error_for_status()
        .context("latest release page")?;
    extract_release_tag_from_url(response.url()).ok_or_else(|| {
        anyhow!(
            "Failed to resolve latest release tag from {}.",
            response.url()
        )
    })
}

fn repo_owner_name(repo_url: &str) -> Result<(Url, String, String)> {
    let parsed =
        Url::parse(repo_url).with_context(|| format!("invalid repository URL {repo_url}"))?;
    let path = parsed.path().trim_matches('/').to_string();
    let mut parts = path.split('/');
    let owner = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("repository URL must include owner and repo: {repo_url}"))?
        .to_string();
    let repo = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("repository URL must include owner and repo: {repo_url}"))?;
    if parts.next().is_some() {
        bail!("repository URL must be in '<host>/<owner>/<repo>' format: {repo_url}");
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo).to_string();
    Ok((parsed, owner, repo))
}

fn release_page_url(repo_url: &str, suffix: &str) -> Result<Url> {
    let (mut url, owner, repo) = repo_owner_name(repo_url)?;
    url.set_path(&format!("/{owner}/{repo}/releases/{suffix}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn release_api_url(repo_url: &str, suffix: &str) -> Result<Url> {
    let (mut url, owner, repo) = repo_owner_name(repo_url)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("repository URL is missing host: {repo_url}"))?;
    if host == "github.com" {
        url.set_host(Some("api.github.com"))
            .map_err(|_| anyhow!("failed to set GitHub API host"))?;
        url.set_path(&format!("/repos/{owner}/{repo}/releases/{suffix}"));
    } else {
        url.set_path(&format!("/api/v3/repos/{owner}/{repo}/releases/{suffix}"));
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn extract_release_tag_from_url(url: &Url) -> Option<String> {
    let segments: Vec<&str> = url.path_segments()?.collect();
    segments
        .windows(3)
        .find(|window| window[0] == "releases" && window[1] == "tag")
        .map(|window| window[2].to_string())
}

fn normalize_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

fn validate_target_tag(tag: &str) -> Result<()> {
    if !tag.starts_with('v') {
        bail!("Invalid version tag '{tag}': must start with 'v'.");
    }
    if tag.len() > 64 {
        bail!("Invalid version tag '{tag}': too long.");
    }
    if tag.contains('/') || tag.contains('\\') || tag.contains("..") {
        bail!("Invalid version tag '{tag}': contains forbidden path characters.");
    }
    if !tag
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_')
    {
        bail!("Invalid version tag '{tag}': only [A-Za-z0-9._-] allowed.");
    }
    Ok(())
}

fn current_asset_candidates() -> Result<Vec<String>> {
    asset_candidates(std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_candidates(os: &str, arch: &str) -> Result<Vec<String>> {
    let names = match (os, arch) {
        ("macos", "x86_64") => vec![
            "aimux-darwin-universal.tar.gz".into(),
            "aimux-darwin-x64.tar.gz".into(),
        ],
        ("macos", "aarch64") => vec![
            "aimux-darwin-universal.tar.gz".into(),
            "aimux-darwin-arm64.tar.gz".into(),
        ],
        ("linux", "x86_64") => vec!["aimux-linux-x64-musl.tar.gz".into()],
        ("linux", "aarch64") => vec!["aimux-linux-arm64-musl.tar.gz".into()],
        ("windows", "x86_64") => vec!["aimux-windows-x64.zip".into()],
        _ => bail!("Self-update is not supported for platform {os}/{arch}."),
    };
    Ok(names)
}

fn tagged_asset_name(tag: &str, asset_name: &str) -> String {
    if let Some(suffix) = asset_name.strip_prefix("aimux-") {
        format!("aimux-{tag}-{suffix}")
    } else {
        asset_name.to_string()
    }
}

fn release_asset_names(tag: &str, asset_name: &str) -> Vec<String> {
    let tagged = tagged_asset_name(tag, asset_name);
    if tagged == asset_name {
        vec![asset_name.to_string()]
    } else {
        vec![asset_name.to_string(), tagged]
    }
}

fn select_release_asset<'a>(
    assets: &'a [ReleaseAsset],
    target_tag: &str,
    expected_asset_names: &[String],
) -> Option<&'a ReleaseAsset> {
    expected_asset_names.iter().find_map(|expected| {
        let names = release_asset_names(target_tag, expected);
        names
            .iter()
            .find_map(|name| assets.iter().find(|asset| asset.name == *name))
    })
}

async fn download_asset(client: &Client, asset: &ReleaseAsset) -> Result<DownloadedAsset> {
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("download release asset")?;
    let response = response
        .error_for_status()
        .context("release asset request")?;
    if let Some(len) = response.content_length() {
        validate_size(len, &asset.name)?;
    }
    let bytes = response.bytes().await.context("read release asset")?;
    validate_size(bytes.len() as u64, &asset.name)?;

    let temp_dir = tempfile::tempdir().context("temp directory")?;
    let file_name = sanitized_file_name(&asset.name)?;
    let path = temp_dir.path().join(file_name);
    fs::write(&path, &bytes).map_err(|e| Error::io(&path, e))?;
    Ok(DownloadedAsset {
        _temp_dir: temp_dir,
        path,
    })
}

fn sanitized_file_name(asset_name: &str) -> Result<&str> {
    Path::new(asset_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .ok_or_else(|| anyhow!("Invalid asset name: {asset_name}"))
}

fn validate_size(size_bytes: u64, asset_name: &str) -> Result<()> {
    if size_bytes <= MAX_ASSET_BYTES {
        return Ok(());
    }
    let max_mb = MAX_ASSET_BYTES / (1024 * 1024);
    let size_mb = size_bytes / (1024 * 1024);
    bail!(
        "Release asset '{asset_name}' is too large ({size_mb} MB). Maximum allowed size is {max_mb} MB."
    );
}

async fn verify_checksum(
    client: &Client,
    repo_url: &str,
    target_tag: &str,
    asset: &ReleaseAsset,
    archive_path: &Path,
) -> Result<()> {
    let actual = sha256_file(archive_path)?;
    let expected = if let Some(digest) = asset.digest.as_deref().and_then(parse_sha256_digest) {
        println!("Verifying checksum from release metadata digest.");
        digest
    } else {
        let checksum_url = release_page_url(
            repo_url,
            &format!("download/{target_tag}/{CHECKSUMS_FILE_NAME}"),
        )?;
        println!("Verifying checksum: {checksum_url}");
        let body = client
            .get(checksum_url)
            .send()
            .await
            .context("download checksums.txt")?
            .error_for_status()
            .context("checksums.txt request")?
            .text()
            .await
            .context("read checksums.txt")?;
        parse_checksum_for_asset(&body, &asset.name)?
    };
    if actual != expected {
        bail!(
            "Checksum mismatch for {}: expected {expected}, got {actual}.",
            asset.name
        );
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|e| Error::io(path, e))?;
    Ok(hex::encode(hasher.finalize()))
}

fn parse_checksum_for_asset(checksum_content: &str, asset_name: &str) -> Result<String> {
    checksum_content
        .lines()
        .filter_map(|line| {
            let (hash, file) = parse_sha256sum_line(line.trim_end())?;
            if file == asset_name {
                Some(hash.to_ascii_lowercase())
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| anyhow!("Unable to find SHA256 for {asset_name} in {CHECKSUMS_FILE_NAME}."))
}

fn parse_sha256sum_line(line: &str) -> Option<(&str, &str)> {
    if line.len() < 66 {
        return None;
    }
    let (hash, remainder) = line.split_at(64);
    if !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    remainder
        .strip_prefix("  ")
        .or_else(|| remainder.strip_prefix(" *"))
        .map(|file| (hash, file))
}

fn parse_sha256_digest(digest: &str) -> Option<String> {
    let digest = digest.strip_prefix("sha256:")?;
    if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(digest.to_ascii_lowercase())
}

fn extract_binary(archive_path: &Path) -> Result<PathBuf> {
    let extract_dir = archive_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid archive path"))?
        .join("extracted");
    fs::create_dir_all(&extract_dir).map_err(|e| Error::io(&extract_dir, e))?;
    #[cfg(windows)]
    {
        extract_zip_binary(archive_path, &extract_dir)
    }
    #[cfg(not(windows))]
    {
        extract_tar_binary(archive_path, &extract_dir)
    }
}

#[cfg(not(windows))]
fn extract_tar_binary(archive_path: &Path, extract_dir: &Path) -> Result<PathBuf> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let file = fs::File::open(archive_path).map_err(|e| Error::io(archive_path, e))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    for entry in archive.entries().context("read archive entries")? {
        let mut entry = entry.context("read archive entry")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry.path().context("archive entry path")?;
        if entry_path.file_name().and_then(|n| n.to_str()) != Some(BINARY_NAME) {
            continue;
        }
        let binary_path = extract_dir.join(BINARY_NAME);
        let mut output = fs::File::create(&binary_path).map_err(|e| Error::io(&binary_path, e))?;
        io::copy(&mut entry, &mut output).context("unpack binary from tar")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o755))
                .map_err(|e| Error::io(&binary_path, e))?;
        }
        return Ok(binary_path);
    }
    bail!("Extracted archive does not contain expected binary: {BINARY_NAME}");
}

#[cfg(windows)]
fn extract_zip_binary(archive_path: &Path, extract_dir: &Path) -> Result<PathBuf> {
    let file = fs::File::open(archive_path).map_err(|e| Error::io(archive_path, e))?;
    let mut zip = zip::ZipArchive::new(file).context("open ZIP archive")?;
    let binary_filename = format!("{BINARY_NAME}.exe");
    let mut entry = zip
        .by_name(&binary_filename)
        .map_err(|_| anyhow!("ZIP archive does not contain {binary_filename}"))?;
    let binary_path = extract_dir.join(&binary_filename);
    let mut output = fs::File::create(&binary_path).map_err(|e| Error::io(&binary_path, e))?;
    io::copy(&mut entry, &mut output).context("extract binary from ZIP")?;
    Ok(binary_path)
}

fn replace_current_binary(new_binary_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        self_replace::self_replace(new_binary_path)
            .context("Failed to replace running executable on Windows")
    }
    #[cfg(not(windows))]
    {
        let current = std::env::current_exe().context("resolve current executable path")?;
        replace_unix_binary(new_binary_path, &current)
    }
}

#[cfg(not(windows))]
fn replace_unix_binary(new_binary_path: &Path, current_binary: &Path) -> Result<()> {
    let parent = current_binary
        .parent()
        .ok_or_else(|| anyhow!("Current executable path has no parent directory."))?;
    let staged = parent.join(format!("{BINARY_NAME}.new"));
    match fs::remove_file(&staged) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(Error::io(&staged, err).into()),
    }
    fs::copy(new_binary_path, &staged).map_err(|e| map_permission(&staged, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map_err(|e| map_permission(&staged, e))?;
    }
    fs::rename(&staged, current_binary).map_err(|e| map_permission(current_binary, e))?;
    Ok(())
}

#[cfg(not(windows))]
fn map_permission(target: &Path, err: io::Error) -> anyhow::Error {
    if err.kind() == io::ErrorKind::PermissionDenied {
        anyhow!(
            "Permission denied while updating {}. Re-run with elevated privileges (for example: sudo aimux update), or reinstall with install.sh / install.ps1.",
            target.display()
        )
    } else {
        Error::io(target, err).into()
    }
}

fn is_homebrew_install() -> bool {
    #[cfg(windows)]
    {
        false
    }
    #[cfg(not(windows))]
    {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return false,
        };
        if let Ok(prefix) = std::env::var("HOMEBREW_PREFIX") {
            if exe.starts_with(&prefix) {
                return true;
            }
        }
        ["/opt/homebrew", "/home/linuxbrew/.linuxbrew"]
            .iter()
            .any(|prefix| exe.starts_with(prefix))
    }
}

fn should_skip_implicit_downgrade(
    current_version: &str,
    target_version: &str,
    explicit_version: bool,
) -> bool {
    if explicit_version {
        return false;
    }
    match (
        parse_version_nums(current_version),
        parse_version_nums(target_version),
    ) {
        (Some(current), Some(target)) => target < current,
        _ => false,
    }
}

fn parse_version_nums(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_validate_tags() {
        assert_eq!(normalize_tag("0.2.0"), "v0.2.0");
        assert_eq!(normalize_tag("v0.2.0"), "v0.2.0");
        validate_target_tag("v0.2.0").unwrap();
        validate_target_tag("v1.0.0-rc.1").unwrap();
        assert!(validate_target_tag("0.2.0").is_err());
        assert!(validate_target_tag("v../etc").is_err());
        assert!(validate_target_tag("vfoo/bar").is_err());
    }

    #[test]
    fn asset_names_prefer_universal_macos_and_musl_linux() {
        assert_eq!(
            asset_candidates("macos", "aarch64").unwrap(),
            vec![
                "aimux-darwin-universal.tar.gz".to_string(),
                "aimux-darwin-arm64.tar.gz".to_string()
            ]
        );
        assert_eq!(
            asset_candidates("linux", "x86_64").unwrap(),
            vec!["aimux-linux-x64-musl.tar.gz".to_string()]
        );
        assert_eq!(
            asset_candidates("windows", "x86_64").unwrap(),
            vec!["aimux-windows-x64.zip".to_string()]
        );
        assert!(asset_candidates("linux", "riscv64").is_err());
    }

    #[test]
    fn tagged_and_plain_asset_names_are_both_accepted() {
        let assets = vec![
            ReleaseAsset {
                name: "aimux-v0.2.0-linux-x64-musl.tar.gz".into(),
                browser_download_url: "https://example.invalid/a".into(),
                digest: None,
            },
            ReleaseAsset {
                name: "checksums.txt".into(),
                browser_download_url: "https://example.invalid/c".into(),
                digest: None,
            },
        ];
        let expected = vec!["aimux-linux-x64-musl.tar.gz".to_string()];
        let selected = select_release_asset(&assets, "v0.2.0", &expected).unwrap();
        assert_eq!(selected.name, "aimux-v0.2.0-linux-x64-musl.tar.gz");
    }

    #[test]
    fn prefers_untagged_asset_name() {
        let assets = vec![
            ReleaseAsset {
                name: "aimux-linux-x64-musl.tar.gz".into(),
                browser_download_url: "https://example.invalid/a".into(),
                digest: None,
            },
            ReleaseAsset {
                name: "aimux-v0.2.0-linux-x64-musl.tar.gz".into(),
                browser_download_url: "https://example.invalid/b".into(),
                digest: None,
            },
        ];
        let expected = vec!["aimux-linux-x64-musl.tar.gz".to_string()];
        let selected = select_release_asset(&assets, "v0.2.0", &expected).unwrap();
        assert_eq!(selected.name, "aimux-linux-x64-musl.tar.gz");
    }

    #[test]
    fn checksum_parser_accepts_text_and_binary_sha256sum() {
        let body = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  aimux-linux-x64-musl.tar.gz\n\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *checksums.txt\n";
        assert_eq!(
            parse_checksum_for_asset(body, "aimux-linux-x64-musl.tar.gz").unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            parse_sha256_digest(
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            )
            .unwrap(),
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert!(parse_sha256_digest("md5:deadbeef").is_none());
    }

    #[test]
    fn implicit_downgrade_is_skipped_unless_version_is_explicit() {
        assert!(should_skip_implicit_downgrade("0.3.0", "0.2.0", false));
        assert!(!should_skip_implicit_downgrade("0.3.0", "0.2.0", true));
        assert!(!should_skip_implicit_downgrade("0.2.0", "0.3.0", false));
        assert!(!should_skip_implicit_downgrade("0.2.0", "0.2.0", false));
    }

    #[test]
    fn update_check_json_uses_cli_field_names() {
        let info = build_update_check_info("1.2.3", "v1.2.4".into(), false);
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["currentVersion"], "1.2.3");
        assert_eq!(value["targetTag"], "v1.2.4");
        assert_eq!(value["isAlreadyLatest"], false);
        assert_eq!(value["isDowngrade"], false);
        assert_eq!(value["isHomebrewManaged"], false);
    }

    #[test]
    fn repo_urls_map_to_github_api_and_pages() {
        let api = release_api_url("https://github.com/iam2r/aimux", "latest").unwrap();
        assert_eq!(
            api.as_str(),
            "https://api.github.com/repos/iam2r/aimux/releases/latest"
        );
        let page = release_page_url(
            "https://github.com/iam2r/aimux.git",
            "download/v0.2.0/checksums.txt",
        )
        .unwrap();
        assert_eq!(
            page.as_str(),
            "https://github.com/iam2r/aimux/releases/download/v0.2.0/checksums.txt"
        );
        let tag_url = Url::parse("https://github.com/iam2r/aimux/releases/tag/v0.2.0").unwrap();
        assert_eq!(
            extract_release_tag_from_url(&tag_url).as_deref(),
            Some("v0.2.0")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn extracts_binary_from_tar_gz() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("aimux-linux-x64-musl.tar.gz");
        write_tar_gz(&archive, BINARY_NAME, b"fake-aimux-bytes");
        let extracted = extract_binary(&archive).unwrap();
        assert_eq!(fs::read(&extracted).unwrap(), b"fake-aimux-bytes");
        assert_eq!(extracted.file_name().unwrap(), BINARY_NAME);
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_replace_renames_over_destination() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("aimux");
        let src = temp.path().join("new");
        fs::write(&dest, b"old").unwrap();
        fs::write(&src, b"new-bytes").unwrap();
        replace_unix_binary(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new-bytes");
        assert!(!temp.path().join("aimux.new").exists());
    }

    #[cfg(not(windows))]
    fn write_tar_gz(path: &Path, name: &str, data: &[u8]) {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        let file = fs::File::create(path).unwrap();
        let enc = GzEncoder::new(file, Compression::fast());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
        builder.finish().unwrap();
    }
}
