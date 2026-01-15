// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Tool downloading and extraction functionality.

use super::types::{Platform, Tool, ToolchainError, ToolchainResult};
use super::FAILED_DOWNLOADS;
use console::style;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::sleep;

/// Maximum number of retry attempts for downloads
const MAX_RETRIES: u32 = 3;

/// Base delay in milliseconds for exponential backoff
const BASE_DELAY_MS: u64 = 500;

/// Download and extract a tool for the specified platform and version
pub async fn download_and_extract(
    tool: Tool,
    platform: Platform,
    version: &str,
    cache_dir: &Path,
) -> ToolchainResult<(String, PathBuf)> {
    // Create tool directory if it doesn't exist
    let tool_dir = cache_dir.join(tool.as_str());
    fs::create_dir_all(&tool_dir)?;

    // Resolve the actual version if "latest" is requested
    let actual_version = if version == "latest" {
        fetch_latest_version(tool).await?
    } else {
        version.to_string()
    };

    // Create version directory
    let version_dir = tool_dir.join(&actual_version);
    fs::create_dir_all(&version_dir)?;

    // Download with retries
    let (download_path, checksum) =
        download_with_retry(tool, platform, &actual_version, &version_dir).await?;

    // Extract the downloaded archive
    let executable_path =
        extract_archive(tool, platform, &download_path, &version_dir).await?;

    // Set executable permission on Unix platforms
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if executable_path.exists() {
            let mut perms = fs::metadata(&executable_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&executable_path, perms)?;
        }
    }

    // Write checksum file
    let checksum_file = version_dir.join(format!("{}.sha256", tool.as_str()));
    fs::write(checksum_file, checksum)?;

    // Update latest symlink
    let latest_link = tool_dir.join("latest");
    // Remove the symlink if it exists
    if latest_link.is_symlink() {
        let _ = fs::remove_file(&latest_link);
    }

    // Create proper symlinks with relative paths on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        if version == "latest" {
            let _ = symlink(&actual_version, &latest_link);
        }
    }

    // Create proper symlinks on Windows
    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_file;
        if version == "latest" {
            let _ = symlink_file(&actual_version, &latest_link);
        }
    }

    Ok((actual_version, executable_path))
}

/// Fetch the latest version of a tool
async fn fetch_latest_version(tool: Tool) -> ToolchainResult<String> {
    let client = Client::new();

    match tool {
        Tool::Sass => {
            // Fetch latest Dart Sass version from GitHub API
            let response = client
                .get("https://api.github.com/repos/sass/dart-sass/releases/latest")
                .header("User-Agent", "luat-cli")
                .send()
                .await;

            let resp = match response {
                Ok(resp) => {
                    if resp.status() == reqwest::StatusCode::FORBIDDEN {
                        // Rate limited, use fallback version
                        println!(
                            "{}",
                            style("GitHub API rate limit exceeded. Using fallback version.").yellow()
                        );
                        return Ok("1.89.2".to_string());
                    }

                    resp.json::<serde_json::Value>().await.map_err(|e| {
                        ToolchainError::ReleaseFetchFailed(format!(
                            "Failed to parse GitHub API response: {}",
                            e
                        ))
                    })?
                }
                Err(e) => {
                    return Err(ToolchainError::ReleaseFetchFailed(format!(
                        "Failed to fetch Sass version: {}",
                        e
                    )));
                }
            };

            if let Some(message) = resp["message"].as_str() {
                if message.contains("rate limit exceeded") {
                    println!(
                        "{}",
                        style("GitHub API rate limit exceeded. Using fallback version.").yellow()
                    );
                    return Ok("1.89.2".to_string());
                }

                return Err(ToolchainError::ReleaseFetchFailed(format!(
                    "GitHub API error: {}",
                    message
                )));
            }

            let version = resp["tag_name"].as_str().ok_or_else(|| {
                ToolchainError::ReleaseFetchFailed("Failed to parse sass release tag".to_string())
            })?;

            // Remove 'v' prefix if present
            let version = version.strip_prefix('v').unwrap_or(version);

            Ok(version.to_string())
        }

        Tool::Tailwind => {
            // Fetch latest Tailwind CSS version from GitHub API
            let resp = client
                .get("https://api.github.com/repos/tailwindlabs/tailwindcss/releases/latest")
                .header("User-Agent", "luat-cli")
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;

            let version = resp["tag_name"].as_str().ok_or_else(|| {
                ToolchainError::ReleaseFetchFailed("Failed to parse release tag".to_string())
            })?;

            // Remove 'v' prefix if present
            let version = version.strip_prefix('v').unwrap_or(version);

            Ok(version.to_string())
        }

        Tool::TypeScript => {
            // Fetch latest esbuild version from NPM registry
            let resp = client
                .get("https://registry.npmjs.org/esbuild")
                .header("User-Agent", "luat-cli")
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;

            let version = resp["dist-tags"]["latest"].as_str().ok_or_else(|| {
                ToolchainError::ReleaseFetchFailed(
                    "Failed to parse esbuild latest version from dist-tags".to_string(),
                )
            })?;

            Ok(version.to_string())
        }
    }
}

