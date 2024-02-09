use anyhow::{bail, Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};

use super::AssetSource;
use crate::{
    arch::PlatformDependent, debug, fetcher::AssetInfos, pattern::Pattern,
    repository::FileExtraction,
};

#[derive(Serialize, Deserialize)]
pub struct GitHubSourceParams {
    pub author: String,
    pub repo_name: String,
    pub asset: PlatformDependent<(Pattern, FileExtraction)>,
    pub version: GitHubVersionExtraction,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum GitHubVersionExtraction {
    TagName,
    ReleaseTitle,
}

pub struct GitHubSource;

impl AssetSource for GitHubSource {
    type Params = GitHubSourceParams;

    // fn make_parser() -> Box<dyn parsy::Parser<Self>> {
    //     todo!()
    // }

    fn fetch(params: &Self::Params) -> anyhow::Result<AssetInfos> {
        let GitHubSourceParams {
            author,
            repo_name,
            asset,
            version,
        } = params;

        let (asset_pattern, extraction) = asset.get_for_current_platform()?;

        let release = fetch_latest_release(author, repo_name)?;

        if release.assets.is_empty() {
            bail!("No asset found in latest release in repo {author}/{repo_name}");
        }

        let (filtered_assets, non_matching_assets) = release
            .assets
            .into_iter()
            .partition::<Vec<_>, _>(|asset| asset_pattern.regex.is_match(&asset.name));

        if filtered_assets.len() > 1 {
            bail!(
                "Multiple entries matched the asset regex ({}):\n{}",
                asset_pattern.source,
                filtered_assets
                    .into_iter()
                    .map(|asset| format!("* {}", asset.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }

        let asset = filtered_assets.into_iter().next().with_context(|| {
            format!(
                "No entry matched the release regex ({}) in repo {author}/{repo_name}.\nFound non-matching assets:\n\n{}",
                asset_pattern.source,
                non_matching_assets.iter().map(|asset| format!("* {}", asset.name)).collect::<Vec<_>>().join("\n")
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
            version,
            extraction: extraction.clone(),
        })
    }
}

fn fetch_latest_release(author: &str, repo_name: &str) -> Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{author}/{repo_name}/releases/latest");

    debug!("Fetching latest release from: {url}");

    let resp = Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, "FetchyAppUserAgent")
        .send()
        .with_context(|| {
            format!("Failed to fetch latest release of repo '{author}/{repo_name}'")
        })?;

    let status = resp.status();

    let text = resp.text().with_context(|| {
        format!("Failed to fetch latest release of repo '{author}/{repo_name}' as text")
    })?;

    if status != StatusCode::OK {
        bail!("Failed to fetch latest release of repo '{author}/{repo_name}':\n{text}");
    }

    serde_json::from_str(&text).with_context(|| {
        format!("Failed to parse latest release of repo '{author}/{repo_name}' as JSON")
    })
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
