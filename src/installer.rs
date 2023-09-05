use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::Select;
use tempfile::TempDir;

use crate::{
    app_data::{AppState, InstalledPackage, Repositories},
    archives::extract_archive,
    debug, error, error_anyhow,
    fetcher::{fetch_package, fetch_package_asset_infos},
    find_matching_packages, info, progress_bar_tracker,
    repository::{AssetFileType, FileFormat, Package},
    save_app_state,
    selector::find_installed_packages,
    success,
    utils::{copy_dir, read_dir_tree},
};

pub struct InstallPackageOptions<'a, 'b, 'c, 'd> {
    pub pkg: &'a Package,
    pub dl_file_path: PathBuf,
    pub tmp_dir: TempDir,
    pub bin_dir: &'b Path,
    pub config_dir: &'c Path,
    pub repo_name: &'d str,
    pub version: String,
}

pub fn install_package(options: InstallPackageOptions<'_, '_, '_, '_>) -> Result<InstalledPackage> {
    let InstallPackageOptions {
        pkg,
        dl_file_path,
        tmp_dir,
        bin_dir,
        config_dir,
        repo_name,
        version,
    } = options;

    debug!("Installing package {repo_name}/{}", pkg.name);

    let items_to_copy = match &pkg.download.file_format {
        FileFormat::Binary { filename } => vec![ItemToCopy {
            extracted_path: dl_file_path,
            file_type: AssetFileType::Binary {
                copy_as: filename.clone(),
            },
        }],
        FileFormat::Archive { format, files } => {
            let extraction_path = tmp_dir.path().join("archive");

            fs::create_dir(&extraction_path).context("Failed to create a temporary directory")?;

            extract_archive(dl_file_path, format, extraction_path.clone())?;

            let mut out = Vec::with_capacity(files.len());
            let mut treated = vec![None; files.len()];

            let extracted =
                read_dir_tree(&extraction_path).context("Failed to list extracted items")?;

            let mut archive_files = vec![];

            for extracted_path in extracted {
                let path = extracted_path
                    .strip_prefix(&extraction_path)
                    .context("Failed to determine item path from extraction directory")?;

                if path.is_dir() {
                    continue;
                }

                archive_files.push(path.to_string_lossy().to_string());

                let Some(path_str) = path.to_str() else {
                    continue;
                };

                for (i, file) in files.iter().enumerate() {
                    if !file.relative_path.regex.is_match(path_str) {
                        continue;
                    }

                    if let Some(prev) = &treated[i] {
                        bail!("Found at least two entries matching the file regex ({}) in the archive:\n* {}\n* {}",
                            file.relative_path.source,
                            prev,
                            path_str
                        );
                    }

                    out.push(ItemToCopy {
                        // original_path: Some(path_str.to_owned()),
                        extracted_path: extracted_path.clone(),
                        file_type: file.file_type.clone(),
                    });

                    treated[i] = Some(path_str.to_owned());
                }
            }

            if let Some(pos) = treated.iter().position(Option::is_none) {
                bail!(
                    "No entry matched the file regex ({}) in the archive. Contained files are:\n{}",
                    files[pos].relative_path.source,
                    archive_files
                        .iter()
                        .map(|file| format!("* {file}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }

            out
        }
    };

    debug!("Copying {} items...", items_to_copy.len());

    for item in &items_to_copy {
        println!(
            "{}",
            match &item.file_type {
                AssetFileType::Binary { copy_as } => format!("Copying binary: {copy_as}..."),
                AssetFileType::ConfigDir =>
                    format!("Copying configuration directory: {}...", pkg.name),
                AssetFileType::ConfigSubDir { copy_as } => {
                    format!("Copying configuration sub-directory: {copy_as}...")
                }
            }
        );

        let (out_path, is_dir) = match &item.file_type {
            AssetFileType::Binary { copy_as } => (bin_dir.join(copy_as), false),
            AssetFileType::ConfigDir => (config_dir.join(&pkg.name), true),
            AssetFileType::ConfigSubDir { copy_as } => {
                (config_dir.join(&pkg.name).join(copy_as), true)
            }
        };

        if !is_dir {
            fs::copy(&item.extracted_path, &out_path).with_context(|| {
                format!(
                    "Failed to copy file '{}' to the binaries directory",
                    out_path.file_name().unwrap().to_string_lossy()
                )
            })?;

            #[cfg(target_family = "unix")]
            {
                use std::os::unix::fs::PermissionsExt;

                debug!("Setting file permissions...");

                fs::set_permissions(&out_path, std::fs::Permissions::from_mode(0o755))
                    .context("Failed to write file's new metadata (updated permissions)")?;
            }
        } else {
            // TODO: show progress bar
            copy_dir(&item.extracted_path, &out_path)?;
        }
    }

    Ok(InstalledPackage {
        pkg_name: pkg.name.clone(),
        repo_name: repo_name.to_owned(),
        version,
        at: SystemTime::now(),
        binaries: items_to_copy
            .iter()
            .filter_map(|file| match &file.file_type {
                AssetFileType::Binary { copy_as } => Some(copy_as.clone()),
                AssetFileType::ConfigDir | AssetFileType::ConfigSubDir { copy_as: _ } => None,
            })
            .collect(),
    })
}

pub struct InstallPackagesOptions<'a, 'b, 'c, 'd, 'e, 'f> {
    pub bin_dir: &'a Path,
    pub config_dir: &'b Path,
    pub app_state: &'c mut AppState,
    pub state_file_path: &'d Path,
    pub repositories: &'e Repositories,
    pub names: &'f [String],
    pub confirm: bool,
    pub ignore_installed: bool,
    pub quiet: bool,
}

pub fn install_packages(
    InstallPackagesOptions {
        bin_dir,
        config_dir,
        app_state,
        state_file_path,
        repositories,
        names,
        confirm,
        ignore_installed,
        quiet,
    }: InstallPackagesOptions<'_, '_, '_, '_, '_, '_>,
) -> Result<()> {
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

        return Ok(());
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

    let mut failed = 0;

    for (i, (repo, pkg)) in to_install.into_iter().enumerate() {
        info!(
            "==> Installing package {} from repo {} ({} / {})...",
            pkg.name.bright_yellow(),
            repo.content.name.bright_magenta(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let asset_infos = match fetch_package_asset_infos(pkg) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        let installed = match fetch_package(
            pkg,
            &repo.content.name,
            asset_infos,
            bin_dir,
            config_dir,
            progress_bar_tracker(),
        ) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

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

        save_app_state(state_file_path, app_state)?;
    }

    if failed > 0 {
        bail!("{} error(s) occurred.", failed.to_string().bright_yellow());
    }

    Ok(())
}

pub fn update_packages(
    app_state: &mut AppState,
    repositories: &Repositories,
    bin_dir: &Path,
    config_dir: &Path,
    names: &[String],
) -> Result<()> {
    let mut to_update = find_installed_packages(app_state, names)?;
    to_update.sort_by(|a, b| a.pkg_name.cmp(&b.pkg_name));

    let yellow_len = to_update.len().to_string().bright_yellow();

    let mut failed = 0;

    for (i, installed) in to_update.into_iter().enumerate() {
        info!(
            "==> Updating package {} [from repo {}] ({} / {})...",
            installed.pkg_name.bright_yellow(),
            installed.repo_name.bright_magenta(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let repo = match repositories
            .list
            .iter()
            .find(|repo| repo.content.name == installed.repo_name)
        {
            Some(data) => data,
            None => {
                failed += 1;
                error!(
                    "Package {} comes from unregistered repository {}, cannot update.",
                    installed.pkg_name, installed.repo_name
                );
                continue;
            }
        };

        let Some(pkg) = repo
            .content
            .packages
            .iter()
            .find(|candidate| candidate.name == installed.pkg_name)
        else {
            info!(
                " |> Package {} is installed {}\n",
                installed.pkg_name.bright_blue(),
                "but does not seem to exist anymore in this repository".bright_yellow()
            );
            continue;
        };

        let asset_infos = match fetch_package_asset_infos(pkg) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        if asset_infos.version == installed.version {
            info!(
                " |> Package is already up-to-date (version {}), skipping.\n",
                installed.version.bright_yellow()
            );
            continue;
        }

        let prev_version = installed.version.clone();

        *installed = match fetch_package(
            pkg,
            &repo.content.name,
            asset_infos,
            bin_dir,
            config_dir,
            progress_bar_tracker(),
        ) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        info!(
            " |> Updated package from version {} to {}.",
            prev_version.bright_yellow(),
            installed.version.bright_yellow(),
        );

        println!();
    }

    if failed > 0 {
        bail!("{} error(s) occurred.", failed.to_string().bright_yellow());
    }

    Ok(())
}

pub struct ItemToCopy {
    // original_path: Option<String>,
    extracted_path: PathBuf,
    file_type: AssetFileType,
}
