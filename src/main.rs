// TODO: try using ProgressIterator instead of doing manual pb.inc(1) in for loops

#![forbid(unsafe_code)]
#![forbid(unused_must_use)]
#![warn(unused_crate_dependencies)]
// TODO: remove nightly feature
#![feature(result_flattening)]

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    process::ExitCode,
};

use anyhow::{bail, Context, Result};
use clap::Parser as _;
use colored::Colorize;
use comfy_table::{presets, Attribute, Cell, Color, ContentArrangement, Table};
use log::{error, info, warn};
use rapidfuzz::distance::jaro_winkler::BatchComparator;
use tokio::fs;

// Bundling a vendored version of OpenSSL to avoid cross-platform compilation problems
// And avoid requiring OpenSSL on the client machine
use openssl_sys as _;

use self::{
    args::{Action, Args},
    db::{data::SourcedRepository, Db},
    fetch_repos::{fetch_repositories, fetch_repository, RepositoryLocation, RepositorySource},
    install::{display_pkg_phase, install_pkgs, InstalledPackagesHandling},
    logger::Logger,
    repos::ast::PackageManifest,
    resolver::{
        build_pkgs_reverse_deps_map, compute_no_longer_needed_deps, refresh_pkg,
        resolve_installed_pkgs, resolve_installed_pkgs_by_name, resolve_pkgs_by_name_with_deps,
    },
    utils::{confirm, join_iter},
};

mod args;
mod db;
mod fetch_repos;
mod install;
mod logger;
mod repos;
mod resolver;
mod sources;
mod utils;
mod validator;

#[tokio::main]
async fn main() -> ExitCode {
    let Args { action, verbosity } = Args::parse();

    // Set up the logger
    Logger::new(verbosity).init().unwrap();

    match inner(action).await {
        Ok(()) => ExitCode::SUCCESS,

        Err(err) => {
            error!("{err:?}");
            ExitCode::FAILURE
        }
    }
}

