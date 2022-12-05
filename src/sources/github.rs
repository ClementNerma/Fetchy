use anyhow::{bail, Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{fetcher::Asset, pattern::Pattern};

pub async fn fetch_latest_release_asset(
    author: &str,
    repo_name: &str,
    asset_pattern: &Pattern,
) -> Result<Asset> {
    let release = fetch_latest_release(author, repo_name).await?;

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

    Ok(Asset {
        url: asset.browser_download_url,
        filename: Some(asset.name),
        release_title: release.name,
        tag_name: Some(release.tag_name),
    })
}

async fn fetch_latest_release(author: &str, repo_name: &str) -> Result<GitHubRelease> {
    let resp = Client::new()
        .get(&format!(
            "https://api.github.com/repos/{author}/{repo_name}/releases/latest"
        ))
        .header(reqwest::header::USER_AGENT, "FetchyAppUserAgent")
        .send()
        .await
        .with_context(|| {
            format!("Failed to fetch latest release of repo '{author}/{repo_name}'")
        })?;

    let status = resp.status();

    let text = resp.text().await.with_context(|| {
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
