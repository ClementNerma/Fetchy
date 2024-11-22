use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use parsy::{ErrorReport, Parser};
use serde::{Deserialize, Serialize};
use tokio::{fs, task::JoinSet};

use crate::{
    repos::{ast::Repository, parser::repository},
    utils::{join_fallible_ordered_set, join_iter, progress_bar, ITEMS_PROGRESS_BAR_STYLE},
    validator::validate_repository,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySource {
    pub location: RepositoryLocation,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub enum RepositoryLocation {
    File(PathBuf),
    // Url(String),
}

impl PartialEq for RepositoryLocation {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(a), Self::File(b)) => a == b,
        }
    }
}

impl std::fmt::Display for RepositoryLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(f, "file '{}'", path.display()),
        }
    }
}

pub async fn fetch_repository(source: &RepositorySource) -> Result<Repository> {
    let RepositorySource { location, json } = source;

    let repo_str = match location {
        RepositoryLocation::File(path) => {
            if !path.is_file() {
                bail!("Provided repository file does not exist");
            }

            fs::read_to_string(path)
                .await
                .context("Failed to read provided repository file")?
        }
    };

    let parsed = if *json {
        serde_json::from_str(&repo_str)
            .with_context(|| format!("Failed to parse JSON repository at {location}"))?
    } else {
        repository()
            .parse_str(&repo_str)
            .map(|parsed| parsed.data)
            .map_err(|err| {
                let location = format!("{location}");
                let err = ErrorReport::parsing_error(&repo_str, &location, &err);

                // NOTE: I didn't find a way to reset styles here
                // Error messages are printed in red, but I'd like to only print the error using the
                // terminal's default style here. But even using an ANSI escape code to reset style
                // doesn't work for some reason...
                anyhow!("{}", format!("{err}").white())
            })?
    };

    if let Err(errors) = validate_repository(&parsed) {
        bail!(
            "Found {} issues with the repository:\n\n{}",
            errors.len().to_string().bright_yellow(),
            join_iter(
                errors
                    .iter()
                    .map(|error| format!("{} {error}", "*".bright_yellow())),
                "\n"
            )
        )
    }

    Ok(parsed)
}

pub async fn fetch_repositories(
    sources: impl ExactSizeIterator<Item = RepositorySource>,
) -> Result<Vec<Repository>> {
    let pb = progress_bar(
        sources.len(),
        ITEMS_PROGRESS_BAR_STYLE.clone(),
        "Fetching repositories...",
    );

    let mut tasks = JoinSet::new();

    for (i, source) in sources.enumerate() {
        let pb = pb.clone();

        tasks.spawn(async move {
            let result = fetch_repository(&source).await;
            pb.inc(1);
            result.map(|repo| (i, repo))
        });
    }

    join_fallible_ordered_set(tasks)
        .await
        .inspect(|_| pb.finish_and_clear())
        .inspect_err(|_| pb.abandon())
}
