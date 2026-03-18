//! `swarm update` sub-command.

use std::{
    cmp::Ordering,
    env::consts::{ARCH, OS},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context};
use clap::Args;
use semver::Version;
use serde::Deserialize;
use swarm_config::SwarmConfig;
use walkdir::WalkDir;

const REPO_OWNER: &str = "JosunLP";
const REPO_NAME: &str = "AiOfficeSwarm";
const BIN_NAME: &str = "swarm";

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
    let release = fetch_release(args.version.as_deref()).await?;
    let current_version = env!("CARGO_PKG_VERSION");
    let release_version = normalize_version(&release.tag_name);

    if args.version.is_none() {
        match compare_versions(&release.tag_name, current_version) {
            Some(Ordering::Equal) => {
                println!("swarm ist bereits aktuell (Version {}).", current_version);
                return Ok(());
            }
            Some(Ordering::Less) => {
                bail!(
                    "Die neueste veröffentlichte Version ({}) ist älter als die laufende Build-Version ({}).",
                    release_version,
                    current_version
                );
            }
            _ => {}
        }
    }

    if args.check {
        println!(
            "Aktuelle Version: {}\nVerfügbare Version: {}\nRelease: {}",
            current_version, release_version, release.html_url
        );
        if compare_versions(&release.tag_name, current_version) == Some(Ordering::Greater) {
            println!("Ein Update ist verfügbar.");
        } else {
            println!("Kein Update erforderlich.");
        }
        return Ok(());
    }

    let target = current_target_triple()?;
    let asset_name = asset_name_for_target(target);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| {
            format!(
                "Kein Release-Asset '{}' für das Ziel '{}' gefunden.",
                asset_name, target
            )
        })?;

    println!("Lade {} für {} herunter...", release.tag_name, target);

    let client = github_client()?;
    let temp_dir =
        tempfile::tempdir().context("Temporäres Verzeichnis konnte nicht erstellt werden")?;
    let archive_path = temp_dir.path().join(&asset.name);
    download_asset(&client, &asset.browser_download_url, &archive_path).await?;
    let binary_path = extract_binary(&archive_path, temp_dir.path())?;

    self_replace::self_replace(&binary_path).with_context(|| {
        format!(
            "Konnte '{}' nicht durch die neue Version ersetzen",
            BIN_NAME
        )
    })?;

    println!(
        "swarm wurde erfolgreich von {} auf {} aktualisiert.",
        current_version, release_version
    );
    println!("Release-Hinweise: {}", release.html_url);

    Ok(())
}

fn github_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("{BIN_NAME}/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("GitHub-HTTP-Client konnte nicht erstellt werden")
}

async fn fetch_release(version: Option<&str>) -> anyhow::Result<GitHubRelease> {
    let client = github_client()?;
    let url = match version {
        Some(version) => format!(
            "https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/tags/{}",
            normalize_tag(version)
        ),
        None => format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest"),
    };

    client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("GitHub Release-Metadaten konnten nicht geladen werden")?
        .error_for_status()
        .context("GitHub Release-Metadaten konnten nicht geladen werden")?
        .json::<GitHubRelease>()
        .await
        .context("GitHub Release-Antwort konnte nicht verarbeitet werden")
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
        .with_context(|| format!("Download von '{}' fehlgeschlagen", url))?
        .error_for_status()
        .with_context(|| format!("Download von '{}' fehlgeschlagen", url))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Antwort von '{}' konnte nicht gelesen werden", url))?;

    fs::write(destination, &bytes).with_context(|| {
        format!(
            "Archiv '{}' konnte nicht gespeichert werden",
            destination.display()
        )
    })
}

fn extract_binary(archive_path: &Path, temp_root: &Path) -> anyhow::Result<PathBuf> {
    let extract_dir = temp_root.join("extract");
    fs::create_dir_all(&extract_dir).with_context(|| {
        format!(
            "Extraktionsverzeichnis '{}' konnte nicht erstellt werden",
            extract_dir.display()
        )
    })?;

    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Archivname konnte nicht bestimmt werden"))?;

    if archive_name.ends_with(".zip") {
        extract_zip_archive(archive_path, &extract_dir)?;
    } else if archive_name.ends_with(".tar.gz") {
        extract_tar_gz_archive(archive_path, &extract_dir)?;
    } else {
        bail!("Nicht unterstütztes Archivformat: {}", archive_name);
    }

    let binary_name = executable_name();
    WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_type().is_file() && entry.file_name().to_string_lossy() == binary_name
        })
        .map(|entry| entry.into_path())
        .with_context(|| format!("Die Datei '{}' wurde im Archiv nicht gefunden", binary_name))
}

fn extract_tar_gz_archive(archive_path: &Path, extract_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path).with_context(|| {
        format!(
            "Archiv '{}' konnte nicht geöffnet werden",
            archive_path.display()
        )
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(extract_dir).with_context(|| {
        format!(
            "Archiv '{}' konnte nicht entpackt werden",
            archive_path.display()
        )
    })
}

fn extract_zip_archive(archive_path: &Path, extract_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path).with_context(|| {
        format!(
            "Archiv '{}' konnte nicht geöffnet werden",
            archive_path.display()
        )
    })?;
    let mut archive = zip::ZipArchive::new(file).with_context(|| {
        format!(
            "ZIP-Archiv '{}' konnte nicht gelesen werden",
            archive_path.display()
        )
    })?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("ZIP-Eintrag #{} konnte nicht gelesen werden", index))?;
        let Some(relative_path) = entry.enclosed_name().map(|path| path.to_owned()) else {
            continue;
        };

        let output_path = extract_dir.join(relative_path);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&output_path).with_context(|| {
                format!(
                    "Verzeichnis '{}' konnte nicht erstellt werden",
                    output_path.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Verzeichnis '{}' konnte nicht erstellt werden",
                    parent.display()
                )
            })?;
        }

        let mut output_file = File::create(&output_path).with_context(|| {
            format!(
                "Datei '{}' konnte nicht erstellt werden",
                output_path.display()
            )
        })?;
        io::copy(&mut entry, &mut output_file).with_context(|| {
            format!(
                "Datei '{}' konnte nicht geschrieben werden",
                output_path.display()
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = entry.unix_mode() {
                fs::set_permissions(&output_path, fs::Permissions::from_mode(mode)).with_context(
                    || {
                        format!(
                            "Berechtigungen für '{}' konnten nicht gesetzt werden",
                            output_path.display()
                        )
                    },
                )?;
            }
        }
    }

    Ok(())
}

fn current_target_triple() -> anyhow::Result<&'static str> {
    match (OS, ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => bail!(
            "Die Plattform '{}' mit Architektur '{}' wird für Self-Updates nicht unterstützt",
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
    version.trim().trim_start_matches('v')
}

fn normalize_tag(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        trimmed.to_owned()
    } else {
        format!("v{trimmed}")
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

#[cfg(test)]
mod tests {
    use super::{asset_name_for_target, compare_versions, normalize_tag, normalize_version};
    use std::cmp::Ordering;

    #[test]
    fn normalizes_versions_and_tags() {
        assert_eq!(normalize_version("v0.2.1"), "0.2.1");
        assert_eq!(normalize_version("0.2.1"), "0.2.1");
        assert_eq!(normalize_tag("0.2.1"), "v0.2.1");
        assert_eq!(normalize_tag("v0.2.1"), "v0.2.1");
    }

    #[test]
    fn compares_semver_values_with_or_without_prefix() {
        assert_eq!(compare_versions("v1.2.0", "1.1.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v1.2.0", "1.2.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.1.9", "v1.2.0"), Some(Ordering::Less));
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
}
