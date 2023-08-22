use std::{
    fs::{self, File},
    path::Path,
};

use anyhow::{bail, Context, Result};
use indicatif::ProgressBar;
use once_cell::sync::Lazy;
use pomsky_macro::pomsky;
use regex::Regex;

use crate::{
    app_data::{InstalledPackage, RepositorySource},
    installer::{install_package, InstallPackageOptions},
    repository::{DownloadSource, Package, Repository, VersionExtraction, VersionExtractionSource},
    sources::*,
};

pub struct FetchedPackageAssetInfos {
    pub url: String,
    pub version: String,
}

pub fn fetch_package_asset_infos(pkg: &Package) -> Result<FetchedPackageAssetInfos> {
    let Asset {
        url,
        filename,
        release_title,
        tag_name,
    } = match &pkg.download.source {
        DownloadSource::Direct { url } => Asset {
            url: url.get_for_current_platform()?.clone(),
            filename: None,
            release_title: None,
            tag_name: None,
        },
        DownloadSource::GitHub {
            author,
            repo_name,
            asset_pattern,
        } => github::fetch_latest_release_asset(
            author,
            repo_name,
            asset_pattern.get_for_current_platform()?,
        )?,
    };

    let VersionExtraction {
        source,
        regex,
        skip_validation,
    } = &pkg.download.version_extraction;

    let version = match source {
        VersionExtractionSource::Url => &url,
        VersionExtractionSource::ReleaseTitle => release_title
            .as_ref()
            .context("Cannot match on non-existent release title")?,
        VersionExtractionSource::DownloadedFileName => filename
            .as_ref()
            .context("Cannot match on non-existent filename")?,
        VersionExtractionSource::TagName => tag_name
            .as_ref()
            .context("Cannot match on non-existent tag name")?,
    };

    let version = match regex {
        None => version,
        Some(regex) => regex
            .regex
            .captures(version)
            .with_context(|| {
                format!(
                    "Version extraction regex ({}) did not match on string: {version}",
                    regex.source
                )
            })?
            .get(1)
            .with_context(|| format!("Missing version capture group on regex: {}", regex.source))?
            .as_str(),
    };

    let version = match skip_validation {
        Some(true) => version,
        Some(false) | None => VERSION_VALIDATOR
            .captures(version)
            .with_context(|| format!("Version validation failed on: {version}"))?
            .get(1)
            .unwrap()
            .as_str(),
    };

    Ok(FetchedPackageAssetInfos {
        version: version.to_string(),
        url,
    })
}

pub fn fetch_package(
    pkg: &Package,
    repo_name: &str,
    FetchedPackageAssetInfos { url, version }: FetchedPackageAssetInfos,
    bin_dir: &Path,
    config_dir: &Path,
    pb: ProgressBar,
) -> Result<InstalledPackage> {
    println!("Downloading asset from URL: {url}...");

    let tmp_dir = tempfile::tempdir().context("Failed to create a temporary file")?;

    let dl_file_path = tmp_dir.path().join("asset.tmp");
    let dl_file = File::create(&dl_file_path).context("Failed to create a temporary file")?;

    let mut resp = reqwest::blocking::get(&url)
        .with_context(|| format!("Failed to fetch asset at URL: {url}"))?;

    if let Some(len) = resp.content_length() {
        pb.set_length(len);
    }

    pb.set_position(0);

    resp.copy_to(&mut pb.wrap_write(dl_file))
        .context("Failed to download file")?;

    pb.finish();

    install_package(InstallPackageOptions {
        pkg,
        dl_file_path,
        tmp_dir,
        bin_dir,
        config_dir,
        repo_name,
        version,
    })
}

pub fn fetch_repository(repo: &RepositorySource) -> Result<Repository> {
    match repo {
        RepositorySource::File(path) => {
            if !path.is_file() {
                bail!("Provided repository file does not exist");
            }

            let repo_str =
                fs::read_to_string(path).context("Failed to read provided repository file")?;

            ron::from_str::<Repository>(&repo_str)
                .context("Failed to parse provided repository file")
        }
    }
}

pub struct Asset {
    pub url: String,
    pub filename: Option<String>,
    pub release_title: Option<String>,
    pub tag_name: Option<String>,
}

static VERSION_VALIDATOR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(pomsky!(
        Start 'v'? [s]* :([L d '.' '-']+) End
    ))
    .unwrap()
});
