#![forbid(unsafe_code)]
#![forbid(unused_must_use)]
#![warn(unused_crate_dependencies)]

use glob::Pattern;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use openssl_sys as _;

use std::{fmt::Write, fs, path::Path, sync::atomic::Ordering};

use anyhow::{bail, Context, Result};
use app_data::{AppState, InstalledPackage, Repositories, RepositorySource, SourcedRepository};
use clap::Parser;
use cmd::*;
use colored::Colorize;
use fetcher::fetch_repository;
use installer::{update_packages, InstallPackagesOptions};
use logging::PRINT_DEBUG_MESSAGES;
use repository::Package;

use crate::installer::install_packages;

mod app_data;
mod arch;
mod archives;
mod cmd;
mod fetcher;
mod installer;
mod logging;
mod pattern;
mod repository;
mod selector;
mod sources;
mod utils;

fn main() {
    if let Err(err) = inner() {
        error!("{}", err.chain().next().unwrap());

        for error in err.chain().skip(1) {
            error!("  {} {error}", "Caused by:".bright_yellow());
        }

        std::process::exit(1);
    }
}

fn inner() -> Result<()> {
    let args = Cmd::parse();

    if args.verbose {
        PRINT_DEBUG_MESSAGES.store(true, Ordering::SeqCst);
    }

    let app_data_dir = dirs::data_local_dir()
        .context("Failed to get path to local data directory")?
        .join("fetchy");

    if !app_data_dir.exists() {
        fs::create_dir_all(&app_data_dir)
            .context("Failed to create the application's data directory")?;
    }

    let config_dir = dirs::config_dir().context("Failed to get path to config directory")?;

    let bin_dir = app_data_dir.join("bin");
    let state_file_path = app_data_dir.join("state.json");
    let repositories_file_path = app_data_dir.join("repositories.json");

    if !bin_dir.exists() {
        fs::create_dir(&bin_dir).context("Failed to create the binaries directory")?;
    }

    let mut app_state = if state_file_path.exists() {
        let json = fs::read_to_string(&state_file_path)
            .context("Failed to read application's data file")?;

        serde_json::from_str::<AppState>(&json)
            .context("Failed to parse application's data file")?
    } else {
        AppState::default()
    };

    let mut repositories = if repositories_file_path.exists() {
        let json = fs::read_to_string(&repositories_file_path)
            .context("Failed to read the repositories file")?;

        serde_json::from_str::<Repositories>(&json)
            .context("Failed to parse the repositories file")?
    } else {
        Repositories::default()
    };

    match args.action {
        Action::Path => {
            print!(
                "{}",
                bin_dir
                    .to_str()
                    .context("Path to the binaries directory contains invalid UTF-8 characters")?
            );
        }

        Action::Repos(action) => match action {
            ReposAction::Add(AddRepoArgs { file, ignore }) => {
                let repo = fetch_repository(&RepositorySource::File(file.clone()))?;

                let already_exists = repositories
                    .list
                    .iter()
                    .any(|other| other.content.name == repo.name);

                if already_exists {
                    if !ignore {
                        bail!(
                            "Another repository is already registered with the name: {}",
                            repo.name.bright_magenta()
                        );
                    }
                } else {
                    repositories.list.push(SourcedRepository {
                        content: repo,
                        source: RepositorySource::File(file),
                    });

                    success!("Successfully added the repository!");

                    save_repositories(&repositories_file_path, &repositories)?;
                }
            }

            ReposAction::List => {
                info!(
                    "There are {} registered repositories:\n",
                    repositories.list.len()
                );

                for (i, sourced) in repositories.list.iter().enumerate() {
                    info!(
                        "* {:>2}. {}",
                        (i + 1).to_string().bright_yellow(),
                        sourced.content.name.bright_magenta()
                    );
                }
            }

            ReposAction::Update => {
                let yellow_len = repositories.list.len().to_string().bright_yellow();

                for (i, sourced) in repositories.list.iter_mut().enumerate() {
                    if !args.quiet {
                        info!(
                            "==> Updating repository {} ({} / {})...",
                            sourced.content.name.bright_magenta(),
                            (i + 1).to_string().bright_yellow(),
                            yellow_len
                        );
                    }

                    sourced.content = fetch_repository(&sourced.source)?;
                }

                if !args.quiet {
                    success!("Successfully updated all repositories!");
                }

                save_repositories(&repositories_file_path, &repositories)?;
            }
        },

        Action::Search(SearchArgs {
            filter,
            show_installed,
        }) => {
            let filter = filter
                .map(|filter| Pattern::new(&filter))
                .transpose()
                .context("Failed to parse provided glob pattern")?;

            let installable = repositories
                .list
                .iter()
                .flat_map(|repo| repo.content.packages.iter().map(move |pkg| (pkg, repo)))
                .filter(|(pkg, _)| match filter {
                    None => true,
                    Some(ref filter) => filter.matches(&pkg.name),
                })
                .filter(|(pkg, repo)| {
                    show_installed
                        || !app_state
                            .installed
                            .iter()
                            .any(|c| c.pkg_name == pkg.name && c.repo_name == repo.content.name)
                });

            for (pkg, _) in installable {
                println!("{}", pkg.name.bright_yellow());
            }
        }

        Action::Require(RequireArgs {
            names,
            no_install,
            confirm,
        }) => {
            let missing = names
                .iter()
                .filter(|name| {
                    !app_state
                        .installed
                        .iter()
                        .any(|installed| &&installed.pkg_name == name)
                })
                .collect::<Vec<_>>();

            if missing.is_empty() {
                return Ok(());
            }

            if no_install {
                bail!(
                    "Detected the following missing packages:\n{}",
                    missing
                        .iter()
                        .map(|name| format!("* {}", name.bright_yellow()))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }

            install_packages(InstallPackagesOptions {
                bin_dir: &bin_dir,
                config_dir: &config_dir,
                app_state: &mut app_state,
                state_file_path: &state_file_path,
                repositories: &repositories,
                names: &names,
                confirm,
                ignore_installed: true,
                quiet: args.quiet,
            })?;
        }

        Action::Install(InstallArgs { names }) => {
            if repositories.list.is_empty() {
                bail!("No repository found, please register one.");
            }

            install_packages(InstallPackagesOptions {
                bin_dir: &bin_dir,
                config_dir: &config_dir,
                app_state: &mut app_state,
                state_file_path: &state_file_path,
                repositories: &repositories,
                names: &names,
                confirm: false,
                ignore_installed: false,
                quiet: args.quiet,
            })?;
        }

        Action::Installed(InstalledArgs { sort_by, rev_sort }) => {
            let mut installed: Vec<_> = app_state.installed.iter().collect();

            if installed.is_empty() {
                warn!("No package installed.");
                return Ok(());
            }

            match sort_by {
                None | Some(PkgSortBy::Name) => installed.sort_by_key(|pkg| &pkg.pkg_name),
                Some(PkgSortBy::InstallDate) => installed.sort_by_key(|pkg| pkg.at),
            }

            if rev_sort {
                installed.reverse();
            }

            let largest_pkg_name = largest_key_width!(installed, pkg_name);
            let largest_pkg_version = largest_key_width!(installed, version);
            let largest_pkg_repo_name = largest_key_width!(installed, repo_name);

            for InstalledPackage {
                pkg_name,
                repo_name,
                version,
                at: _,
                binaries,
            } in installed
            {
                print!(
                    "{} {} {} {} ",
                    "*".bright_yellow(),
                    format!("{pkg_name:largest_pkg_name$}").bright_cyan(),
                    format!("{version:largest_pkg_version$}").bright_yellow(),
                    format!("[{repo_name:largest_pkg_repo_name$}]").bright_magenta(),
                );

                for bin in binaries {
                    print!("{}", bin.bright_green().underline());
                }

                println!();
            }
        }

        Action::Update(UpdateArgs { names }) => {
            let result =
                update_packages(&mut app_state, &repositories, &bin_dir, &config_dir, &names);

            save_app_state(&state_file_path, &app_state)?;

            result?;
        }

        Action::Uninstall(UninstallArgs { name }) => {
            let index = app_state
                .installed
                .iter()
                .position(|package| package.pkg_name == name)
                .with_context(|| format!("Package '{name}' is not currently installed."))?;

            for file in &app_state.installed[index].binaries {
                fs::remove_file(bin_dir.join(file))
                    .with_context(|| format!("Failed to remove binary '{file}'"))?;
            }

            app_state.installed.remove(index);

            save_app_state(&state_file_path, &app_state)?;

            success!(
                "Successfully uninstalled package '{}'",
                name.bright_yellow()
            );
        }
    }

    Ok(())
}

fn find_matching_packages<'a>(
    repos: &'a Repositories,
    name: &str,
) -> Vec<(&'a SourcedRepository, &'a Package)> {
    repos
        .list
        .iter()
        .flat_map(|repo| {
            repo.content
                .packages
                .iter()
                .filter(|package| package.name == name)
                .map(move |package| (repo, package))
        })
        .collect()
}

fn save_app_state(state_file_path: &Path, app_state: &AppState) -> Result<()> {
    debug!("Application's state changed, flushing to disk.");

    let app_data_str =
        serde_json::to_string(app_state).context("Failed to serialize application's data")?;

    fs::write(state_file_path, app_data_str).context("Failed to write application's data to disk")
}

fn save_repositories(repositories_file_path: &Path, repositories: &Repositories) -> Result<()> {
    debug!("Repositories changed, flushing to disk.");

    let repositories_str =
        serde_json::to_string(repositories).context("Failed to serialize the repositories")?;

    fs::write(repositories_file_path, repositories_str)
        .context("Failed to write the repositories to disk")
}

pub fn progress_bar_tracker() -> ProgressBar {
    ProgressBar::new(0)
    .with_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-")
    )
}
