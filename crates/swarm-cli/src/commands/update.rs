//! `swarm update` sub-command.

use std::{
    cmp::Ordering,
    env::consts::{ARCH, OS},
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, bail, Context};
use clap::Args;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use swarm_config::SwarmConfig;
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;

const REPO_OWNER: &str = "JosunLP";
const REPO_NAME: &str = "AiOfficeSwarm";
const BIN_NAME: &str = "swarm";
const CHECKSUMS_ASSET_NAME: &str = "SHA256SUMS";
const GITHUB_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const GITHUB_READ_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Update the installed `swarm` binary from GitHub releases.
#[derive(Args)]
pub struct UpdateArgs {
    /// Only report whether an update is available.
    #[arg(long)]
    pub check: bool,

    /// Install a specific released version instead of the latest one.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn run(args: UpdateArgs, _config: &SwarmConfig) -> anyhow::Result<()> {
    let client = github_client()?;
    let release = fetch_release(&client, args.version.as_deref()).await?;
    let current_version = env!("CARGO_PKG_VERSION");
    let release_version = normalize_version(&release.tag_name);
    let version_comparison = compare_versions(&release.tag_name, current_version);

    if args.check {
        println!(
            "{}",
            format_check_report(
                current_version,
                release_version,
                &release.html_url,
                &release.tag_name,
                version_comparison,
            )
        );
        return Ok(());
    }

    if args.version.is_none() {
        match version_comparison {
            Some(Ordering::Equal) => {
                println!("swarm is already up to date (version {}).", current_version);
                return Ok(());
            }
            Some(Ordering::Less) => {
                bail!(
                    "The latest published version ({}) is older than the running build version ({}).",
                    release_version,
                    current_version
                );
            }
            _ => {}
        }
    }

    let target = current_target_triple()?;
    let asset_name = asset_name_for_target(target);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| {
            format!(
                "No release asset '{}' was found for target '{}'.",
                asset_name, target
            )
        })?;

    println!("Downloading {} for {}...", release.tag_name, target);

    let temp_dir = tempfile::tempdir().context("Failed to create temporary directory")?;
    let archive_path = temp_dir.path().join(&asset.name);
    download_asset(&client, &asset.browser_download_url, &archive_path).await?;
    verify_asset_checksum(
        &client,
        &release,
        &asset.name,
        &archive_path,
        temp_dir.path(),
    )
    .await?;
    let binary_path = extract_binary(&archive_path, temp_dir.path())?;

    self_replace::self_replace(&binary_path)
        .with_context(|| format!("Failed to replace '{}' with the new version", BIN_NAME))?;

    println!(
        "swarm was successfully updated from {} to {}.",
        current_version, release_version
    );
    println!("Release notes: {}", release.html_url);

    Ok(())
}

fn github_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("{BIN_NAME}/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(GITHUB_CONNECT_TIMEOUT)
        .read_timeout(GITHUB_READ_TIMEOUT)
        .timeout(GITHUB_REQUEST_TIMEOUT)
        .build()
        .context("Failed to create GitHub HTTP client")
}

async fn fetch_release(
    client: &reqwest::Client,
    version: Option<&str>,
) -> anyhow::Result<GitHubRelease> {
    client
        .get(release_api_url(version)?)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("Failed to load GitHub release metadata")?
        .error_for_status()
        .context("Failed to load GitHub release metadata")?
        .json::<GitHubRelease>()
        .await
        .context("Failed to parse GitHub release response")
}

fn release_api_url(version: Option<&str>) -> anyhow::Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(&format!(
        "https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases"
    ))
    .context("Failed to construct GitHub releases API URL")?;
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| anyhow!("Failed to construct GitHub releases API path"))?;

    if let Some(version) = version {
        segments.push("tags");
        segments.push(&normalize_tag(version));
    } else {
        segments.push("latest");
    }

    drop(segments);
    Ok(url)
}

