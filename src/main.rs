#![forbid(unsafe_code)]
#![forbid(unused_must_use)]

use std::{fmt::Write, path::Path};

use anyhow::{bail, Context, Result};
use app_data::{AppState, Repositories, RepositorySource, SourcedRepository};
use clap::Parser;
use cmd::*;
use colored::Colorize;
use dialoguer::Select;
use fetcher::{fetch_repository, FetchProgressTracking};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use logging::PRINT_DEBUG_MESSAGES;
use repository::Package;
use tokio::fs;

use crate::fetcher::{fetch_package, fetch_package_asset_infos};

mod app_data;
mod cmd;
mod fetcher;
mod installer;
mod logging;
mod pattern;
mod repository;
mod sources;

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
            let to_update = app_state
                .installed
                .iter_mut()
                .filter(|package| {
                    if names.is_empty() {
                        true
                    } else {
                        names.contains(&package.pkg_name)
                    }
                })
                .collect::<Vec<_>>();

            for name in names {
                if !to_update.iter().any(|package| package.pkg_name == name) {
                    bail!("Package '{name}' was not found");
                }
            }

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

                let pkg = repo
                    .content
                    .packages
                    .iter()
                    .find(|candidate| candidate.name == installed.pkg_name)
                    .with_context(|| {
                        format!(
                            "Package {} was not found in repository {}",
                            installed.pkg_name.bright_yellow(),
                            installed.repo_name.bright_magenta()
                        )
                    })?;

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
                    &bin_dir,
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

            // success!("Successfully updated {yellow_len} package(s)!");

            save_app_state(&state_file_path, &app_state).await?;
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

struct InstallPackageOptions {
    confirm: bool,
    ignore_installed: bool,
    quiet: bool,
}

async fn install_packages(
    bin_dir: &Path,
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

    let yellow_len = to_install.len().to_string().bright_yellow();

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

    for (i, (repo, pkg)) in to_install.iter().enumerate() {
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

    Ok(to_install.len())
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
