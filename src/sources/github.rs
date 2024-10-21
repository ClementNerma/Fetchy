use std::{env, sync::LazyLock};

use anyhow::{bail, Context, Result};
use log::debug;
use regex::Regex;
use reqwest::{
    header::{self, HeaderMap, HeaderName, HeaderValue},
    Client, StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{repos::arch::PlatformDependent, utils::join_iter, validator::validate_asset_type};

use super::{pattern::Pattern, AssetInfos, AssetSource, AssetType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubSource {
    pub author: String,
    pub repo_name: String,
    pub asset: PlatformDependent<(Pattern, AssetType)>,
    pub version: GitHubVersionExtraction,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum GitHubVersionExtraction {
    TagName,
    ReleaseTitle,
}

static NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Za-z0-9_.-]+$").unwrap());

static GITHUB_BASE_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    HeaderMap::from_iter([
        (
            HeaderName::from_static("x-github-api-version"),
            HeaderValue::from_static("2022-11-28"),
        ),
        (header::USER_AGENT, HeaderValue::from_static("FetchyCliApp")),
    ])
});

impl AssetSource for GitHubSource {
    fn validate(&self) -> Vec<String> {
        let Self {
            author,
            repo_name,
            asset,
            version: _,
        } = self;

        let mut errors = vec![];

        if !NAME_REGEX.is_match(author) {
            errors.push(format!(
                "Author name {author:?} contains invalid character(s)"
            ));
        }

        if !NAME_REGEX.is_match(repo_name) {
            errors.push(format!(
                "Repository name {author:?} contains invalid character(s)"
            ));
        }

        for (_, asset) in asset.values() {
            validate_asset_type(asset, &mut errors);
        }

        errors
    }

    async fn fetch_infos(&self) -> Result<AssetInfos> {
        let Self {
            author,
            repo_name,
            asset,
            version,
        } = self;

        let (asset_pattern, asset_content) = asset.get_for_current_platform()?;

        let mut headers = GITHUB_BASE_HEADERS.clone();

        if let Some(access_token) = env::var("FETCHY_GITHUB_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
        {
            headers.append(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {access_token}"))
                    .context("Failed to use access token as a header value")?,
            );
        }

        let release = fetch_latest_release(author, repo_name, headers.clone())
            .await
            .with_context(|| {
                format!("Failed to fetch latest release of repo '{author}/{repo_name}'")
            })?;

        if release.assets.is_empty() {
            bail!("No asset found in latest release in repo {author}/{repo_name}");
        }

        let (filtered_assets, non_matching_assets) = release
            .assets
            .into_iter()
            .partition::<Vec<_>, _>(|asset| asset_pattern.is_match(&asset.name));

        if filtered_assets.len() > 1 {
            bail!(
                "Multiple entries matched the asset regex ({}):\n{}",
                asset_pattern.to_string(),
                join_iter(
                    filtered_assets
                        .into_iter()
                        .map(|asset| format!("* {}", asset.name)),
                    "\n"
                )
            )
        }

        let asset = filtered_assets.into_iter().next().with_context(|| {
            format!(
                "No entry matched the release regex ({}) in repo {author}/{repo_name}.\nFound non-matching assets:\n\n{}",
                **asset_pattern,
                join_iter(non_matching_assets.iter().map(|asset| format!("* {}", asset.name)), "\n")
            )
        })?;

        let version = match version {
            GitHubVersionExtraction::TagName => release.tag_name,
            GitHubVersionExtraction::ReleaseTitle => {
                release.name.context("Fetched released has no title")?
            }
        };

        Ok(AssetInfos {
            url: asset.browser_download_url,
            headers,
            version,
            typ: asset_content.clone(),
        })
    }
}

async fn fetch_latest_release(
    author: &str,
    repo_name: &str,
    headers: HeaderMap<HeaderValue>,
) -> Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{author}/{repo_name}/releases/latest");

    debug!("Fetching latest release from: {url}");

    let resp = Client::new()
        .get(url)
        .headers(headers)
        .send()
        .await
        .with_context(|| {
            format!("Failed to fetch latest release of repo '{author}/{repo_name}'")
        })?;

    let status = resp.status();

    let text = resp
        .text()
        .await
        .context("Failed to decode response as text")?;

    if status != StatusCode::OK {
        bail!("Server returned an error:\n{text}");
    }

    serde_json::from_str(&text).context("Failed to parse response as JSON")
}

#[derive(Serialize, Deserialize)]
struct GitHubRelease {
    name: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
    tag_name: String,
}

#[derive(Serialize, Deserialize)]
struct GitHubReleaseAsset {
    browser_download_url: String,
    name: String,
}