async fn download_asset(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download '{}'", url))?
        .error_for_status()
        .with_context(|| format!("Failed to download '{}'", url))?;

    let mut file = tokio::fs::File::create(destination)
        .await
        .with_context(|| format!("Failed to create archive '{}'", destination.display()))?;

    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("Failed to read response body from '{}'", url))?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("Failed to write archive '{}'", destination.display()))?;
    }

    file.flush()
        .await
        .with_context(|| format!("Failed to flush archive '{}'", destination.display()))
}

async fn verify_asset_checksum(
    client: &reqwest::Client,
    release: &GitHubRelease,
    asset_name: &str,
    archive_path: &Path,
    temp_root: &Path,
) -> anyhow::Result<()> {
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == CHECKSUMS_ASSET_NAME)
        .with_context(|| {
            format!(
                "No release asset '{}' was found for checksum verification.",
                CHECKSUMS_ASSET_NAME
            )
        })?;
    let checksums_path = temp_root.join(CHECKSUMS_ASSET_NAME);
    download_asset(
        client,
        &checksum_asset.browser_download_url,
        &checksums_path,
    )
    .await?;

    let checksums = fs::read_to_string(&checksums_path).with_context(|| {
        format!(
            "Failed to read checksum file '{}'",
            checksums_path.display()
        )
    })?;
    let expected_checksum = checksum_for_asset(&checksums, asset_name).with_context(|| {
        format!(
            "Failed to find a checksum entry for '{}' in '{}'",
            asset_name,
            checksums_path.display()
        )
    })?;
    let actual_checksum = sha256_digest(archive_path)?;

    if actual_checksum != expected_checksum {
        bail!(
            "Checksum verification failed for '{}': expected {}, got {}",
            asset_name,
            expected_checksum,
            actual_checksum
        );
    }

    Ok(())
}

fn checksum_for_asset(checksums: &str, asset_name: &str) -> anyhow::Result<String> {
    checksums
        .lines()
        .filter_map(parse_checksum_line)
        .find_map(|(checksum, name)| (name == asset_name).then_some(checksum))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| anyhow!("checksum entry not found"))
}

fn parse_checksum_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let checksum = parts.next()?;
    let asset_name = parts.next()?.trim_start_matches('*');
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    Some((checksum, asset_name))
}

fn sha256_digest(path: &Path) -> anyhow::Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open '{}'", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read '{}'", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_binary(archive_path: &Path, temp_root: &Path) -> anyhow::Result<PathBuf> {
    let extract_dir = temp_root.join("extract");
    fs::create_dir_all(&extract_dir).with_context(|| {
        format!(
            "Failed to create extraction directory '{}'",
            extract_dir.display()
        )
    })?;

    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Failed to determine archive name"))?;

    if archive_name.ends_with(".zip") {
        extract_zip_archive(archive_path, &extract_dir)?;
    } else if archive_name.ends_with(".tar.gz") {
        extract_tar_gz_archive(archive_path, &extract_dir)?;
    } else {
        bail!("Unsupported archive format: {}", archive_name);
    }

    let binary_name = executable_name();
    WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_type().is_file() && entry.file_name().to_string_lossy() == binary_name
        })
        .map(|entry| entry.into_path())
        .with_context(|| format!("The file '{}' was not found in the archive", binary_name))
}

fn extract_tar_gz_archive(archive_path: &Path, extract_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive '{}'", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    for (index, entry) in archive
        .entries()
        .with_context(|| format!("Failed to read archive '{}'", archive_path.display()))?
        .enumerate()
    {
        let mut entry = entry.with_context(|| {
            format!(
                "Failed to read tar entry #{} from '{}'",
                index,
                archive_path.display()
            )
        })?;
        let entry_path = entry
            .path()
            .with_context(|| {
                format!(
                    "Failed to read tar entry path #{} from '{}'",
                    index,
                    archive_path.display()
                )
            })?
            .into_owned();
        validate_relative_archive_path(&entry_path).with_context(|| {
            format!(
                "Archive '{}' contains an unsafe path '{}'",
                archive_path.display(),
                entry_path.display()
            )
        })?;
        validate_tar_entry_type(entry.header().entry_type()).with_context(|| {
            format!(
                "Archive '{}' contains an unsupported entry type for '{}'",
                archive_path.display(),
                entry_path.display()
            )
        })?;

        let unpacked = entry.unpack_in(extract_dir).with_context(|| {
            format!(
                "Failed to extract '{}' from archive '{}'",
                entry_path.display(),
                archive_path.display()
            )
        })?;
        if !unpacked {
            bail!(
                "Archive '{}' contains an entry outside the extraction directory: '{}'",
                archive_path.display(),
                entry_path.display()
            );
        }
    }

    Ok(())
}

