use std::{
    fs,
    path::{Path, PathBuf, MAIN_SEPARATOR_STR},
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::Select;
use tempfile::TempDir;

use crate::{
    app_data::{AppState, InstalledPackage, Repositories, SourcedRepository},
    archives::extract_archive,
    debug, error, error_anyhow,
    fetcher::{fetch_package, fetch_package_asset_infos},
    find_matching_packages, info,
    repository::{FileExtraction, Package},
    save_app_state,
    selector::find_installed_packages,
    success,
    utils::{progress_bar_tracker, read_dir_tree},
};

pub struct InstallPackageOptions<'a, 'b, 'c> {
    pub pkg: &'a Package,
    pub dl_file_path: PathBuf,
    pub tmp_dir: TempDir,
    pub bin_dir: &'b Path,
    pub repo_name: &'c str,
    pub version: String,
    pub extraction: FileExtraction,
}

pub fn install_package(options: InstallPackageOptions) -> Result<InstalledPackage> {
    let InstallPackageOptions {
        pkg,
        dl_file_path,
        tmp_dir,
        bin_dir,
        repo_name,
        version,
        extraction,
    } = options;

    debug!("Installing package {repo_name}/{}", pkg.name);

    let items_to_copy = match &extraction {
        FileExtraction::Binary { copy_as } => vec![ItemToCopy {
            extracted_path: dl_file_path,
            bin_name: copy_as.to_owned(),
        }],

        FileExtraction::Archive { format, files } => {
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

                let path_str = path_str.replace('\\', "/");

                for (i, file) in files.iter().enumerate() {
                    if !file.relative_path.regex.is_match(&path_str) {
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
                        extracted_path: extracted_path.clone(),
                        bin_name: file.rename.clone().unwrap_or_else(|| {
                            Path::new(&path_str)
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned()
                        }),
                    });

                    treated[i] = Some(path_str.replace('/', MAIN_SEPARATOR_STR));
                }
            }

            if let Some(pos) = treated.iter().position(Option::is_none) {
                if archive_files.is_empty() {
                    bail!("Archive is empty!");
                }

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
        debug!(
            "Copy binary '{}' from {}...",
            item.bin_name,
            item.extracted_path.display()
        );

        let out_path = bin_dir.join(&item.bin_name);

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
    }

    Ok(InstalledPackage {
        pkg_name: pkg.name.clone(),
        repo_name: repo_name.to_owned(),
        version,
        at: SystemTime::now(),
        binaries: items_to_copy
            .iter()
            .map(|item| item.bin_name.clone())
            .collect(),
    })
}

pub struct InstallPackagesOptions<'a, 'b, 'c, 'd, 'e> {
    pub bin_dir: &'a Path,
    pub app_state: &'b mut AppState,
    pub state_file_path: &'c Path,
    pub repositories: &'d Repositories,
    pub names: &'e [String],
    pub confirm: bool,
    pub ignore_installed: bool,
    pub quiet: bool,
}

pub fn install_packages(
    InstallPackagesOptions {
        bin_dir,
        app_state,
        state_file_path,
        repositories,
        names,
        confirm,
        ignore_installed,
        quiet,
    }: InstallPackagesOptions,
) -> Result<()> {
    let mut to_install: Vec<ResolvedPkg> = vec![];

    for name in names {
        for resolved in find_package_with_dependencies(name, repositories)? {
            let is_dup = to_install
                .iter()
                .any(|c| c.package.name == resolved.package.name);

            if is_dup {
                continue;
            }

            if ignore_installed || resolved.dependency_of.is_some() {
                let already_installed = app_state
                    .installed
                    .iter()
                    .any(|pkg| pkg.pkg_name == resolved.package.name);

                if already_installed {
                    continue;
                }
            }

            to_install.push(resolved);
        }
    }

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
                .map(|resolved| format!("* {}", resolved.package.name.bright_yellow()))
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

    for (i, resolved) in to_install.into_iter().enumerate() {
        let ResolvedPkg {
            from_repo,
            package,
            dependency_of,
        } = resolved;

        info!(
            "==> Installing package {} from repo {}{} ({} / {})...",
            package.name.bright_yellow(),
            from_repo.content.name.bright_magenta(),
            dependency_of
                .map(|dependency_of| format!(
                    " (as a dependency of {})",
                    dependency_of.bright_yellow()
                ))
                .unwrap_or_default(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let asset_infos = match fetch_package_asset_infos(package) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        let fetched = match fetch_package(
            package,
            &from_repo.content.name,
            asset_infos,
            bin_dir,
            progress_bar_tracker(),
        ) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        let installed = match install_package(fetched) {
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
            installed.repo_name == from_repo.content.name && installed.pkg_name == package.name
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
    names: &[String],
    force: bool,
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
            if !force {
                info!(
                    " |> Package is already up-to-date (version {}), skipping.\n",
                    installed.version.bright_yellow()
                );

                continue;
            }

            info!(
                "|> Package is already up-to-date ({}), reinstalling anyway as requested...",
                installed.version.bright_yellow()
            );
        } else {
            info!(
                "|> Updating from version {} to version {}...",
                installed.version.bright_yellow(),
                asset_infos.version.bright_yellow(),
            );
        }

        let fetched = match fetch_package(
            pkg,
            &repo.content.name,
            asset_infos,
            bin_dir,
            progress_bar_tracker(),
        ) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        *installed = match install_package(fetched) {
            Ok(data) => data,
            Err(err) => {
                error_anyhow!(err);
                failed += 1;
                continue;
            }
        };

        info!(" |> Success.",);

        println!();
    }

    if failed > 0 {
        bail!("{} error(s) occurred.", failed.to_string().bright_yellow());
    }

    Ok(())
}

fn find_package<'a>(
    name: &str,
    repositories: &'a Repositories,
) -> Result<(&'a SourcedRepository, &'a Package)> {
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
}

fn find_package_with_dependencies<'a>(
    name: &str,
    repositories: &'a Repositories,
) -> Result<Vec<ResolvedPkg<'a>>> {
    let (from_repo, package) = find_package(name, repositories)?;

    let mut out = vec![ResolvedPkg {
        from_repo,
        package,
        dependency_of: None,
    }];

    out.append(&mut resolve_package_dependencies(package, repositories)?);

    Ok(out)
}

fn resolve_package_dependencies<'a>(
    package: &'a Package,
    repositories: &'a Repositories,
) -> Result<Vec<ResolvedPkg<'a>>> {
    let mut out = vec![];

    for dep in package.depends_on.as_ref().unwrap_or(&vec![]) {
        let (from_repo, dep_pkg) = find_package(dep, repositories).with_context(|| {
            format!(
                "Failed to find package '{dep}' which is a dependency of '{}'",
                package.name
            )
        })?;

        let mut resolved = resolve_package_dependencies(dep_pkg, repositories)?;

        out.push(ResolvedPkg {
            from_repo,
            package: dep_pkg,
            dependency_of: Some(&package.name),
        });

        out.append(&mut resolved);
    }

    Ok(out)
}

#[derive(Clone, Copy)]
struct ResolvedPkg<'a> {
    from_repo: &'a SourcedRepository,
    package: &'a Package,
    dependency_of: Option<&'a str>,
}

pub struct ItemToCopy {
    // original_path: Option<String>,
    extracted_path: PathBuf,
    bin_name: String,
}
