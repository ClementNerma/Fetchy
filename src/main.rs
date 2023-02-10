#![forbid(unsafe_code)]
#![forbid(unused_must_use)]

use std::{fmt::Write, path::Path};

use anyhow::{bail, Context, Result};
use app_data::{AppState, Repositories, RepositorySource, SourcedRepository};
use clap::Parser;
use cmd::*;
use colored::Colorize;
use fetcher::{fetch_repository, FetchProgressTracking};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use installer::{update_packages, InstallPackageOptions};
use logging::PRINT_DEBUG_MESSAGES;
use repository::Package;
use tokio::fs;

use crate::installer::install_packages;

mod app_data;
mod cmd;
mod fetcher;
mod installer;
mod logging;
mod pattern;
mod repository;
mod selector;
mod sources;
mod utils;

#[tokio::main]
async fn main() {
    if let Err(err) = inner().await {
        error!("{}", err.chain().next().unwrap());

        for error in err.chain().skip(1) {
            error!("  {} {error}", "Caused by:".bright_yellow());
        }

        std::process::exit(1);
    }
}

async fn inner() -> Result<()> {
    let args = Cmd::parse();

    if args.verbose {
        PRINT_DEBUG_MESSAGES.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    let app_data_dir = dirs::data_local_dir()
        .context("Failed to get path to local data directory")?
        .join("fetchy");

    if !app_data_dir.exists() {
        fs::create_dir(&app_data_dir)
            .await
            .context("Failed to create the application's data directory")?;
    }

    let config_dir = dirs::config_dir().context("Failed to get path to config directory")?;

    let bin_dir = app_data_dir.join("bin");
    let state_file_path = app_data_dir.join("state.json");
    let repositories_file_path = app_data_dir.join("repositories.json");

    if !bin_dir.exists() {
        fs::create_dir(&bin_dir)
            .await
            .context("Failed to create the binaries directory")?;
    }

    let mut app_state = if state_file_path.exists() {
        let json = fs::read_to_string(&state_file_path)
            .await
            .context("Failed to read application's data file")?;

        serde_json::from_str::<AppState>(&json)
            .context("Failed to parse application's data file")?
    } else {
        AppState::default()
    };

    let mut repositories = if repositories_file_path.exists() {
        let json = fs::read_to_string(&repositories_file_path)
            .await
            .context("Failed to read the repositories file")?;

        serde_json::from_str::<Repositories>(&json)
            .context("Failed to parse the repositories file")?
    } else {
        Repositories::default()
    };

    match args.action {
        Action::Path(PathArgs {}) => {
            print!(
                "{}",
                bin_dir
                    .to_str()
                    .context("Path to the binaries directory contains invalid UTF-8 characters")?
            );
        }

        Action::AddRepo(AddRepoArgs { file, ignore }) => {
            let repo = fetch_repository(&RepositorySource::File(file.clone())).await?;

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

                save_repositories(&repositories_file_path, &repositories).await?;
            }
        }

        Action::ListRepos(ListReposArgs {}) => {
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

        Action::UpdateRepos(UpdateReposArgs {}) => {
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

                sourced.content = fetch_repository(&sourced.source).await?;
            }

            if !args.quiet {
                success!("Successfully updated all repositories!");
            }

            save_repositories(&repositories_file_path, &repositories).await?;
        }

        Action::Require(RequireArgs { names, confirm }) => {
            let count = install_packages(
                &bin_dir,
                &config_dir,
                &mut app_state,
                &repositories,
                &names,
                InstallPackageOptions {
                    confirm,
                    ignore_installed: true,
                    quiet: args.quiet,
                },
            )
            .await?;

            if count > 0 {
                save_app_state(&state_file_path, &app_state).await?;
            }
        }

        Action::CheckInstalled(CheckInstalledArgs { names }) => {
            let missing = names
                .iter()
                .filter(|name| {
                    !app_state
                        .installed
                        .iter()
                        .any(|installed| &&installed.pkg_name == name)
                })
                .collect::<Vec<_>>();

            if !missing.is_empty() {
                bail!(
                    "Detected the following missing packages:\n{}",
                    missing
                        .iter()
                        .map(|name| format!("* {}", name.bright_yellow()))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }

            if !args.quiet {
                success!("All provided packages are already installed!");
            }
        }

        Action::Install(InstallArgs { names }) => {
            if repositories.list.is_empty() {
                bail!("No repository found, please register one.");
            }

            let count = install_packages(
                &bin_dir,
                &config_dir,
                &mut app_state,
                &repositories,
                &names,
                InstallPackageOptions {
                    confirm: false,
                    ignore_installed: false,
                    quiet: args.quiet,
                },
            )
            .await?;

            if count > 0 {
                save_app_state(&state_file_path, &app_state).await?;
            }
        }

        Action::Update(UpdateArgs { names }) => {
            update_packages(&mut app_state, &repositories, &bin_dir, &config_dir, &names).await?;

            save_app_state(&state_file_path, &app_state).await?;
        }

        Action::Uninstall(UninstallArgs { name }) => {
            let index = app_state
                .installed
                .iter()
                .position(|package| package.pkg_name == name)
                .with_context(|| format!("Package '{name}' is not currently installed."))?;

            for file in &app_state.installed[index].binaries {
                fs::remove_file(bin_dir.join(file))
                    .await
                    .with_context(|| format!("Failed to remove binary '{file}'"))?;
            }

            app_state.installed.remove(index);

            save_app_state(&state_file_path, &app_state).await?;

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

fn progress_bar_tracker() -> FetchProgressTracking {
    let pb = Box::leak(Box::new(
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
        ));

    FetchProgressTracking {
        on_message: Box::new(|_| {}),
        on_dl_progress: Box::new(|a, b| {
            if let Some(b) = b {
                pb.set_length(b);
            }

            pb.set_position(a as u64);
        }),
        on_finish: Box::new(|| pb.finish()),
    }
}

async fn save_app_state(state_file_path: &Path, app_state: &AppState) -> Result<()> {
    debug!("Application's state changed, flushing to disk.");

    let app_data_str =
        serde_json::to_string(app_state).context("Failed to serialize application's data")?;

    fs::write(&state_file_path, &app_data_str)
        .await
        .context("Failed to write application's data to disk")
}

async fn save_repositories(
    repositories_file_path: &Path,
    repositories: &Repositories,
) -> Result<()> {
    debug!("Repositories changed, flushing to disk.");

    let repositories_str =
        serde_json::to_string(repositories).context("Failed to serialize the repositories")?;

    fs::write(&repositories_file_path, &repositories_str)
        .await
        .context("Failed to write the repositories to disk")
}
