use std::{
    borrow::Cow,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use async_compression::tokio::write::GzipDecoder;
use async_zip::read::fs::ZipFileReader;
use futures_util::StreamExt;
use tokio::{
    fs::{self, File},
    io::{self, AsyncWriteExt},
};
use tokio_tar::Archive;

use crate::{
    app_data::{InstalledPackage, RepositorySource},
    repository::{
        ArchiveFormat, DownloadSource, FileFormat, Package, Repository, VersionExtractionSource,
    },
    sources::*,
};

pub struct FetchProgressTracking {
    pub on_message: Box<dyn Fn(&str)>,
    pub on_dl_progress: Box<dyn Fn(usize, Option<u64>)>,
    pub on_finish: Box<dyn Fn()>,
}

pub struct FetchedPackageAssetInfos {
    pub url: String,
    pub version: String,
}

pub async fn fetch_package_asset_infos(pkg: &Package) -> Result<FetchedPackageAssetInfos> {
    let Asset {
        url,
        filename,
        release_title,
        tag_name,
    } = match &pkg.download.source {
        DownloadSource::Direct { url } => Asset {
            url: url.clone(),
            filename: None,
            release_title: None,
            tag_name: None,
        },
        DownloadSource::GitHub {
            author,
            repo_name,
            asset_pattern,
        } => github::fetch_latest_release_asset(author, repo_name, &asset_pattern).await?,
    };

    let version_extraction_string = match pkg.download.version_extraction.source {
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

    let version = pkg
        .download
        .version_extraction
        .regex
        .regex
        .captures(&version_extraction_string)
        .with_context(|| {
            format!("Version extraction regex ({}) did not match on string: {version_extraction_string}", pkg.download.version_extraction.regex.source)
        })?
        .get(1)
        .with_context(|| {
            format!(
                "Missing version capture group on regex: {}",
                pkg.download.version_extraction.regex.source
            )
        })?
        .as_str()
        .to_owned();

    Ok(FetchedPackageAssetInfos { url, version })
}

pub async fn fetch_package(
    pkg: &Package,
    repo_name: &str,
    FetchedPackageAssetInfos { url, version }: FetchedPackageAssetInfos,
    bin_dir: &Path,
    FetchProgressTracking {
        on_message,
        on_dl_progress,
        on_finish,
    }: &FetchProgressTracking,
) -> Result<InstalledPackage> {
    on_message(&format!("Downloading asset from URL: {url}..."));

    let tmp_dir = tempfile::tempdir().context("Failed to create a temporary file")?;

    let dl_file_path = tmp_dir.path().join("asset.tmp");
    let mut dl_file = File::create(&dl_file_path)
        .await
        .context("Failed to create a temporary file")?;

    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("Failed to fetch asset at URL: {url}"))?;

    let total_len = resp.content_length();

    on_dl_progress(0, total_len);

    let mut stream = resp.bytes_stream();
    let mut wrote = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk =
            chunk_result.with_context(|| format!("Failed to download chunk from URL: {url}"))?;

        wrote += chunk.len();

        dl_file
            .write_all(&chunk)
            .await
            .with_context(|| format!("Failed to write chunk downloaded from URL: {url}"))?;

        on_dl_progress(wrote, total_len);
    }

    on_finish();

    dl_file
        .flush()
        .await
        .context("Failed to flush the downloadd file to disk after download ended")?;

    let files_to_copy = match &pkg.download.file_format {
        FileFormat::Archive { format, files } => match format {
            ArchiveFormat::TarGz => {
                on_message("Extracting GZip archive...");

                let tar_file_path = tmp_dir.path().join("tarball.tmp");

                let mut tar_file = File::create(&tar_file_path)
                    .await
                    .context("Failed to create a temporary file for tarball extraction")?;

                let mut decoder = GzipDecoder::new(&mut tar_file);

                let mut dl_file = File::open(&dl_file_path)
                    .await
                    .context("Failed to open downloaded file")?;

                io::copy(&mut dl_file, &mut decoder)
                    .await
                    .context("Failed to extract GZip archive")?;

                on_message("Analyzing tarball archive...");

                let tar_file = File::open(&tar_file_path)
                    .await
                    .context("Failed to open the tarball archive")?;

                let mut tarball = Archive::new(tar_file);

                let mut stream = tarball
                    .entries()
                    .context("Failed to list entries from tarball")?;

                let mut out = Vec::with_capacity(files.len());
                let mut treated = vec![None; files.len()];

                while let Some(entry) = stream.next().await {
                    let mut entry = entry.context("Failed to get entry from tarball archive")?;

                    let path = entry
                        .path()
                        .map(Cow::into_owned)
                        .context("Failed to get entry's path from tarball")?;

                    let Some(path_str) = path.to_str() else { continue };

                    for (i, file) in files.iter().enumerate() {
                        if !file.relative_path.regex.is_match(&path_str) {
                            continue;
                        }

                        if let Some(prev) = &treated[i] {
                            bail!("Multiple entries matched the file regex ({}) in the tarball archive:\n* {}\n* {}",
                                file.relative_path.source,
                                prev,
                                path_str
                            );
                        }

                        let extraction_path = tmp_dir.path().join(format!("{i}.tmp"));

                        entry
                            .unpack(&extraction_path)
                            .await
                            .context("Failed to extract file from tarball archive")?;

                        out.push(FileToCopy {
                            // original_path: Some(path_str.to_owned()),
                            current_path: extraction_path,
                            rename_to: file.rename_to.clone(),
                        });

                        treated[i] = Some(path_str.to_owned());
                    }
                }

                if let Some(pos) = treated.iter().position(Option::is_none) {
                    bail!(
                        "No entry matched the file regex ({}) in the tarball archive",
                        files[pos].relative_path.source
                    );
                }

                out
            }
            ArchiveFormat::Zip => {
                on_message("Analyzing ZIP archive...");

                let zip = ZipFileReader::new(&dl_file_path)
                    .await
                    .context("Failed to open ZIP archive")?;

                let entries = zip.entries();

                let mut out = Vec::with_capacity(files.len());

                for (i, file) in files.iter().enumerate() {
                    let results = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| file.relative_path.regex.is_match(entry.filename()))
                        .collect::<Vec<_>>();

                    if results.is_empty() {
                        bail!(
                            "No entry matched the file regex ({}) in the ZIP archive",
                            file.relative_path.source
                        );
                    } else if results.len() > 1 {
                        bail!(
                            "Multiple entries matched the file regex ({}) in the ZIP archive:\n{}",
                            file.relative_path.source,
                            results
                                .into_iter()
                                .map(|(_, entry)| format!("* {}", entry.filename()))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    }

                    let reader = zip
                        .entry_reader(results[0].0)
                        .await
                        .context("Failed to read entry from ZIP archive")?;

                    let extraction_path = tmp_dir.path().join(format!("{i}.tmp"));

                    let mut write = File::create(&extraction_path)
                        .await
                        .context("Failed to open writable file for extraction")?;

                    reader
                        .copy_to_end_crc(&mut write, 64 * 1024)
                        .await
                        .context("Failed to extract file from ZIP archive")?;

                    out.push(FileToCopy {
                        // original_path: Some(entry.filename().to_owned()),
                        current_path: extraction_path,
                        rename_to: file.rename_to.clone(),
                    });
                }

                out
            }
        },

        FileFormat::Binary {
            filename: out_filename,
        } => {
            vec![FileToCopy {
                // original_path: filename.clone(),
                current_path: dl_file_path,
                rename_to: out_filename.clone(),
            }]
        }
    };

    for file in &files_to_copy {
        on_message(&format!("Copying binary: {}...", file.rename_to));

        let bin_path = bin_dir.join(&file.rename_to);

        fs::copy(&file.current_path, &bin_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to copy binary '{}' to the binaries directory",
                    file.rename_to
                )
            })?;

        // TODO: fix this as this doesn't work :(
        fs::set_permissions(&file.current_path, std::fs::Permissions::from_mode(0o744))
            .await
            .context("Failed to write file's new metadata (updated permissions)")?;
    }

    Ok(InstalledPackage {
        pkg_name: pkg.name.clone(),
        repo_name: repo_name.to_owned(),
        version,
        at: SystemTime::now(),
        binaries: files_to_copy
            .iter()
            .map(|file| file.rename_to.clone())
            .collect(),
    })
}

pub async fn fetch_repository(repo: &RepositorySource) -> Result<Repository> {
    match repo {
        RepositorySource::File(path) => {
            if !path.is_file() {
                bail!("Provided repository file does not exist");
            }

            let repo_str = fs::read_to_string(&path)
                .await
                .context("Failed to read provided repository file")?;

            serde_json::from_str::<Repository>(&repo_str)
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

struct FileToCopy {
    // original_path: Option<String>,
    current_path: PathBuf,
    rename_to: String,
}
