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
    app_data::{AppData, InstalledPackage, Repositories},
    archives::extract_archive,
    debug, error_anyhow,
    fetcher::{fetch_package, AssetInfos},
    info,
    repository::{FileExtraction, Package},
    resolver::{build_install_phases, InstallPhases, ResolvedPkg},
    save_app_state, success,
    utils::{progress_bar, read_dir_tree},
    AppState,
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

#[derive(Clone, Copy)]
pub enum InstalledPackagesAction {
    Ignore,
    Update,
    Reinstall,
}

pub fn install_packages(
    repositories: &Repositories,
    names: &[String],
    for_already_installed: InstalledPackagesAction,
    app_data: &mut AppData,
    state: &AppState,
) -> Result<()> {
    let InstallPhases {
        already_installed,
        no_update_needed,
        update_available,
        update,
        install_new,
        install_deps,
        reinstall,
    } = build_install_phases(names, repositories, for_already_installed, app_data)?;

    let install_count = update.len() + install_new.len() + install_deps.len() + reinstall.len();

    if install_count == 0 && state.quiet {
        return Ok(());
    }

    print_category(
        "The following NEW package(s) will be installed",
        install_new.iter().map(|(p, _)| *p),
    );

    print_category(
        "The following dependency package(s) will be installed",
        install_deps.iter().map(|(p, _)| *p),
    );

    print_category(
        "The following package(s) will be updated",
        update.iter().map(|(p, _)| *p),
    );

    print_category(
        "The following existing package(s) will be *re*installed",
        reinstall.iter().map(|(p, _)| *p),
    );

    print_category(
        "The following package(s) have an available update",
        update_available.iter().copied(),
    );

    if !state.quiet {
        print_category(
            "The following package(s) are already on their latest version",
            no_update_needed.iter().copied(),
        );

        print_category(
            "The following package(s) are already installed and require no action",
            already_installed.iter().copied(),
        );
    }

    if install_count == 0 {
        success!("Nothing to install!");
        return Ok(());
    }

    let yellow_len = install_count.to_string().bright_yellow();

    if install_count > names.len() {
        let prompt = format!(
            "|> Going to install a total {yellow_len} package(s)\n\nDo you want to continue?"
        )
        .bright_blue();

        let choice = Select::new()
            .with_prompt(prompt.to_string())
            .items(&["Continue", "Abort"])
            .interact()?;

        if choice != 0 {
            bail!("Aborted by user.");
        }

        println!();
    }

    let to_install = install_new
        .into_iter()
        .chain(install_deps)
        .chain(update)
        .chain(reinstall)
        .collect::<Vec<_>>();

    perform_install(to_install, app_data, state)
}

fn print_category<'a>(name: &str, pkgs: impl ExactSizeIterator<Item = ResolvedPkg<'a>>) {
    // Don't display categories with no package
    if pkgs.len() == 0 {
        return;
    }

    let pkgs_table = pkgs
        .enumerate()
        .fold(String::new(), |mut acc, (i, resolved)| {
            if i > 0 {
                acc.push(if i % 10 == 0 { '\n' } else { ' ' });
            }

            acc.push_str(&resolved.package.name);
            acc
        })
        .bright_yellow();

    println!("{}\n\n{pkgs_table}\n", format!("{name}:").bright_blue(),);
}

fn perform_install(
    to_install: Vec<(ResolvedPkg, AssetInfos)>,
    app_data: &mut AppData,
    state: &AppState,
) -> Result<()> {
    let yellow_len = to_install.len().to_string().bright_yellow();

    let mut failed = 0;

    for (i, (resolved, asset_infos)) in to_install.into_iter().enumerate() {
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
                .map(|dep_of| format!(" (as a dependency of {})", dep_of.bright_yellow()))
                .unwrap_or_default(),
            (i + 1).to_string().bright_yellow(),
            yellow_len,
        );

        let fetched_result = fetch_package(
            package,
            &from_repo.content.name,
            asset_infos,
            state,
            progress_bar(0, "{bytes}/{total_bytes}"),
        );

        let installed = match fetched_result.and_then(install_package) {
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

        let existing_index = app_data.installed.iter().position(|installed| {
            installed.repo_name == from_repo.content.name && installed.pkg_name == package.name
        });

        match existing_index {
            Some(index) => app_data.installed[index] = installed,
            None => app_data.installed.push(installed),
        }

        println!();

        save_app_state(&state.state_file_path, app_data)?;
    }

    if failed > 0 {
        bail!("{} error(s) occurred.", failed.to_string().bright_yellow());
    }

    Ok(())
}

pub struct ItemToCopy {
    // original_path: Option<String>,
    extracted_path: PathBuf,
    bin_name: String,
}