fn extract_zip_archive(archive_path: &Path, extract_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive '{}'", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Failed to read ZIP archive '{}'", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Failed to read ZIP entry #{}", index))?;
        let Some(relative_path) = entry.enclosed_name().map(|path| path.to_owned()) else {
            anyhow::bail!(
                "ZIP archive contains an unsafe path '{}'; aborting update",
                entry.name()
            );
        };

        let output_path = extract_dir.join(relative_path);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&output_path).with_context(|| {
                format!("Failed to create directory '{}'", output_path.display())
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory '{}'", parent.display()))?;
        }

        let mut output_file = File::create(&output_path)
            .with_context(|| format!("Failed to create file '{}'", output_path.display()))?;
        io::copy(&mut entry, &mut output_file)
            .with_context(|| format!("Failed to write file '{}'", output_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = entry.unix_mode() {
                fs::set_permissions(&output_path, fs::Permissions::from_mode(mode)).with_context(
                    || format!("Failed to set permissions for '{}'", output_path.display()),
                )?;
            }
        }
    }

    Ok(())
}

fn validate_relative_archive_path(path: &Path) -> anyhow::Result<()> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("archive entry escapes the extraction directory");
    }

    Ok(())
}

fn validate_tar_entry_type(entry_type: tar::EntryType) -> anyhow::Result<()> {
    if entry_type.is_file() || entry_type.is_dir() {
        return Ok(());
    }

    bail!(
        "tar entry type '{}' is not supported for self-update extraction",
        tar_entry_type_label(entry_type)
    )
}

fn tar_entry_type_label(entry_type: tar::EntryType) -> String {
    let raw = entry_type.as_byte();
    let kind = if entry_type.is_symlink() {
        "symlink"
    } else if entry_type.is_hard_link() {
        "hardlink"
    } else if entry_type.is_pax_global_extensions() {
        "pax-global-extensions"
    } else if entry_type.is_pax_local_extensions() {
        "pax-local-extensions"
    } else if entry_type.is_gnu_longname() {
        "gnu-longname"
    } else if entry_type.is_gnu_longlink() {
        "gnu-longlink"
    } else {
        "special"
    };

    format!("{kind} (byte 0x{raw:02x})")
}

fn current_target_triple() -> anyhow::Result<&'static str> {
    supported_target_triple(OS, ARCH)
}

fn supported_target_triple(os: &str, arch: &str) -> anyhow::Result<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => {
            bail!("Self-updates are not published for Linux ARM64 yet")
        }
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => {
            bail!("Self-updates are not published for Windows ARM64 yet")
        }
        (os, arch) => bail!(
            "Self-updates are not supported on platform '{}' with architecture '{}'",
            os,
            arch
        ),
    }
}

fn asset_name_for_target(target: &str) -> String {
    if target.contains("windows") {
        format!("{BIN_NAME}-{target}.zip")
    } else {
        format!("{BIN_NAME}-{target}.tar.gz")
    }
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "swarm.exe"
    } else {
        BIN_NAME
    }
}

fn normalize_version(version: &str) -> &str {
    let trimmed = version.trim();
    match trimmed.strip_prefix('v') {
        Some(stripped) if Version::parse(stripped).is_ok() => stripped,
        _ => trimmed,
    }
}

fn normalize_tag(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        trimmed.to_owned()
    } else if Version::parse(trimmed).is_ok() {
        format!("v{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    match (
        Version::parse(normalize_version(left)),
        Version::parse(normalize_version(right)),
    ) {
        (Ok(left), Ok(right)) => Some(left.cmp(&right)),
        _ => None,
    }
}

