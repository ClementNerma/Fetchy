use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar};
use reqwest::Client;
use tempfile::TempDir;
use tokio::{fs::File, io::AsyncWriteExt, task::JoinSet};

use crate::{
    repos::ast::PackageManifest,
    sources::AssetInfos,
    utils::{join_fallible, BYTES_PROGRESS_BAR_STYLE, SPINNER_PROGRESS_BAR_STYLE},
};

use super::extract::extract_asset;

pub async fn download_pkgs_and<F: Future<Output = Result<()>> + Send>(
    pkgs: Vec<(PackageManifest, AssetInfos)>,
    bins_dir: &Path,
    finalize: impl Fn(PackageManifest, AssetInfos) -> F + Clone + Send + Sync + 'static,
) -> Result<
    // The temporary directory is returned as its content is deleted when its `Drop`ped
    TempDir,
> {
    let dl_dir = TempDir::new().context("Failed to create a temporary downloads directory")?;

    let multi = MultiProgress::new();
    let mut tasks = JoinSet::new();

    let largest_pkg_name = pkgs
        .iter()
        .map(|(manifest, _)| manifest.name.len())
        .max()
        .unwrap();

    for (pkg, asset_infos) in pkgs {
        let pb = multi.add(
            ProgressBar::new_spinner()
                .with_style(SPINNER_PROGRESS_BAR_STYLE.clone())
                .with_prefix(format!("{:largest_pkg_name$} ", pkg.name))
                .with_message(asset_infos.version.clone()),
        );

        pb.enable_steady_tick(Duration::from_millis(125));

        let dl_dir = dl_dir.path().to_owned();

        let bins_dir = bins_dir.to_owned();
        let finalize = finalize.clone();

        tasks.spawn(async move {
            let asset_path = download_asset(&pkg, &asset_infos, &dl_dir, pb.clone())
                .await
                .with_context(|| {
                    format!(
                        "Failed to download asset for package {}...",
                        pkg.name.bright_yellow()
                    )
                })?;

            let pb_bis = pb.clone();
            let asset_typ = asset_infos.typ.clone();
            let pkg_name = pkg.name.clone();

            tokio::task::spawn_blocking(move || {
                extract_asset(&asset_path, &asset_typ, &bins_dir, pb_bis).with_context(|| {
                    format!(
                        "Failed to extract downloaded asset for package {}",
                        pkg_name.bright_yellow()
                    )
                })
            })
            .await
            .context("Failed to wait on Tokio task")??;

            pb.set_message("finalizing...");

            let pkg_name = pkg.name.clone();

            finalize(pkg, asset_infos)
                .await
                .with_context(|| format!("Failed on package {}", pkg_name.bright_yellow()))?;

            Ok(())
        });
    }

    join_fallible(tasks).await?;

    // Ignore errors from failing to clear multibar
    let _ = multi.clear();

    Ok(dl_dir)
}

async fn download_asset(
    pkg: &PackageManifest,
    asset_infos: &AssetInfos,
    dl_dir: &Path,
    pb: ProgressBar,
) -> Result<PathBuf> {
    let dl_file_path = dl_dir.join(format!("{}.tmp", pkg.name));

    let mut dl_file = File::create(&dl_file_path)
        .await
        .context("Failed to create temporary download file")?;

    let mut res = Client::new()
        .get(&asset_infos.url)
        .headers(asset_infos.headers.clone())
        .send()
        .await
        .context("Failed to perform GET request on asset's URL")?;

    if let Some(len) = res.content_length() {
        pb.set_length(len);
    }

    pb.set_style(BYTES_PROGRESS_BAR_STYLE.clone());

    while let Some(chunk) = res
        .chunk()
        .await
        .context("Failed to read chunk from response")?
    {
        dl_file
            .write(&chunk)
            .await
            .context("Failed to write chunk to disk")?;

        pb.inc(chunk.len().try_into().unwrap());
    }

    dl_file.flush().await?;

    Ok(dl_file_path)
}