/// Download a tool with retry logic
async fn download_with_retry(
    tool: Tool,
    platform: Platform,
    version: &str,
    version_dir: &Path,
) -> ToolchainResult<(PathBuf, String)> {
    let client = Client::new();
    let mut retries = 0;

    loop {
        // Generate the download URL
        let url = match tool {
            Tool::Sass => {
                format!(
                    "https://github.com/sass/dart-sass/releases/download/{}/{}",
                    version,
                    platform.asset_name(tool, version)
                )
            }

            Tool::Tailwind => {
                let asset_name = platform.asset_name(tool, version);
                format!(
                    "https://github.com/tailwindlabs/tailwindcss/releases/download/v{}/{}",
                    version, asset_name
                )
            }

            Tool::TypeScript => {
                // Get the platform-specific package name
                let pkg = platform.asset_name(tool, version);

                // First get the platform-specific package metadata to find the tarball URL
                let pkg_info_url = format!("https://registry.npmjs.org/{}/{}", pkg, version);
                let pkg_info = client
                    .get(&pkg_info_url)
                    .header("User-Agent", "luat-cli")
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                // Extract the tarball URL directly from the package metadata
                pkg_info["dist"]["tarball"]
                    .as_str()
                    .ok_or_else(|| {
                        ToolchainError::DownloadFailed(format!(
                            "Failed to extract tarball URL from package metadata: {}",
                            pkg
                        ))
                    })?
                    .to_string()
            }
        };

        // Setup progress bar
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message(format!(
            "Downloading {} v{}...",
            style(tool.as_str()).cyan(),
            style(version).cyan()
        ));

        // Try to download the file
        let result = async {
            let resp = client
                .get(&url)
                .header("User-Agent", "luat-cli")
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                return Err(ToolchainError::DownloadFailed(format!(
                    "HTTP error: {} when downloading from URL: {}",
                    status, url
                )));
            }

            let content_length = resp.content_length().unwrap_or(0);
            pb.set_length(content_length);

            // Create a temporary file to download to
            let file_ext = match tool {
                Tool::Sass | Tool::TypeScript => ".tar.gz",
                Tool::Tailwind => "",
            };
            let download_path = version_dir.join(format!("{}{}", tool.as_str(), file_ext));
            let mut file = File::create(&download_path)?;

            // Setup hasher for checksum verification
            let mut hasher = Sha256::new();

            // Stream the response body
            let mut stream = resp.bytes_stream();
            let mut downloaded = 0;

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                downloaded += chunk.len() as u64;
                pb.set_position(downloaded);

                // Update hash
                hasher.update(&chunk);

                // Write to file
                file.write_all(&chunk)?;
            }

            pb.finish_with_message(format!(
                "Downloaded {} v{}",
                style(tool.as_str()).green(),
                style(version).green()
            ));

            // Generate checksum
            let hash = format!("{:x}", hasher.finalize());

            Ok::<_, ToolchainError>((download_path, hash))
        }
        .await;

        match result {
            Ok(res) => return Ok(res),
            Err(err) => {
                retries += 1;
                if retries >= MAX_RETRIES {
                    // Mark this download as failed so we don't try again
                    let mut failed = FAILED_DOWNLOADS.lock().unwrap();
                    failed.push(format!("{:?}-{}", tool, version));

                    return Err(err);
                }

                // Exponential backoff
                let delay = BASE_DELAY_MS * 2_u64.pow(retries - 1);
                pb.finish_with_message(format!(
                    "Download failed, retrying in {}ms ({}/{})",
                    style(delay).yellow(),
                    style(retries).yellow(),
                    style(MAX_RETRIES).yellow()
                ));

                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
}

/// Extract an archive file
async fn extract_archive(
    tool: Tool,
    platform: Platform,
    archive_path: &Path,
    extract_dir: &Path,
) -> ToolchainResult<PathBuf> {
    // For Tailwind, the downloaded file is the executable itself on all platforms
    if tool == Tool::Tailwind {
        return Ok(archive_path.to_path_buf());
    }

    // Extract the archive based on its extension
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message(format!("Extracting {}...", style(tool.as_str()).cyan()));
    pb.enable_steady_tick(Duration::from_millis(100));

    let result = if archive_path.to_string_lossy().ends_with(".tar.gz") {
        extract_tar_gz(archive_path, extract_dir)
    } else if archive_path.to_string_lossy().ends_with(".zip") {
        extract_zip(archive_path, extract_dir)
    } else {
        // For non-archive files, just return the path as-is
        return Ok(archive_path.to_path_buf());
    };

    match result {
        Ok(()) => {
            pb.finish_with_message(format!("Extracted {}", style(tool.as_str()).green()));

            // Determine the executable path
            let exec_path = platform.executable_path(tool);
            let executable_path = if exec_path.is_empty() {
                extract_dir.to_path_buf()
            } else {
                extract_dir.join(exec_path)
            };

            Ok(executable_path)
        }
        Err(err) => {
            pb.finish_with_message(format!(
                "Extraction failed: {}",
                style(err.to_string()).red()
            ));
            Err(err)
        }
    }
}

/// Extract a .tar.gz archive
fn extract_tar_gz(archive_path: &Path, extract_dir: &Path) -> ToolchainResult<()> {
    let file = File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(extract_dir)?;

    // Delete the archive file
    fs::remove_file(archive_path)?;

    Ok(())
}

/// Extract a .zip archive
fn extract_zip(archive_path: &Path, extract_dir: &Path) -> ToolchainResult<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => extract_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }

            let mut outfile = File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    // Delete the archive file
    fs::remove_file(archive_path)?;

    Ok(())
}