fn format_check_report(
    current_version: &str,
    release_version: &str,
    release_url: &str,
    release_tag: &str,
    version_comparison: Option<Ordering>,
) -> String {
    let status = match version_comparison {
        Some(Ordering::Greater) => "An update is available.",
        Some(Ordering::Equal) => "No update required.",
        Some(Ordering::Less) => {
            "No update required: the running build is newer than the latest published release."
        }
        None => return format!(
            "Current version: {}\nAvailable version: {}\nRelease: {}\nUnable to compare versions automatically for release tag '{}'. Try `swarm update --version {}` to target that release explicitly.",
            current_version, release_version, release_url, release_tag, release_tag
        ),
    };

    format!(
        "Current version: {}\nAvailable version: {}\nRelease: {}\n{}",
        current_version, release_version, release_url, status
    )
}

#[cfg(test)]
mod tests {
    use super::{
        asset_name_for_target, checksum_for_asset, compare_versions, format_check_report,
        normalize_tag, normalize_version, parse_checksum_line, release_api_url,
        supported_target_triple, validate_relative_archive_path, validate_tar_entry_type,
    };
    use std::{cmp::Ordering, path::Path};

    #[test]
    fn normalizes_versions_and_tags() {
        assert_eq!(normalize_version("v0.2.1"), "0.2.1");
        assert_eq!(normalize_version("0.2.1"), "0.2.1");
        assert_eq!(normalize_version("vnext"), "vnext");
        assert_eq!(normalize_tag("0.2.1"), "v0.2.1");
        assert_eq!(normalize_tag("v0.2.1"), "v0.2.1");
        assert_eq!(normalize_tag("release-2026-03-18"), "release-2026-03-18");
    }

    #[test]
    fn compares_semver_values_with_or_without_prefix() {
        assert_eq!(compare_versions("v1.2.0", "1.1.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v1.2.0", "1.2.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.1.9", "v1.2.0"), Some(Ordering::Less));
    }

    #[test]
    fn formats_check_report_for_all_version_states() {
        assert_eq!(
            format_check_report(
                "1.0.0",
                "1.2.0",
                "https://example.invalid/releases/v1.2.0",
                "v1.2.0",
                Some(Ordering::Greater)
            ),
            "Current version: 1.0.0\nAvailable version: 1.2.0\nRelease: https://example.invalid/releases/v1.2.0\nAn update is available."
        );
        assert_eq!(
            format_check_report(
                "1.2.0",
                "1.2.0",
                "https://example.invalid/releases/v1.2.0",
                "v1.2.0",
                Some(Ordering::Equal)
            ),
            "Current version: 1.2.0\nAvailable version: 1.2.0\nRelease: https://example.invalid/releases/v1.2.0\nNo update required."
        );
        assert_eq!(
            format_check_report(
                "1.3.0",
                "1.2.0",
                "https://example.invalid/releases/v1.2.0",
                "v1.2.0",
                Some(Ordering::Less)
            ),
            "Current version: 1.3.0\nAvailable version: 1.2.0\nRelease: https://example.invalid/releases/v1.2.0\nNo update required: the running build is newer than the latest published release."
        );
        assert_eq!(
            format_check_report(
                "1.0.0",
                "vnext",
                "https://example.invalid/releases/vnext",
                "vnext",
                None
            ),
            "Current version: 1.0.0\nAvailable version: vnext\nRelease: https://example.invalid/releases/vnext\nUnable to compare versions automatically for release tag 'vnext'. Try `swarm update --version vnext` to target that release explicitly."
        );
        assert_eq!(
            format_check_report(
                "1.0.0",
                "not-a-semver-tag",
                "https://example.invalid/releases/not-a-semver-tag",
                "not-a-semver-tag",
                None
            ),
            "Current version: 1.0.0\nAvailable version: not-a-semver-tag\nRelease: https://example.invalid/releases/not-a-semver-tag\nUnable to compare versions automatically for release tag 'not-a-semver-tag'. Try `swarm update --version not-a-semver-tag` to target that release explicitly."
        );
    }

