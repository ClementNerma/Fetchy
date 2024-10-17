use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::{
    app_data::{AppState, Repositories, SourcedRepository},
    fetcher::{fetch_package_asset_infos, AssetInfos},
    info,
    installer::InstalledPackagesAction,
    repository::Package,
    utils::progress_bar,
};

#[derive(Default)]
pub struct InstallPhases<'a> {
    pub already_installed: Vec<ResolvedPkg<'a>>,
    pub no_update_needed: Vec<ResolvedPkg<'a>>,
    pub update_available: Vec<ResolvedPkg<'a>>,
    pub update: Vec<(ResolvedPkg<'a>, AssetInfos)>,
    pub install_new: Vec<(ResolvedPkg<'a>, AssetInfos)>,
    pub install_deps: Vec<(ResolvedPkg<'a>, AssetInfos)>,
    pub reinstall: Vec<(ResolvedPkg<'a>, AssetInfos)>,
}

pub fn build_install_phases<'a>(
    names: &[String],
    repositories: &'a Repositories,
    for_already_installed: InstalledPackagesAction,
    app_state: &AppState,
) -> Result<InstallPhases<'a>> {
    info!("Fetching informations about packages...");

    let resolved_with_deps = names
        .par_iter()
        .map(|name| resolve_package_with_dependencies(name, repositories))
        .collect::<Result<Vec<_>, _>>()?;

    let pb = progress_bar(resolved_with_deps.len(), "{pos:>2}/{len:2}");

    let handled = Arc::new(Mutex::new(HashSet::new()));

    let phases = Arc::new(Mutex::new(InstallPhases::default()));
    let phases_bis = Arc::clone(&phases);

    resolved_with_deps
        .into_par_iter()
        .flatten()
        .map(|resolved: ResolvedPkg| -> Result<()> {
            // Dependencies are treated differently than normal packages
            // Unless they're specified as part of the list of packages to install
            if resolved.dependency_of.is_some() && names.contains(&resolved.package.name) {
                return Ok(());
            }

            // Ensure we don't handle a package twice
            // (duplicate name or same dependency for two packages for instance)
            if !handled.lock().unwrap().insert(&resolved.package.name) {
                return Ok(());
            }

            // Check if the package is already installed
            let already_installed = app_state
                .installed
                .iter()
                .find(|pkg| pkg.pkg_name == resolved.package.name);

            let mut phases = phases
                .lock()
                .map_err(|err| anyhow!("failed to lock mutex: {err}"))?;

            match (already_installed, resolved.dependency_of) {
                // Not installed dependency
                (None, Some(_)) => {
                    let infos = fetch_package_asset_infos(resolved.package)?;
                    phases.install_deps.push((resolved, infos));
                }

                // Already installed dependency
                (Some(_), Some(_)) => {}

                // Not installed normal package
                (None, None) => {
                    let infos = fetch_package_asset_infos(resolved.package)?;
                    phases.install_new.push((resolved, infos));
                }

                // Already installed normal package
                (Some(already_installed), None) => match for_already_installed {
                    InstalledPackagesAction::Ignore => {
                        phases.already_installed.push(resolved);
                    }

                    InstalledPackagesAction::Update => {
                        // Show if there's an update and that's all
                        let infos = fetch_package_asset_infos(resolved.package)?;

                        if infos.version == already_installed.version {
                            phases.no_update_needed.push(resolved);
                        } else {
                            phases.update.push((resolved, infos));
                        }
                    }

                    InstalledPackagesAction::Reinstall => {
                        let infos = fetch_package_asset_infos(resolved.package)?;
                        phases.reinstall.push((resolved, infos));
                    }
                },
            }

            Ok(())
        })
        .inspect(|result| {
            if result.is_ok() {
                pb.inc(1);
            }
        })
        .collect::<Result<()>>()?;

    pb.finish_and_clear();

    Ok(Arc::try_unwrap(phases_bis)
        .map_err(|_| anyhow!("failed to unwrap Arc"))?
        .into_inner()
        .unwrap())
}

fn find_package<'a>(
    name: &str,
    repositories: &'a Repositories,
) -> Result<(&'a SourcedRepository, &'a Package)> {
    let candidates = repositories
        .list
        .iter()
        .flat_map(|repo| {
            repo.content
                .packages
                .iter()
                .filter(|package| package.name == name)
                .map(move |package| (repo, package))
        })
        .collect::<Vec<_>>();

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

fn resolve_package_with_dependencies<'a>(
    name: &str,
    repositories: &'a Repositories,
) -> Result<Vec<ResolvedPkg<'a>>> {
    let (from_repo, package) = find_package(name, repositories)?;

    let mut out = vec![ResolvedPkg {
        from_repo,
        package,
        dependency_of: None,
    }];

    let deps = resolve_package_dependencies(package, repositories)?;

    out.extend(deps);

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
pub struct ResolvedPkg<'a> {
    pub from_repo: &'a SourcedRepository,
    pub package: &'a Package,
    pub dependency_of: Option<&'a str>,
}
