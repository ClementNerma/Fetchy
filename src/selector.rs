use anyhow::{bail, Result};

use crate::app_data::{AppState, InstalledPackage};

pub fn find_installed_packages<'a>(
    app_state: &'a mut AppState,
    names: &[String],
) -> Result<Vec<&'a mut InstalledPackage>> {
    let packages = app_state
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
        if !packages.iter().any(|package| &package.pkg_name == name) {
            bail!("Package '{name}' was not found");
        }
    }

    Ok(packages)
}
