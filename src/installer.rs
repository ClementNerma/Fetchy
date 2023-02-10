use std::{
    borrow::Cow,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use async_compression::tokio::write::{GzipDecoder, XzDecoder};
use async_zip::read::fs::ZipFileReader;
use colored::Colorize;
use dialoguer::Select;
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::{
    fs::{self, File},
    io,
};
use tokio_tar::Archive;

use crate::{
    app_data::{AppState, InstalledPackage, Repositories},
    fetcher::{fetch_package, fetch_package_asset_infos},
    find_matching_packages, info, progress_bar_tracker,
    repository::{ArchiveFormat, AssetFileType, FileFormat, Package},
    selector::find_installed_packages,
    success,
};

pub async fn install_package(
    pkg: &Package,
    dl_file_path: PathBuf,
    tmp_dir: TempDir,
    bin_dir: &Path,
    config_dir: &Path,
    repo_name: &str,
    version: String,
    on_message: &Box<dyn Fn(&str)>,
) -> Result<InstalledPackage> {
    let files_to_copy = match &pkg.download.file_format {
        FileFormat::Archive { format, files } => match format {
            ArchiveFormat::TarGz | ArchiveFormat::TarXz => {
                on_message("Extracting GZip archive...");

                let tar_file_path = tmp_dir.path().join("tarball.tmp");

                let mut tar_file = File::create(&tar_file_path)
                    .await
                    .context("Failed to create a temporary file for tarball extraction")?;

                let mut dl_file = File::open(&dl_file_path)
                    .await
                    .context("Failed to open downloaded file")?;

                if *format == ArchiveFormat::TarGz {
                    io::copy(&mut dl_file, &mut GzipDecoder::new(&mut tar_file))
                        .await
                        .context("Failed to extract GZip archive")?
                } else {
                    io::copy(&mut dl_file, &mut XzDecoder::new(&mut tar_file))
                        .await
                        .context("Failed to extract Xz archive")?
                };

                on_message("Analyzing tarball archive...");

                let tar_file = File::open(&tar_file_path)
                    .await
                    .context("Failed to open the tarball archive")?;

                let mut tarball = Archive::new(tar_file);

                let mut stream = tarball
                    .entries()
                    .context("Failed to list entries from tarball")?;

                let mut out = Vec::with_capacity(files.len());
                let mut treated = vec![None; files.len()];

                while let Some(entry) = stream.next().await {
                    let mut entry = entry.context("Failed to get entry from tarball archive")?;

                    let path = entry
                        .path()
                        .map(Cow::into_owned)
                        .context("Failed to get entry's path from tarball")?;

                    let Some(path_str) = path.to_str() else { continue };

                    for (i, file) in files.iter().enumerate() {
                        if !file.relative_path.regex.is_match(path_str) {
                            continue;
                        }

                        if let Some(prev) = &treated[i] {
                            bail!("Multiple entries matched the file regex ({}) in the tarball archive:\n* {}\n* {}",
                                file.relative_path.source,
                                prev,
                                path_str
                            );
                        }

                        let extraction_path = tmp_dir.path().join(format!("{i}.tmp"));

                        entry
                            .unpack(&extraction_path)
                            .await
                            .context("Failed to extract file from tarball archive")?;

                        out.push(ItemToCopy {
                            // original_path: Some(path_str.to_owned()),
                            extracted_path: extraction_path,
                            file_type: file.file_type.clone(),
                        });

                        treated[i] = Some(path_str.to_owned());
                    }
                }

                if let Some(pos) = treated.iter().position(Option::is_none) {
                    bail!(
                        "No entry matched the file regex ({}) in the tarball archive",
                        files[pos].relative_path.source
                    );
                }

                out
            }
            ArchiveFormat::Zip => {
                on_message("Analyzing ZIP archive...");

                let zip = ZipFileReader::new(&dl_file_path)
                    .await
                    .context("Failed to open ZIP archive")?;

                let entries = zip.file().entries();

                let mut out = Vec::with_capacity(files.len());

                for (i, file) in files.iter().enumerate() {
                    let results = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            file.relative_path.regex.is_match(entry.entry().filename())
                        })
                        .collect::<Vec<_>>();

                    if results.is_empty() {
                        bail!(
                            "No entry matched the file regex ({}) in the ZIP archive",
                            file.relative_path.source
                        );
                    } else if results.len() > 1 {
                        bail!(
                            "Multiple entries matched the file regex ({}) in the ZIP archive:\n{}",
                            file.relative_path.source,
                            results
                                .into_iter()
                                .map(|(_, entry)| format!("* {}", entry.entry().filename()))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    }

                    let mut reader = zip
                        .entry(results[0].0)
                        .await
                        .context("Failed to read entry from ZIP archive")?;

                    let extraction_path = tmp_dir.path().join(format!("{i}.tmp"));

                    let mut write = File::create(&extraction_path)
                        .await
                        .context("Failed to open writable file for extraction")?;

                    io::copy(&mut reader, &mut write)
                        .await
                        .context("Failed to extract file from ZIP to disk")?;

                    // TODO: CRC32 checking

                    out.push(ItemToCopy {
                        // original_path: Some(entry.filename().to_owned()),
                        extracted_path: extraction_path,
                        file_type: file.file_type.clone(),
                    });
                }

                out
            }
        },

        FileFormat::Binary {
            filename: out_filename,
        } => {
            vec![ItemToCopy {
                // original_path: filename.clone(),
                extracted_path: dl_file_path,
                file_type: AssetFileType::Binary {
                    copy_as: out_filename.clone(),
                },
            }]
        }
    };

    for file in &files_to_copy {
        on_message(&match &file.file_type {
            AssetFileType::Binary { copy_as } => format!("Copying binary: {copy_as}..."),
            AssetFileType::ConfigDir => format!("Copying configuration directory: {}...", pkg.name),
            AssetFileType::ConfigSubDir { copy_as } => {
                format!("Copying configuration sub-directory: {copy_as}...")
            }
        });

        let (out_path, is_dir) = match &file.file_type {
            AssetFileType::Binary { copy_as } => (bin_dir.join(copy_as), false),
            AssetFileType::ConfigDir => (config_dir.join(&pkg.name), true),
            AssetFileType::ConfigSubDir { copy_as } => {
                (config_dir.join(&pkg.name).join(copy_as), true)
            }
        };

        if !is_dir {
            fs::copy(&file.extracted_path, &out_path)
                .await
                .with_context(|| {
                    format!(
                        "Failed to copy file '{}' to the binaries directory",
                        out_path.file_name().unwrap().to_string_lossy()
                    )
                })?;
        } else {
            todo!()
        }

        // TODO: fix this as this doesn't work :(
        fs::set_permissions(&file.extracted_path, std::fs::Permissions::from_mode(0o744))
            .await
            .context("Failed to write file's new metadata (updated permissions)")?;
    }

    Ok(InstalledPackage {
        pkg_name: pkg.name.clone(),
        repo_name: repo_name.to_owned(),
        version,
        at: SystemTime::now(),
        binaries: files_to_copy
            .iter()
            .filter_map(|file| match &file.file_type {
                AssetFileType::Binary { copy_as } => Some(copy_as.clone()),
                AssetFileType::ConfigDir | AssetFileType::ConfigSubDir { copy_as: _ } => None,
            })
            .collect(),
    })
}

