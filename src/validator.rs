use std::{fmt::Display, sync::LazyLock};

use colored::Colorize;
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
        ($typ: expr, $name: expr, $colorize: ident) => {
            if let Err(err) = validate_name($typ, $name, Colorize::$colorize) {
                errors.push(err);
            }
        };
    }

    let Repository {
        name,
        description: _,
        packages,
    } = repo;

    validate_name!("Repository", name, bright_blue);

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

        validate_name!("Package", name, bright_yellow);

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
            DownloadSource::Direct(params) => DirectSource::validate_params(params),
            DownloadSource::GitHub(params) => GitHubSource::validate_params(params),
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
    validate_name("Binary", bin_name, Colorize::bright_green)
}

fn validate_name<'a, T: Display>(
    typ: &str,
    name: &'a str,
    colorize: impl FnOnce(&'a str) -> T,
) -> Result<(), String> {
    if NAME_REGEX.is_match(name) {
        Ok(())
    } else {
        Err(
            format!(
                "{typ} name {} is invalid (name should only contain lowercase and uppercase letters, digits, underscores and dashes)",
                colorize(name)
            )
        )
    }
}
