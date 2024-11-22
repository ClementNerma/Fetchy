use std::sync::LazyLock;

use owo_colors::{colors::BrightGreen, Color, OwoColorize};
use regex::Regex;

use crate::{
    repos::ast::{DownloadSource, PackageManifest, Repository},
    sources::{
        direct::DirectSource, github::GitHubSource, AssetSource, AssetType, BinaryInArchive,
    },
};

static NAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^([a-zA-Z0-9\-_.]+)$"#).unwrap());

// TODO: detect cyclic dependencies
pub fn validate_repository(repo: &Repository) -> Result<(), Vec<String>> {
    let mut errors = vec![];

    macro_rules! validate_name {
        ($typ: expr, $name: expr, $color_type: ident) => {
            if let Err(err) = validate_name::<owo_colors::colors::$color_type>($typ, $name) {
                errors.push(err);
            }
        };
    }

    let Repository {
        name,
        description: _,
        packages,
    } = repo;

    validate_name!("Repository", name, BrightBlue);

    for (name, manifest) in packages {
        if *name != manifest.name {
            errors.push(format!(
                "Repository contains package {} under name {}",
                name.bright_yellow(),
                manifest.name.bright_yellow()
            ));
        }
    }

    // Return errors early as they would cause problems with later checkings
    if !errors.is_empty() {
        return Err(errors);
    }

    for manifest in packages.values() {
        let PackageManifest {
            name,
            source,
            depends_on,
        } = manifest;

        validate_name!("Package", name, BrightYellow);

        for depend_on in depends_on {
            if !repo.packages.contains_key(depend_on) {
                errors.push(format!(
                    "Package {} depends on package {} which was not found in the repository",
                    name.bright_yellow(),
                    depend_on.bright_yellow()
                ));
            }
        }

        let param_errors = match source {
            DownloadSource::Direct(params) => DirectSource::validate(params),
            DownloadSource::GitHub(params) => GitHubSource::validate(params),
        };

        errors.extend(
            param_errors
                .iter()
                .map(|err| format!("In package {}: {err}", name.bright_yellow())),
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_asset_type(typ: &AssetType, errors: &mut Vec<String>) {
    match typ {
        AssetType::Binary { copy_as } => {
            if let Err(err) = validate_binary_name(copy_as) {
                errors.push(err);
            }
        }

        AssetType::Archive { format: _, files } => {
            for file in files {
                let BinaryInArchive {
                    path_matcher: _,
                    rename_as,
                } = file;

                if let Some(rename_as) = rename_as {
                    if let Err(err) = validate_binary_name(rename_as) {
                        errors.push(err);
                    }
                }
            }
        }
    }
}

pub fn validate_binary_name(bin_name: &str) -> Result<(), String> {
    validate_name::<BrightGreen>("Binary", bin_name)
}

fn validate_name<C: Color>(typ: &str, name: &str) -> Result<(), String> {
    if NAME_REGEX.is_match(name) {
        Ok(())
    } else {
        Err(
            format!(
                "{typ} name {} is invalid (name should only contain lowercase and uppercase letters, digits, underscores and dashes)",
                name.fg::<C>()
            )
        )
    }
}