async fn inner(action: Action) -> Result<()> {
    let data_dir = dirs::state_dir()
        .context("Failed to get path to the user's app state directory")?
        .join("fetchy");

    let bin_dir = data_dir.join("bin");

    // Short-circuit before opening (and parsing) the database to make things quicker
    // This is especially important given that this action may be called on each user shell's startup
    if matches!(action, Action::BinPath) {
        println!("{}", bin_dir.display());
        return Ok(());
    }

    let mut db = Db::open_data_dir(data_dir, bin_dir).await?;

    let repos = db
        .repositories
        .iter()
        .map(|(name, repo)| (name.clone(), repo.content.clone()))
        .collect::<BTreeMap<_, _>>();

    match action {
        Action::Install {
            names,
            force,
            check_updates,
            discreet,
        } => {
            let pkgs = resolve_pkgs_by_name_with_deps(names.as_slice(), &repos)?;

            install_pkgs(
                pkgs,
                if force {
                    InstalledPackagesHandling::Reinstall
                } else if check_updates {
                    InstalledPackagesHandling::CheckUpdates
                } else {
                    InstalledPackagesHandling::Ignore
                },
                &mut db,
                discreet,
            )
            .await?;
        }

        Action::Update { names, force } => {
            let pkgs = if !names.is_empty() {
                resolve_installed_pkgs_by_name(&names, &db.installed, &repos)?
            } else {
                resolve_installed_pkgs(db.installed.values(), &repos)?
            };

            let pkgs = pkgs
                .into_iter()
                .map(|(resolved, _)| resolved)
                .map(refresh_pkg)
                .collect::<Result<Vec<_>, _>>()?;

            install_pkgs(
                pkgs,
                if force {
                    InstalledPackagesHandling::Reinstall
                } else {
                    InstalledPackagesHandling::Update
                },
                &mut db,
                false,
            )
            .await?;
        }

        Action::Uninstall { names, deps } => {
            let installed = resolve_installed_pkgs(db.installed.values(), &repos)?;

            let reverse_deps_map = build_pkgs_reverse_deps_map(
                installed.iter().map(|(resolved, _)| resolved.manifest),
            );

            let to_uninstall = resolve_installed_pkgs_by_name(&names, &db.installed, &repos)?;
            let to_uninstall_names = HashSet::from_iter(names.iter().map(String::as_str));

            for (resolved, _) in &to_uninstall {
                let Some(deps_of) = reverse_deps_map.get(resolved.manifest.name.as_str()) else {
                    continue;
                };

                let would_break = deps_of
                    .difference(&to_uninstall_names)
                    .collect::<BTreeSet<_>>();

                if !would_break.is_empty() {
                    bail!(
                        "Cannot remove package {} as it would break the following packages depending on it: {}",
                        resolved.manifest.name.bright_yellow(),
                        join_iter(would_break.iter().map(|name| name.bright_yellow()), " ")
                    );
                }
            }

            display_pkg_phase(
                "The following package(s) will be UNINSTALLED",
                to_uninstall.iter().map(|(p, _)| *p),
            );

            let no_longer_needed_deps =
                compute_no_longer_needed_deps(&installed, &to_uninstall_names, &reverse_deps_map);

            let to_uninstall = if !no_longer_needed_deps.is_empty() {
                display_pkg_phase(
                    if deps {
                        "The following unneeded dependencies will be uninstalled as well"
                    } else {
                        "The following dependencies will no longer be needed"
                    },
                    no_longer_needed_deps.iter().map(|(p, _)| *p),
                );

                if deps {
                    let mut to_uninstall = to_uninstall;
                    let mut no_longer_needed_deps = no_longer_needed_deps;

                    to_uninstall.append(&mut no_longer_needed_deps);
                    to_uninstall
                } else {
                    to_uninstall
                }
            } else {
                to_uninstall
            };

            warn!(
                "Do you want to want to uninstall {} package(s)?\n",
                to_uninstall.len().to_string().bright_red()
            );

            if !confirm().await? {
                return Ok(());
            }

            let bin_dir = db.bin_dir();

            let bin_paths = to_uninstall
                .into_iter()
                .flat_map(|(_, installed)| {
                    installed
                        .binaries
                        .iter()
                        .map(move |bin| (bin_dir.join(bin), bin, installed))
                })
                .collect::<Vec<_>>();

            if let Some((bin_path, bin_name, installed)) = bin_paths
                .iter()
                .find(|(bin_path, _, _)| !bin_path.is_file())
            {
                bail!(
                    "Binary {} from package {} is missing (at path: {})",
                    bin_name.bright_green(),
                    installed.manifest.name.bright_yellow(),
                    bin_path.to_string_lossy().bright_magenta()
                );
            }

            for (bin_path, bin_name, installed) in &bin_paths {
                fs::remove_file(&bin_path).await.with_context(|| {
                    format!(
                        "Faile dto remove binary {} from package {} is missing (at path: {})",
                        bin_name.bright_green(),
                        installed.manifest.name.bright_yellow(),
                        bin_path.to_string_lossy().bright_magenta()
                    )
                })?;
            }

            let to_uninstall = bin_paths
                .into_iter()
                .map(|(_, _, installed)| installed.manifest.name.clone())
                .collect::<Vec<_>>();

            db.update(|db| {
                for pkg_name in &to_uninstall {
                    assert!(db.installed.remove(pkg_name).is_some());
                }
            })
            .await?;

            info!(
                "Successfully removed {} packages!",
                to_uninstall.len().to_string().bright_yellow()
            );
        }

        Action::List {} => {
            let mut table = Table::new();

            table
                // Disable borders
                .load_preset(presets::NOTHING)
                // Enable dynamic sizing for columns
                .set_content_arrangement(ContentArrangement::Dynamic)
                // Add header
                .set_header(
                    ["Name", "Version", "Repository", "Binaries", "Install date"]
                        .into_iter()
                        .map(|header| {
                            Cell::new(header)
                                .add_attribute(Attribute::Bold)
                                .add_attribute(Attribute::Underlined)
                        }),
                );

            // TODO: add options to sort results
            let mut pkgs = db.installed.values().collect::<Vec<_>>();

            pkgs.sort_by(|a, b| {
                a.repo_name
                    .cmp(&b.repo_name)
                    .then_with(|| a.manifest.name.cmp(&b.manifest.name))
            });

            table.add_rows(pkgs.iter().map(|installed| {
                [
                    Cell::new(&installed.manifest.name).fg(Color::Yellow),
                    Cell::new(&installed.version).fg(Color::DarkCyan),
                    Cell::new(&installed.repo_name).fg(Color::Blue),
                    Cell::new(join_iter(installed.binaries.iter(), " ")).fg(Color::Green),
                    Cell::new(installed.at.strftime("%F %T")),
                ]
            }));

            println!("{table}");
        }

        Action::Repair { names } => {
            let installed = if !names.is_empty() {
                resolve_installed_pkgs_by_name(&names, &db.installed, &repos)?
            } else {
                resolve_installed_pkgs(db.installed.values(), &repos)?
            };

            let broken = installed
                .iter()
                .filter(|(_, installed)| {
                    installed
                        .binaries
                        .iter()
                        .any(|bin| !db.bin_dir().join(bin).is_file())
                })
                .collect::<Vec<_>>();

            if broken.is_empty() {
                info!("Found no broken package!");
                return Ok(());
            }

            display_pkg_phase(
                "Going to repair (and update) the following broken package(s)",
                broken.iter().map(|(resolved, _)| *resolved),
            );

            warn!("Do you want to continue?");

            if !confirm().await? {
                return Ok(());
            }

            let broken = broken
                .into_iter()
                .map(|(resolved, _)| refresh_pkg(*resolved))
                .collect::<Result<Vec<_>, _>>()?;

            install_pkgs(broken, InstalledPackagesHandling::Reinstall, &mut db, false).await?;
        }

        Action::Search {
            pattern,
            in_repos,
            show_installed,
        } => {
            if db.repositories.is_empty() {
                warn!("No registered repository");
                return Ok(());
            }

            let mut repos = repos;

            if !in_repos.is_empty() {
                let in_repos = HashSet::<_>::from_iter(in_repos.iter());
                repos.retain(|name, _| in_repos.contains(name));
            };

            let mut results = repos
                .values()
                .flat_map(|repo| {
                    repo.packages
                        .iter()
                        .filter(|(_, manifest)| pattern.is_match(&manifest.name))
                        .map(|(_, manifest)| (&repo.name, manifest))
                })
                .collect::<Vec<_>>();

            if !show_installed {
                let installed = db
                    .installed
                    .values()
                    .map(|installed| {
                        (
                            installed.repo_name.as_str(),
                            installed.manifest.name.as_str(),
                        )
                    })
                    .collect::<HashSet<_>>();

                results.retain(|(repo_name, manifest)| {
                    !installed.contains(&(repo_name.as_str(), manifest.name.as_str()))
                });
            }

            let comparator = BatchComparator::new(pattern.to_string().chars());

            let relevance = |manifest: &PackageManifest| {
                (comparator.distance(manifest.name.chars()) * 1_000_000_000.0) as u128
            };

            // Sort results by relevance, then by name
            results.sort_by(|(_, a), (_, b)| {
                relevance(a)
                    .cmp(&relevance(b))
                    .then_with(|| a.name.cmp(&b.name))
            });

            let mut table = Table::new();

            table
                // Disable borders
                .load_preset(presets::NOTHING)
                .set_header(["Package name", "Repository"].into_iter().map(|header| {
                    Cell::new(header)
                        .add_attribute(Attribute::Bold)
                        .add_attribute(Attribute::Underlined)
                }));

            table.add_rows(results.into_iter().map(|(repo_name, manifest)| {
                [
                    Cell::new(&manifest.name).fg(Color::Yellow),
                    Cell::new(repo_name).fg(Color::Blue),
                ]
            }));

            println!("{table}");
        }

        Action::AddRepo { path, json, ignore } => {
            let path = fs::canonicalize(&path)
                .await
                .context("Failed to canonicalize repository path")?;

            let location = RepositoryLocation::File(path);

            if let Some(repo) = db
                .repositories
                .values()
                .find(|repo| repo.source.location == location)
            {
                if !ignore {
                    warn!(
                        "Repository {} with the same provided location is already registered, skipping.",
                        repo.content.name.bright_blue()
                    );
                }

                return Ok(());
            }

            let source = RepositorySource { location, json };

            let repo = fetch_repository(&source).await?;

            if let Some(existing) = db.repositories.get(&repo.name) {
                bail!(
                    "A repository with the same name is already installed, source location: {}",
                    existing.source.location
                );
            }

            let pkgs_count = repo.packages.len();

            db.update(|db| {
                db.repositories.insert(
                    repo.name.clone(),
                    SourcedRepository {
                        content: repo,
                        source,
                    },
                );
            })
            .await?;

            info!(
                "Success! You now have {} additional packages to choose from!",
                pkgs_count.to_string().bright_yellow()
            );
        }

        Action::UpdateRepos {} => {
            if db.repositories.is_empty() {
                warn!("No registered repository");
                return Ok(());
            }

            let fetched =
                fetch_repositories(db.repositories.values().map(|repo| repo.source.clone()))
                    .await?;

            db.update(|db| {
                let mut fetched = fetched.into_iter();

                for (_, repo) in db.repositories.iter_mut() {
                    let fetched = fetched.next().unwrap();

                    // Just to be safe
                    assert_eq!(repo.content.name, fetched.name);

                    repo.content = fetched;
                }
            })
            .await?;

            info!(
                "Successfully updated {} repositories.",
                repos.len().to_string().bright_yellow()
            );
        }

        Action::RemoveRepos { names } => {
            let names = HashSet::<_>::from_iter(names.iter());
            let repos_names = HashSet::<_>::from_iter(repos.keys());

            if let Some(not_found) = names.difference(&repos_names).next() {
                bail!("Repository {} was not found", not_found.bright_blue());
            }

            db.update(|db| {
                for name in names {
                    assert!(db.repositories.remove(name).is_some());
                }
            })
            .await?;
        }

        Action::ListRepos {} => {
            if db.repositories.is_empty() {
                warn!("No registered repository");
                return Ok(());
            }

            let mut table = Table::new();

            table
                // Disable borders
                .load_preset(presets::NOTHING)
                // Add header
                .set_header(
                    ["Repository name", "Packages", "Source"]
                        .into_iter()
                        .map(|header| {
                            Cell::new(header)
                                .add_attribute(Attribute::Bold)
                                .add_attribute(Attribute::Underlined)
                        }),
                );

            table.add_rows(db.repositories.values().map(|repo| {
                [
                    Cell::new(&repo.content.name).fg(Color::Blue),
                    Cell::new(repo.content.packages.len().to_string()).fg(Color::Yellow),
                    Cell::new(&repo.source.location).fg(Color::Magenta),
                ]
            }));

            println!("{table}");
        }

        Action::BinPath => println!("{}", db.bin_dir().display()),
    }

    Ok(())
}
