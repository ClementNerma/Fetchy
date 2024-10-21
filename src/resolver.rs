use std::collections::{btree_map::Entry, BTreeMap, HashMap, HashSet, VecDeque};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::{
    db::data::InstalledPackage,
    repos::ast::{PackageManifest, Repository},
    utils::join_iter,
};

pub fn resolve_pkg_by_name(
    name: impl AsRef<str>,
    repos: &BTreeMap<String, Repository>,
) -> Result<ResolvedPkg> {
    let name = name.as_ref();

    let mut candidates = repos
        .values()
        .filter_map(|repo| repo.packages.get(name).map(|pkg| (pkg, repo)));

    let (manifest, repository) = candidates
        .next()
        .with_context(|| format!("Package {} was not found", name.bright_yellow()))?;

    // This does not allocate if there are no clashing packages
    let clashing = candidates.collect::<Vec<_>>();

    if !clashing.is_empty() {
        bail!(
            "Package {} exists in multiple repositories:\n\n{}",
            name.bright_yellow(),
            join_iter(
                clashing
                    .into_iter()
                    .map(|(_, repo)| format!("* {}", repo.name.bright_yellow())),
                "\n"
            )
        );
    }

    Ok(ResolvedPkg {
        manifest,
        repository,
        is_dep: false,
    })
}

pub fn resolve_pkgs_by_name<'a, S: AsRef<str>>(
    names: &[S],
    repos: &'a BTreeMap<String, Repository>,
) -> Result<Vec<ResolvedPkg<'a, 'a>>> {
    names
        .iter()
        .map(|name| resolve_pkg_by_name(name, repos))
        .collect::<Result<Vec<_>, _>>()
}

pub fn resolve_pkgs_by_name_with_deps<'a, S: AsRef<str>>(
    names: &[S],
    repos: &'a BTreeMap<String, Repository>,
) -> Result<Vec<ResolvedPkg<'a, 'a>>> {
    resolve_pkgs_with_deps(&resolve_pkgs_by_name(names, repos)?)
}

// TODO: show paths in errors
pub fn resolve_pkgs_with_deps<
    'a,
    // This bound is required as we return packages from the original list ('a)
    // but also from the provided repositories ('b)
    'b: 'a,
>(
    pkgs: &[ResolvedPkg<'a, 'b>],
) -> Result<Vec<ResolvedPkg<'a, 'b>>> {
    // List of packages to handle
    let mut queue = pkgs.iter().cloned().collect::<VecDeque<_>>();

    // List of packages that have already been handled with their associated repository
    // Used to detect conflicts when we need two packages with the same name but from different repositories
    let mut handled = BTreeMap::<&str, ResolvedPkg>::new();

    // Process the queue, item by item
    // Each package is pushed to the output, and all its dependencies are queued
    // The `handled` variable ensures we don't push packages twice
    while let Some(resolved) = queue.pop_front() {
        let ResolvedPkg {
            manifest,
            repository,
            is_dep: _,
        } = &resolved;

        match handled.entry(&manifest.name) {
            Entry::Occupied(handled) => {
                if handled.get().repository.name != repository.name {
                    bail!(
                        "Dependencies graph resolves to two packages named {} from repository {} and {}",
                        manifest.name.bright_yellow(),
                        repository.name.bright_yellow(),
                        handled.get().repository.name.bright_blue()
                    );
                }
            }

            Entry::Vacant(vacant) => {
                vacant.insert(resolved);

                for dep_name in &resolved.manifest.depends_on {
                    if let Some(existing_pkg) =
                        pkgs.iter().find(|pkg| pkg.manifest.name == *dep_name)
                    {
                        if existing_pkg.repository.name != repository.name {
                            bail!(
                                    "Requested package {} from repository {} clashes with package {} which has a dependency of the same name but from repository {}",
                                    dep_name.bright_yellow(),
                                    existing_pkg.repository.name.bright_yellow(),
                                    manifest.name.bright_yellow(),
                                    repository.name.bright_blue()
                                );
                        }
                    }

                    let dep_manifest = repository.packages
                            .get(dep_name)
                            .with_context(|| format!(
                                "Failed to find package {} which is a dependency of {} in repository {}",
                                dep_name.bright_yellow(),
                                manifest.name.bright_yellow(),
                                repository.name.bright_blue()
                            ))?;

                    queue.push_back(ResolvedPkg {
                        manifest: dep_manifest,
                        repository,
                        is_dep: true,
                    });
                }
            }
        }
    }

    Ok(handled.into_values().collect())
}

pub fn resolve_installed_pkg<'a, 'b>(
    installed: &'a InstalledPackage,
    repos: &'b BTreeMap<String, Repository>,
) -> Result<ResolvedPkg<'a, 'b>> {
    let repository = repos.get(&installed.repo_name).with_context(|| {
        format!(
            "Installed package {} belong to unknown repository {}",
            installed.manifest.name.bright_yellow(),
            installed.repo_name.bright_blue()
        )
    })?;

    Ok(ResolvedPkg {
        manifest: &installed.manifest,
        repository,
        is_dep: installed.installed_as_dep,
    })
}

