use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use colored::Colorize;
use jiff::Zoned;
use log::info;
use tokio::sync::RwLock;

use crate::{
    db::{Db, data::InstalledPackage},
    install::{
        display::display_install_phases,
        downloader::download_pkgs_and,
        phases::{InstallPhases, PackagesToInstall},
    },
    repos::ast::PackageManifest,
    resolver::ResolvedPkg,
    sources::{AssetInfos, AssetType},
    utils::confirm,
};

use super::phases::{InstalledPackagesHandling, compute_install_phases};

/// Configure package installation process
pub struct InstallOpts {
    /// Show less informations
    pub discreet: bool,

    /// Perform installation without asking for user confirmation
    pub no_confirm: bool,
}

pub async fn install_pkgs(
    pkgs: Vec<ResolvedPkg<'_, '_>>,
    installed_pkgs_handling: InstalledPackagesHandling,
    db: Db,
    InstallOpts {
        discreet,
        no_confirm,
    }: InstallOpts,
) -> Result<()> {
    let start = Instant::now();

    let phases = compute_install_phases(pkgs, installed_pkgs_handling, &db).await?;

    let InstallPhases {
        untouched: _,
        to_install:
            PackagesToInstall {
                missing_pkgs,
                missing_deps,
                needs_updating,
                reinstall,
            },
    } = &phases;

    let to_install = missing_pkgs
        .iter()
        .chain(missing_deps)
        .map(|(resolved, asset_infos)| (*resolved, asset_infos))
        .chain(
            needs_updating
                .iter()
                .chain(reinstall)
                .map(|(resolved, asset_infos, _)| (*resolved, asset_infos)),
        )
        .collect::<Vec<_>>();

    if to_install.is_empty() && discreet {
        return Ok(());
    }

    display_install_phases(&phases, installed_pkgs_handling, discreet);

    if to_install.is_empty() {
        info!("Nothing to do!");
        return Ok(());
    }

    if (to_install.iter().any(|(pkg, _)| pkg.is_dep)
        || matches!(
            installed_pkgs_handling,
            InstalledPackagesHandling::Update | InstalledPackagesHandling::Reinstall
        ))
        && !no_confirm
    {
        info!(
            "{}",
            format!(
                "Do you want to install these {} package(s)?",
                to_install.len().to_string().bright_yellow()
            )
            .bright_green()
        );

        if !confirm().await? {
            bail!("Aborted by user");
        }
    }

    let mut seen_bins = db
        .installed
        .values()
        .flat_map(|installed| {
            installed
                .binaries
                .iter()
                .map(|bin| (bin.as_str(), &installed.manifest))
        })
        .collect::<HashMap<_, _>>();

    for (pkg, asset_infos) in &to_install {
        let binaries = match &asset_infos.typ {
            AssetType::Binary { copy_as } => vec![copy_as.as_str()],
            AssetType::Archive { format: _, files } => {
                files.iter().map(|bin| bin.copy_as.as_str()).collect()
            }
        };

        for binary in binaries {
            match seen_bins.entry(binary) {
                Entry::Occupied(clashing_pkg) => {
                    if pkg.manifest.name != clashing_pkg.get().name {
                        bail!(
                            "Can't install package {} as it exposes the same binary {} than package {}",
                            pkg.manifest.name.bright_yellow(),
                            binary.bright_green(),
                            clashing_pkg.get().name.bright_yellow()
                        )
                    }
                }

                Entry::Vacant(vacant) => {
                    vacant.insert(pkg.manifest);
                }
            }
        }
    }

    let pkg_infos = to_install
        .iter()
        .map(|(pkg, asset_infos)| {
            (
                pkg.manifest.name.clone(),
                ExtractionPkgInfo {
                    repo_name: pkg.repository.name.clone(),
                    is_dep: pkg.is_dep,
                    binaries: match &asset_infos.typ {
                        AssetType::Binary { copy_as } => vec![copy_as.clone()],
                        AssetType::Archive { format: _, files } => {
                            files.iter().map(|bin| bin.copy_as.clone()).collect()
                        }
                    },
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let to_install_count = to_install.len();

    let to_install = to_install
        .iter()
        .map(|(pkg, asset_infos)| (pkg.manifest.clone(), (*asset_infos).clone()))
        .collect();

    let bins_dir = db.bin_dir().to_owned();
    let db = Arc::new(RwLock::new(db));
    let pkg_infos = Arc::new(pkg_infos);

    let tmp_dir = download_pkgs_and(to_install, &bins_dir, move |manifest, asset_infos| {
        update_db(
            pkg_infos.get(&manifest.name).unwrap().clone(),
            manifest,
            asset_infos,
            Arc::clone(&db),
        )
    })
    .await?;

    info!(
        "Successfully installed {} package(s) in {} second(s)!",
        to_install_count.to_string().bright_yellow(),
        start.elapsed().as_secs().to_string().bright_magenta()
    );

    let tmp_dir_path = tmp_dir.path().to_owned();

    tokio::task::spawn_blocking(move || {
        tmp_dir.close().with_context(|| {
            format!(
                "Failed to remove temporary downloads directory at path: {}",
                tmp_dir_path.display()
            )
        })
    })
    .await
    .context("Failed to wait on Tokio task")??;

    Ok(())
}

#[derive(Clone)]
struct ExtractionPkgInfo {
    repo_name: String,
    is_dep: bool,
    binaries: Vec<String>,
}

async fn update_db(
    pkg_infos: ExtractionPkgInfo,
    manifest: PackageManifest,
    asset_infos: AssetInfos,
    db: Arc<RwLock<Db>>,
) -> Result<()> {
    let ExtractionPkgInfo {
        repo_name,
        is_dep,
        binaries,
    } = pkg_infos;

    db.write()
        .await
        .update(|db| {
            let installed_as_dep = db
                .installed
                .get(&manifest.name)
                .map(|installed| installed.installed_as_dep)
                .unwrap_or(is_dep);

            db.installed.insert(
                manifest.name.clone(),
                InstalledPackage {
                    manifest: manifest.clone(),
                    repo_name: repo_name.clone(),
                    version: asset_infos.version,
                    installed_as_dep,
                    binaries: binaries.clone(),
                    at: Zoned::now(),
                },
            );
        })
        .await
        .context("Failed to update database")?;

    Ok(())
}
