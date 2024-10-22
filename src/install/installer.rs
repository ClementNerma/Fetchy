use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::ProgressBar;
use jiff::Zoned;
use log::info;
use tokio::fs;

use crate::{
    db::{data::InstalledPackage, Db},
    install::{
        display::display_install_phases,
        downloader::download_assets_and,
        extract::ExtractedBinary,
        phases::{InstallPhases, PackagesToInstall},
    },
    repos::ast::PackageManifest,
    resolver::ResolvedPkg,
    sources::AssetInfos,
    utils::{confirm, progress_bar, ITEMS_PROGRESS_BAR_STYLE},
};

use super::{
    extract::{extract_asset, ExtractionResult},
    phases::{compute_install_phases, InstalledPackagesHandling},
};

pub async fn install_pkgs(
    pkgs: Vec<ResolvedPkg<'_, '_>>,
    installed_pkgs_handling: InstalledPackagesHandling,
    db: &mut Db,
    discreet: bool,
) -> Result<()> {
    let start = Instant::now();

    let phases = compute_install_phases(pkgs, installed_pkgs_handling, db).await?;

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

    if to_install.iter().any(|(pkg, _)| pkg.is_dep) {
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

    let state = ExtractionState {
        pkg_infos: Arc::new(
            to_install
                .iter()
                .map(|(pkg, _)| {
                    (
                        pkg.manifest.name.clone(),
                        ExtractionPkgInfo {
                            repo_name: pkg.repository.name.clone(),
                            is_dep: pkg.is_dep,
                        },
                    )
                })
                .collect(),
        ),
    };

    let (tmp_dir, extracted) = download_assets_and(
        to_install
            .iter()
            .map(|(pkg, asset_infos)| (pkg.manifest.clone(), (*asset_infos).clone()))
            .collect(),
        state,
        extract_downloaded_asset,
    )
    .await?;

    let mut seen_bins = db
        .installed
        .values()
        .flat_map(|installed| {
            installed
                .binaries
                .iter()
                .map(|bin| (bin, &installed.manifest))
        })
        .collect::<HashMap<_, _>>();

    let mut flattened_bins = vec![];

    for extracted in &extracted {
        let ExtractionResult { binaries } = &extracted.extracted;

        flattened_bins.reserve(binaries.len());

        for binary in binaries {
            match seen_bins.entry(&binary.name) {
                Entry::Occupied(clashing_pkg) => {
                    if extracted.manifest.name != clashing_pkg.get().name {
                        bail!(
                            "Can't install package {} as it exposes the same binary {} than package {}",
                            extracted.manifest.name.bright_yellow(),
                            binary.name.bright_green(),
                            clashing_pkg.get().name.bright_yellow()
                        )
                    }
                }

                Entry::Vacant(vacant) => {
                    vacant.insert(&extracted.manifest);
                    flattened_bins.push(binary);
                }
            }
        }
    }

    let pb = progress_bar(
        flattened_bins.len(),
        ITEMS_PROGRESS_BAR_STYLE.clone(),
        "copying binaries...",
    );

    for ExtractedBinary { path, name } in &flattened_bins {
        pb.set_message(format!("Copying binary '{name}'..."));

        let dest = db.bin_dir().join(name);

        fs::copy(path, &dest).await.with_context(|| {
            format!(
                "Failed to copy download binary from {} to {}",
                path.display(),
                dest.display()
            )
        })?;

        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .await
                .with_context(|| {
                    format!(
                        "Failed to set binary at path '{}' executable",
                        dest.display()
                    )
                })?;
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    let pkg_count = to_install.len();
    drop(to_install);

    db.update(|db| {
        for pkg in extracted {
            let ExtractedPackage {
                manifest,
                repo_name,
                is_dep,
                version,
                extracted: ExtractionResult { binaries },
            } = pkg;

            let installed_as_dep = db
                .installed
                .get(&manifest.name)
                .map(|installed| installed.installed_as_dep)
                .unwrap_or(is_dep);

            db.installed.insert(
                manifest.name.clone(),
                InstalledPackage {
                    manifest,
                    repo_name,
                    version,
                    binaries: binaries.into_iter().map(|bin| bin.name).collect(),
                    installed_as_dep,
                    at: Zoned::now(),
                },
            );
        }
    })
    .await
    .context("Failed to register newly-installed packages")?;

    info!(
        "Successfully installed {} package(s) in {} second(s)!",
        pkg_count.to_string().bright_yellow(),
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
struct ExtractionState {
    pkg_infos: Arc<HashMap<String, ExtractionPkgInfo>>,
}

#[derive(Clone)]
struct ExtractionPkgInfo {
    repo_name: String,
    is_dep: bool,
}

async fn extract_downloaded_asset(
    manifest: PackageManifest,
    asset_infos: AssetInfos,
    asset_path: PathBuf,
    state: ExtractionState,
    pb: ProgressBar,
) -> Result<ExtractedPackage> {
    let extracted = tokio::task::spawn_blocking(move || {
        extract_asset(&asset_path, &asset_infos.typ, pb.clone())
    })
    .await
    .context("Faield to wait on Tokio task")??;

    let ExtractionPkgInfo { repo_name, is_dep } =
        state.pkg_infos.get(&manifest.name).unwrap().clone();

    Ok(ExtractedPackage {
        manifest,
        repo_name,
        is_dep,
        version: asset_infos.version,
        extracted,
    })
}

struct ExtractedPackage {
    manifest: PackageManifest,
    repo_name: String,
    version: String,
    extracted: ExtractionResult,
    is_dep: bool,
}
