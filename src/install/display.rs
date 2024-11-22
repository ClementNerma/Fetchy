use comfy_table::{presets, Cell, Color, ContentArrangement, Table};
use log::info;
use owo_colors::OwoColorize;

use crate::{
    db::data::InstalledPackage,
    install::phases::{PackagesToInstall, UntouchedPackages},
    resolver::ResolvedPkg,
    sources::AssetInfos,
};

use super::{phases::InstallPhases, InstalledPackagesHandling};

pub(super) fn display_install_phases(
    phases: &InstallPhases,
    installed_pkgs_handling: InstalledPackagesHandling,
    discreet: bool,
) {
    let InstallPhases {
        untouched:
            UntouchedPackages {
                already_installed,
                already_installed_deps,
                no_update_needed,
                update_available,
            },
        to_install:
            PackagesToInstall {
                missing_pkgs,
                missing_deps,
                needs_updating,
                reinstall,
            },
    } = phases;

    display_pkg_phase(
        "The following NEW package(s) will be installed",
        missing_pkgs.iter().map(|(p, _)| *p),
    );

    display_pkg_phase(
        "The following NEW dependency package(s) will be installed",
        missing_deps.iter().map(|(p, _)| *p),
    );

    display_update_phase(
        "The following package(s) will be updated",
        needs_updating
            .iter()
            .map(|(resolved, asset_infos, installed)| (*resolved, asset_infos, *installed)),
    );

    display_pkg_phase(
        "The following installed package(s) will be reinstalled",
        reinstall.iter().map(|(p, _, _)| *p),
    );

    display_update_phase(
        "The following package(s) have an available update",
        update_available
            .iter()
            .map(|(resolved, asset_infos, installed)| (*resolved, asset_infos, *installed)),
    );

    if !discreet {
        if matches!(
            installed_pkgs_handling,
            InstalledPackagesHandling::CheckUpdates
        ) {
            display_pkg_phase(
                "The following package(s) are already on their latest version",
                no_update_needed.iter().copied(),
            );
        }

        if matches!(installed_pkgs_handling, InstalledPackagesHandling::Ignore) {
            display_pkg_phase(
                "The following package(s) are already installed and require no action",
                already_installed.iter().copied(),
            );

            display_pkg_phase(
                "The following dependency package(s) are already installed and require no action",
                already_installed_deps.iter().copied(),
            );
        }
    }
}

static PKGS_PER_ROW: usize = 5;

pub fn display_pkg_phase<'a, 'b>(title: &str, content: impl Iterator<Item = ResolvedPkg<'a, 'b>>) {
    let content = content.collect::<Vec<_>>();

    // Don't display categories with no package
    if content.is_empty() {
        return;
    }

    let mut pkgs_table = Table::new();

    pkgs_table
        // Remove borders
        .load_preset(presets::NOTHING)
        // Ask table to take as much width as possible
        .set_content_arrangement(ContentArrangement::Dynamic)
        .add_rows(content.chunks(PKGS_PER_ROW).map(|chunk| {
            chunk
                .iter()
                .map(|pkg| Cell::new(&pkg.manifest.name).fg(Color::Yellow))
        }));

    info!("{}\n\n{pkgs_table}\n", format!("{title}:").bright_blue());
}

pub fn display_update_phase<'a, 'b, 'c, 'd>(
    title: &str,
    content: impl ExactSizeIterator<Item = (ResolvedPkg<'a, 'b>, &'c AssetInfos, &'d InstalledPackage)>,
) {
    // Don't display categories with no package
    if content.len() == 0 {
        return;
    }

    let mut pkgs_table = Table::new();

    pkgs_table
        // Remove borders
        .load_preset(presets::NOTHING)
        // Ask table to take as much width as possible
        .set_content_arrangement(ContentArrangement::Dynamic)
        .add_rows(content.map(|(resolved, asset_infos, installed)| {
            [
                Cell::new(&resolved.manifest.name).fg(Color::Yellow),
                Cell::new(&installed.version).fg(Color::DarkCyan),
                Cell::new("->").fg(Color::Green),
                Cell::new(&asset_infos.version).fg(Color::DarkCyan),
            ]
        }));

    info!("{}\n\n{pkgs_table}\n", format!("{title}:").bright_blue());
}
