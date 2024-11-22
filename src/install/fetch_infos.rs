use anyhow::{Context, Result};
use colored::Colorize;
use tokio::task::JoinSet;

use crate::{
    repos::ast::{DownloadSource, PackageManifest},
    resolver::ResolvedPkg,
    sources::{AssetInfos, AssetSource},
    utils::{join_fallible_ordered_set, progress_bar, ITEMS_PROGRESS_BAR_STYLE},
};

pub async fn fetch_pkgs_infos(
    pkgs: impl ExactSizeIterator<Item = &PackageManifest>,
) -> Result<Vec<(PackageManifest, AssetInfos)>> {
    let mut tasks = JoinSet::new();

    let pb = progress_bar(
        pkgs.len(),
        ITEMS_PROGRESS_BAR_STYLE.clone(),
        "Fetching package informations...",
    );

    for (i, pkg) in pkgs.enumerate() {
        let pkg = (*pkg).clone();
        let pb = pb.clone();

        tasks.spawn(async move {
            let asset_infos = match &pkg.source {
                DownloadSource::Direct(params) => params.fetch_infos().await,
                DownloadSource::GitHub(params) => params.fetch_infos().await,
            };

            asset_infos
                .with_context(|| {
                    format!(
                        "Failed to fetch informations about package {}",
                        pkg.name.bright_yellow()
                    )
                })
                .inspect(|_| pb.inc(1))
                .map(|infos| (i, (pkg, infos)))
        });
    }

    join_fallible_ordered_set(tasks)
        .await
        .inspect(|_| pb.finish_and_clear())
        .inspect_err(|_| pb.abandon())
}

pub async fn fetch_resolved_pkg_infos<'a, 'b>(
    pkgs: &[ResolvedPkg<'a, 'b>],
) -> Result<Vec<(ResolvedPkg<'a, 'b>, AssetInfos)>> {
    let fetched = fetch_pkgs_infos(pkgs.iter().map(|pkg| pkg.manifest)).await?;

    Ok(fetched
        .into_iter()
        .enumerate()
        .map(|(i, (_, asset_info))| (pkgs[i], asset_info))
        .collect())
}