    #[test]
    fn derives_expected_asset_names() {
        assert_eq!(
            asset_name_for_target("x86_64-unknown-linux-gnu"),
            "swarm-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            asset_name_for_target("x86_64-pc-windows-msvc"),
            "swarm-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn rejects_unsupported_release_targets() {
        assert_eq!(
            supported_target_triple("macos", "aarch64").unwrap(),
            "aarch64-apple-darwin"
        );
        assert!(supported_target_triple("linux", "aarch64").is_err());
        assert!(supported_target_triple("windows", "aarch64").is_err());
    }

    #[test]
    fn rejects_archive_paths_that_escape_extract_dir() {
        assert!(validate_relative_archive_path(Path::new("swarm")).is_ok());
        assert!(validate_relative_archive_path(Path::new("nested/swarm")).is_ok());
        assert!(validate_relative_archive_path(Path::new("../swarm")).is_err());
        assert!(validate_relative_archive_path(Path::new("/tmp/swarm")).is_err());
    }

    #[test]
    fn rejects_unsupported_tar_entry_types() {
        // b'0' = regular file
        assert!(validate_tar_entry_type(tar::EntryType::new(b'0')).is_ok());
        // b'5' = directory
        assert!(validate_tar_entry_type(tar::EntryType::new(b'5')).is_ok());
        // b'2' = symlink
        assert!(validate_tar_entry_type(tar::EntryType::new(b'2')).is_err());
        // b'1' = hardlink
        assert!(validate_tar_entry_type(tar::EntryType::new(b'1')).is_err());
    }

    #[test]
    fn constructs_release_api_urls_with_encoded_tags() {
        let latest = release_api_url(None).unwrap();
        assert_eq!(
            latest.as_str(),
            "https://api.github.com/repos/JosunLP/AiOfficeSwarm/releases/latest"
        );

        let tagged = release_api_url(Some("release 2026/03/18")).unwrap();
        assert_eq!(
            tagged.as_str(),
            "https://api.github.com/repos/JosunLP/AiOfficeSwarm/releases/tags/release%202026%2F03%2F18"
        );
    }

    #[test]
    fn parses_checksum_entries() {
        assert_eq!(
            parse_checksum_line(
                "8f434346648f6b96df89dda901c5176b10a6d83961fca1c64c23d8bbf6f39767  swarm-x86_64-unknown-linux-gnu.tar.gz"
            ),
            Some((
                "8f434346648f6b96df89dda901c5176b10a6d83961fca1c64c23d8bbf6f39767",
                "swarm-x86_64-unknown-linux-gnu.tar.gz"
            ))
        );
        assert_eq!(
            parse_checksum_line(
                "8f434346648f6b96df89dda901c5176b10a6d83961fca1c64c23d8bbf6f39767 *swarm-x86_64-pc-windows-msvc.zip"
            ),
            Some((
                "8f434346648f6b96df89dda901c5176b10a6d83961fca1c64c23d8bbf6f39767",
                "swarm-x86_64-pc-windows-msvc.zip"
            ))
        );
        assert_eq!(parse_checksum_line("invalid"), None);
    }

    #[test]
    fn finds_matching_checksum_entry() {
        let checksums = "\
8f434346648f6b96df89dda901c5176b10a6d83961fca1c64c23d8bbf6f39767  swarm-x86_64-unknown-linux-gnu.tar.gz\n\
1E8BFEAA6F86DB89DDFCE3DC775100E91EB54A4D6FFB8F2A4832460D62D3901F  swarm-x86_64-pc-windows-msvc.zip\n";

        assert_eq!(
            checksum_for_asset(checksums, "swarm-x86_64-pc-windows-msvc.zip").unwrap(),
            "1e8bfeaa6f86db89ddfce3dc775100e91eb54a4d6ffb8f2a4832460d62d3901f"
        );
        assert!(checksum_for_asset(checksums, "missing.tar.gz").is_err());
    }
}