pub fn resolve_installed_pkgs<'a, 'b>(
    pkgs: impl Iterator<Item = &'a InstalledPackage>,
    repos: &'b BTreeMap<String, Repository>,
) -> Result<Vec<(ResolvedPkg<'a, 'b>, &'a InstalledPackage)>> {
    pkgs.map(|installed| {
        resolve_installed_pkg(installed, repos).map(|resolved| (resolved, installed))
    })
    .collect()
}

pub fn resolve_installed_pkg_by_name<'a, 'b>(
    name: impl AsRef<str>,
    installed: &'a BTreeMap<String, InstalledPackage>,
    repos: &'b BTreeMap<String, Repository>,
) -> Result<(ResolvedPkg<'a, 'b>, &'a InstalledPackage)> {
    let name = name.as_ref();

    installed
        .get(name)
        .with_context(|| {
            format!(
                "Package {} is not installed{}",
                name.bright_yellow(),
                if repos.values().any(|repo| repo.packages.contains_key(name)) {
                    ""
                } else {
                    "(and does not exist in any registered repository)"
                }
            )
        })
        .and_then(|installed| {
            resolve_installed_pkg(installed, repos).map(|resolved| (resolved, installed))
        })
}

pub fn resolve_installed_pkgs_by_name<'a, 'b>(
    names: &[impl AsRef<str>],
    installed: &'a BTreeMap<String, InstalledPackage>,
    repos: &'b BTreeMap<String, Repository>,
) -> Result<Vec<(ResolvedPkg<'a, 'b>, &'a InstalledPackage)>> {
    names
        .iter()
        .map(|name| resolve_installed_pkg_by_name(name, installed, repos))
        .collect()
}

pub fn refresh_pkg<'b>(resolved: ResolvedPkg<'_, 'b>) -> Result<ResolvedPkg<'b, 'b>> {
    let ResolvedPkg {
        manifest,
        repository,
        is_dep,
    } = resolved;

    let manifest = repository.packages.get(&manifest.name).with_context(|| {
        format!(
            "Package {} was not found in repository {}",
            manifest.name.bright_blue(),
            repository.name.bright_blue()
        )
    })?;

    Ok(ResolvedPkg {
        manifest,
        repository,
        is_dep,
    })
}

pub fn build_pkgs_reverse_deps_map<'a>(
    pkgs: impl Iterator<Item = &'a PackageManifest>,
) -> HashMap<&'a str, HashSet<&'a str>> {
    let mut deps_map = HashMap::<&str, HashSet<&str>>::new();

    for manifest in pkgs {
        for dep in &manifest.depends_on {
            deps_map.entry(dep).or_default().insert(&manifest.name);
        }
    }

    deps_map
}

pub fn compute_no_longer_needed_deps<'a, 'b>(
    installed: &[(ResolvedPkg<'a, 'b>, &'a InstalledPackage)],
    uninstalling: &HashSet<&'a str>,
    reverse_deps_map: &HashMap<&'a str, HashSet<&'a str>>,
) -> Vec<(ResolvedPkg<'a, 'b>, &'a InstalledPackage)> {
    let mut uninstalling = uninstalling.clone();
    let mut out = vec![];

    compute_no_longer_needed_deps_subroutine(
        installed,
        reverse_deps_map,
        &mut uninstalling,
        &mut out,
    );

    out
}

fn compute_no_longer_needed_deps_subroutine<'a, 'b>(
    installed: &[(ResolvedPkg<'a, 'b>, &'a InstalledPackage)],
    reverse_deps_map: &HashMap<&'a str, HashSet<&'a str>>,
    uninstalling: &mut HashSet<&'a str>,
    out: &mut Vec<(ResolvedPkg<'a, 'b>, &'a InstalledPackage)>,
) {
    let start = out.len();

    out.extend(
        installed
            .iter()
            .filter(|(_, installed)| {
                installed.installed_as_dep
                    && !uninstalling.contains(installed.manifest.name.as_str())
                    && match reverse_deps_map.get(installed.manifest.name.as_str()) {
                        None => true,
                        Some(deps_by) => deps_by.difference(uninstalling).count() == 0,
                    }
            })
            .map(|(resolved, installed)| (*resolved, *installed)),
    );

    if out.len() > start {
        uninstalling.extend(
            out.iter()
                .skip(start)
                .map(|(resolved, _)| resolved.manifest.name.as_str()),
        );

        compute_no_longer_needed_deps_subroutine(installed, reverse_deps_map, uninstalling, out);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedPkg<'a, 'b> {
    pub manifest: &'a PackageManifest,
    pub repository: &'b Repository,
    pub is_dep: bool,
}
