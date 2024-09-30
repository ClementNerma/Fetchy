use std::{
    fs::{self, File},
    path::Path,
};

use anyhow::{anyhow, bail, Context, Result};
use indicatif::ProgressBar;
use parsy::{LocationInString, Parser};

use crate::{
    app_data::RepositorySource,
    debug,
    installer::InstallPackageOptions,
    parser::repository,
    repository::{DownloadSource, FileExtraction, Package, Repository},
    sources::{direct::DirectSource, github::GitHubSource, AssetSource},
};

pub fn fetch_repository(repo: &RepositorySource) -> Result<Repository> {
    match repo {
        RepositorySource::File(path) => {
            if !path.is_file() {
                bail!("Provided repository file does not exist");
            }

            let repo_str =
                fs::read_to_string(path).context("Failed to read provided repository file")?;

            repository()
                .parse_str(&repo_str)
                .map(|parsed| parsed.data)
                .map_err(|err| {
                    let LocationInString { line, col } =
                        err.inner().at().start.compute_offset_in(&repo_str).unwrap();

                    anyhow!(
                        "Failed to parse repository: parsing error at line {} column {}: {}",
                        line + 1,
                        col + 1,
                        err.critical_message()
                            .map(str::to_owned)
                            .or_else(|| err.atomic_error().map(str::to_owned))
                            .unwrap_or_else(|| format!("{}", err.inner().expected()))
                    )
                })
        }
    }
}

pub fn fetch_package_asset_infos(pkg: &Package) -> Result<AssetInfos> {
    match &pkg.source {
        DownloadSource::Direct(params) => DirectSource::fetch(params),
        DownloadSource::GitHub(params) => GitHubSource::fetch(params),
    }
}

pub fn fetch_package<'a, 'b, 'c, 'd>(
    pkg: &'a Package,
    repo_name: &'d str,
    asset: AssetInfos,
    bin_dir: &'b Path,
    isolated_dir: &'c Path,
    pb: ProgressBar,
) -> Result<InstallPackageOptions<'a, 'b, 'c, 'd>> {
    let AssetInfos {
        url,
        version,
        extraction,
    } = asset;

    debug!("Downloading asset from URL: {}...", url.bright_cyan());

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

    Ok(InstallPackageOptions {
        pkg,
        dl_file_path,
        tmp_dir,
        bin_dir,
        isolated_dir,
        repo_name,
        version,
        extraction,
    })
}

pub struct AssetInfos {
    pub url: String,
    pub version: String,
    pub extraction: FileExtraction,
}
