use anyhow::{bail, Result};
use owo_colors::OwoColorize;

use crate::{
    db::{data::InstalledPackage, Db},
    resolver::ResolvedPkg,
    sources::AssetInfos,
};

use super::fetch_infos::fetch_resolved_pkg_infos;

#[derive(Default, Debug)]
pub struct InstallPhases<'a, 'b, 'c> {
    pub untouched: UntouchedPackages<'a, 'b, 'c>,
    pub to_install: PackagesToInstall<'a, 'b, 'c>,
}

#[derive(Default, Debug)]
pub struct UntouchedPackages<'a, 'b, 'c> {
    pub already_installed: Vec<ResolvedPkg<'a, 'b>>,
    pub already_installed_deps: Vec<ResolvedPkg<'a, 'b>>,
    pub no_update_needed: Vec<ResolvedPkg<'a, 'b>>,
    pub update_available: Vec<(ResolvedPkg<'a, 'b>, AssetInfos, &'c InstalledPackage)>,
}

#[derive(Default, Debug)]
pub struct PackagesToInstall<'a, 'b, 'c> {
    pub missing_pkgs: Vec<(ResolvedPkg<'a, 'b>, AssetInfos)>,
    pub missing_deps: Vec<(ResolvedPkg<'a, 'b>, AssetInfos)>,
    pub needs_updating: Vec<(ResolvedPkg<'a, 'b>, AssetInfos, &'c InstalledPackage)>,
    pub reinstall: Vec<(ResolvedPkg<'a, 'b>, AssetInfos, &'c InstalledPackage)>,
}

#[derive(Debug, Clone, Copy)]
pub enum InstalledPackagesHandling {
    Ignore,
    CheckUpdates,
    Update,
    Reinstall,
}

pub async fn compute_install_phases<'a, 'b, 'c>(
    pkgs: Vec<ResolvedPkg<'a, 'b>>,
    installed_pkgs_handling: InstalledPackagesHandling,
    db: &'c Db,
) -> Result<InstallPhases<'a, 'b, 'c>> {
    for pkg in &pkgs {
        if let Some(installed) = db.installed.get(&pkg.manifest.name) {
            if installed.repo_name != pkg.repository.name {
                bail!("Trying to install package {} from repository {}{}, but another package of the same name from repository {} is already installed",
                    pkg.manifest.name.bright_yellow(),
                    pkg.repository.name.bright_blue(),
                    if pkg.is_dep { " as a dependency" } else { "" },
                    installed.repo_name.bright_blue()
                );
            }
        }
    }

    // Skip the whole process if all manually-specified packages are already installed
    // and the action mode is set to 'ignore'
    // This also ignores missing dependencies (e.g. a new package update changed some dependencies)
    if matches!(installed_pkgs_handling, InstalledPackagesHandling::Ignore)
        && pkgs
            .iter()
            .filter(|pkg| !pkg.is_dep)
            .all(|pkg| db.installed.contains_key(&pkg.manifest.name))
    {
        let (already_installed_deps, already_installed) =
            pkgs.into_iter().partition(|pkg| pkg.is_dep);

        return Ok(InstallPhases {
            untouched: UntouchedPackages {
                already_installed,
                already_installed_deps,
                no_update_needed: vec![],
                update_available: vec![],
            },
            to_install: PackagesToInstall::default(),
        });
    }

    // At this point we know that at least one non-dependency package is missing

    let (installed, missing) = match installed_pkgs_handling {
        // If action mode is set to 'Ignore', we identify the already-installed and missing packages to check if there is anything to do
        InstalledPackagesHandling::Ignore => pkgs
            .into_iter()
            .partition(|pkg| db.installed.contains_key(&pkg.manifest.name)),

        // If the mode is set to any other value, we need to fetch informations about all packages in all cases
        InstalledPackagesHandling::CheckUpdates
        | InstalledPackagesHandling::Update
        | InstalledPackagesHandling::Reinstall => (vec![], pkgs),
    };

    let (already_installed_deps, already_installed) =
        installed.into_iter().partition(|pkg| pkg.is_dep);

    // If there's no package to check, we can stop right here
    if missing.is_empty() {
        return Ok(InstallPhases {
            untouched: UntouchedPackages {
                already_installed,
                already_installed_deps,
                no_update_needed: vec![],
                update_available: vec![],
            },
            to_install: PackagesToInstall::default(),
        });
    }

    let mut phases = InstallPhases {
        untouched: UntouchedPackages {
            already_installed,
            already_installed_deps,
            ..Default::default()
        },
        to_install: PackagesToInstall::default(),
    };

    // Fetch informations about packages that require it
    for (pkg, asset_infos) in fetch_resolved_pkg_infos(&missing).await? {
        match db.installed.get(&pkg.manifest.name) {
            None => {
                if pkg.is_dep {
                    phases.to_install.missing_deps.push((pkg, asset_infos));
                } else {
                    phases.to_install.missing_pkgs.push((pkg, asset_infos));
                }
            }

            Some(already_installed) => {
                match installed_pkgs_handling {
                    InstalledPackagesHandling::Ignore => {
                        assert!(!pkg.is_dep);
                    }

                    InstalledPackagesHandling::CheckUpdates => {
                        // Show if there's an update and that's all
                        if asset_infos.version == already_installed.version {
                            phases.untouched.no_update_needed.push(pkg);
                        } else {
                            phases.untouched.update_available.push((
                                pkg,
                                asset_infos,
                                already_installed,
                            ));
                        }
                    }

                    InstalledPackagesHandling::Update => {
                        // Show if there's an update and that's all
                        if asset_infos.version == already_installed.version {
                            phases.untouched.no_update_needed.push(pkg);
                        } else {
                            phases.to_install.needs_updating.push((
                                pkg,
                                asset_infos,
                                already_installed,
                            ));
                        }
                    }

                    InstalledPackagesHandling::Reinstall => {
                        // Don't reinstall unchanged dependencies
                        if pkg.is_dep && asset_infos.version == already_installed.version {
                            phases.untouched.already_installed_deps.push(pkg);
                        } else {
                            phases
                                .to_install
                                .reinstall
                                .push((pkg, asset_infos, already_installed));
                        }
                    }
                }
            }
        }
    }

    Ok(phases)
}
