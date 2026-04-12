use anyhow::{Context, Result};
use colored::Colorize;
use serde::Deserialize;

const RELEASES_URL: &str = "https://api.github.com/repos/yelsed/dots/releases/latest";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn run(check_only: bool) -> Result<()> {
    println!("{}", "dots update".bold());
    println!();

    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    println!("Current version: {}", current.to_string().cyan());

    print!("Checking for updates... ");
    let release: GithubRelease = ureq::get(RELEASES_URL)
        .header("User-Agent", &format!("dots/{}", env!("CARGO_PKG_VERSION")))
        .call()
        .context("Failed to fetch latest release")?
        .body_mut()
        .read_json()
        .context("Failed to parse release response")?;

    let tag = release.tag_name.trim_start_matches('v');
    let latest = semver::Version::parse(tag)
        .or_else(|_| semver::Version::parse(&format!("{}.0", tag)))
        .context("Cannot parse version from release tag")?;

    if latest <= current {
        println!("{}", "already up to date!".green());
        return Ok(());
    }

    println!("{}", format!("{} available", latest).green());

    if check_only {
        println!();
        println!(
            "Update available: {} -> {}",
            current.to_string().yellow(),
            latest.to_string().green()
        );
        println!("  {}", "run `dots update` to install".dimmed());
        return Ok(());
    }

    let expected_asset = asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == expected_asset)
        .ok_or_else(|| anyhow::anyhow!("Asset '{}' not found in release", expected_asset))?;

    println!("Downloading {}...", asset.name.dimmed());
    let archive_bytes = ureq::get(&asset.browser_download_url)
        .header("User-Agent", &format!("dots/{}", env!("CARGO_PKG_VERSION")))
        .call()
        .context("Failed to download release asset")?
        .body_mut()
        .read_to_vec()
        .context("Failed to read download")?;

    println!("Extracting...");
    let new_binary = extract_binary(&archive_bytes)?;

    println!("Replacing binary...");
    replace_binary(&new_binary)?;

    println!();
    println!(
        "{} {} -> {}",
        "Updated!".green().bold(),
        current.to_string().yellow(),
        latest.to_string().green()
    );

    Ok(())
}

fn asset_name() -> Result<&'static str> {
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Ok("dots-x86_64-unknown-linux-gnu.tar.gz")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Ok("dots-aarch64-apple-darwin.tar.gz")
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Ok("dots-x86_64-pc-windows-msvc.zip")
    } else {
        anyhow::bail!("No prebuilt binary available for this platform/architecture")
    }
}

#[cfg(not(target_os = "windows"))]
fn extract_binary(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::ffi::OsStr;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(gz);
    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.file_name() == Some(OsStr::new("dots")) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    anyhow::bail!("Binary 'dots' not found in archive")
}

#[cfg(target_os = "windows")]
fn extract_binary(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Cursor, Read};

    let cursor = Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to read zip archive")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name() == "dots.exe" {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    anyhow::bail!("Binary 'dots.exe' not found in archive")
}

fn replace_binary(new_binary: &[u8]) -> Result<()> {
    let current_exe = std::env::current_exe()?.canonicalize()?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine binary directory"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let temp_path = parent.join(".dots-update-tmp");
        std::fs::write(&temp_path, new_binary).context("Failed to write temporary binary")?;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&temp_path, &current_exe).context(format!(
            "Failed to replace binary at {}. You may need elevated permissions.",
            current_exe.display()
        ))?;
    }

    #[cfg(windows)]
    {
        let backup_path = parent.join("dots.exe.old");
        let _ = std::fs::remove_file(&backup_path);
        std::fs::rename(&current_exe, &backup_path)
            .context("Failed to move current binary aside")?;
        std::fs::write(&current_exe, new_binary).context(format!(
            "Failed to write new binary at {}. You may need elevated permissions.",
            current_exe.display()
        ))?;
        let _ = std::fs::remove_file(&backup_path);
    }

    Ok(())
}