struct ItemToCopy {
    // original_path: Option<String>,
    extracted_path: PathBuf,
    file_type: AssetFileType,
}

pub struct InstallPackageOptions {
    pub confirm: bool,
    pub ignore_installed: bool,
    pub quiet: bool,
}

pub async fn install_packages(
    bin_dir: &Path,
    config_dir: &Path,
    app_state: &mut AppState,
    repositories: &Repositories,
    names: &[String],
    InstallPackageOptions {
        confirm,
        ignore_installed,
        quiet,
    }: InstallPackageOptions,
) -> Result<usize> {
    let to_install = names
        .iter()
        .filter(|name| {
            !ignore_installed || !app_state.installed.iter().any(|pkg| &&pkg.pkg_name == name)
        })
        .map(|name| {
            let candidates = find_matching_packages(repositories, name);

            if candidates.len() > 1 {
                bail!(
                    "Multiple candidates found for this package:\n{}",
                    candidates
                        .iter()
                        .map(|(repo, pkg)| format!(
                            "* {} (from repository {})",
                            pkg.name.bright_yellow(),
                            repo.content.name.bright_magenta()
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }

            candidates
                .into_iter()
                .next()
                .with_context(|| format!("Package {} was not found", name.bright_yellow()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if to_install.is_empty() {
        if !quiet {
            success!("Nothing to install!");
        }

        return Ok(0);
    }

    let total = to_install.len();
    let yellow_len = total.to_string().bright_yellow();

    if confirm {
        let prompt = format!(
            "Going to install {yellow_len} package(s):\n{}\n\nDo you want to continue?",
            to_install
                .iter()
                .map(|(_, pkg)| format!("* {}", pkg.name.bright_yellow()))
                .collect::<Vec<_>>()
                .join("\n")
        )
        .bright_blue();

        let choice = Select::new()
            .with_prompt(prompt.to_string())
            .items(&["Continue", "Abort"])
            .interact()?;

        if choice != 0 {
            bail!("Aborted by user.");
        }
    }

    for (i, (repo, pkg)) in to_install.into_iter().enumerate() {
        info!(
            "==> Installing package {} from repo {} ({} / {})...",
            pkg.name.bright_yellow(),
            repo.content.name.bright_magenta(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let asset_infos = fetch_package_asset_infos(pkg).await?;

        let installed = fetch_package(
            pkg,
            &repo.content.name,
            asset_infos,
            bin_dir,
            config_dir,
            &progress_bar_tracker(),
        )
        .await?;

        info!(
            "  |> Installed package version {} - deployed {} {}",
            installed.version.bright_yellow(),
            if installed.binaries.len() > 1 {
                "binaries"
            } else {
                "binary"
            },
            installed.binaries.join(", ").bright_green().underline()
        );

        let existing_index = app_state.installed.iter().position(|installed| {
            installed.repo_name == repo.content.name && installed.pkg_name == pkg.name
        });

        match existing_index {
            Some(index) => app_state.installed[index] = installed,
            None => app_state.installed.push(installed),
        }

        println!();
    }

    Ok(total)
}

pub async fn update_packages(
    app_state: &mut AppState,
    repositories: &Repositories,
    bin_dir: &Path,
    config_dir: &Path,
    names: &[String],
) -> Result<()> {
    let mut to_update = find_installed_packages(app_state, names)?;
    to_update.sort_by(|a, b| a.pkg_name.cmp(&b.pkg_name));

    let yellow_len = to_update.len().to_string().bright_yellow();

    for (i, installed) in to_update.into_iter().enumerate() {
        info!(
            "==> Updating package {} [from repo {}] ({} / {})...",
            installed.pkg_name.bright_yellow(),
            installed.repo_name.bright_magenta(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let repo = repositories
            .list
            .iter()
            .find(|repo| repo.content.name == installed.repo_name)
            .with_context(|| {
                format!(
                    "Package {} comes from unregistered repository {}, cannot update.",
                    installed.pkg_name, installed.repo_name
                )
            })?;

        let Some(pkg) = repo
            .content
            .packages
            .iter()
            .find(|candidate| candidate.name == installed.pkg_name) else {
                info!(
                    " |> Package {} is installed {}\n",
                    installed.pkg_name.bright_blue(),
                    "but does not seem to exist anymore in this repository".bright_yellow()
                );
                continue;
            };

        let asset_infos = fetch_package_asset_infos(pkg).await?;

        if asset_infos.version == installed.version {
            info!(
                " |> Package is already up-to-date (version {}), skipping.\n",
                installed.version.bright_yellow()
            );
            continue;
        }

        let prev_version = installed.version.clone();

        *installed = fetch_package(
            pkg,
            &repo.content.name,
            asset_infos,
            bin_dir,
            config_dir,
            &progress_bar_tracker(),
        )
        .await?;

        info!(
            " |> Updated package from version {} to {}.",
            prev_version.bright_yellow(),
            installed.version.bright_yellow(),
        );

        println!();
    }

    Ok(())
}
