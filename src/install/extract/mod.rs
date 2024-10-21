use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use async_compression::tokio::bufread::{GzipDecoder, XzDecoder};
use colored::Colorize;
use indicatif::ProgressBar;
use tokio::{
    fs::{self, File},
    io::{AsyncRead, BufReader},
};

use crate::{
    sources::{ArchiveFormat, AssetType, BinaryInArchive},
    utils::join_iter,
};

use self::{tar::TarReader, zip::ZipReader};

mod tar;
mod zip;

pub trait ArchiveReader {
    async fn next(&mut self) -> Option<Result<(PathBuf, impl AsyncRead + Unpin)>>;
}

pub async fn extract_asset(
    asset_path: &Path,
    content: &AssetType,
    pb: ProgressBar,
) -> Result<ExtractionResult> {
    match content {
        AssetType::Binary { copy_as } => Ok(ExtractionResult {
            binaries: vec![ExtractedBinary {
                path: asset_path.to_owned(),
                name: copy_as.clone(),
            }],
        }),

        AssetType::Archive { format, files } => {
            pb.set_message("opening archive...");

            assert!(asset_path.extension().is_some());

            let extraction_dir = asset_path.with_extension("");

            fs::create_dir(&extraction_dir)
                .await
                .context("Failed to create a temporary extraction directory")?;

            let file = File::open(&asset_path)
                .await
                .context("Failed to open downloaded archive")?;

            match format {
                ArchiveFormat::TarGz => {
                    let reader = TarReader::new(GzipDecoder::new(BufReader::new(file)));
                    extract_archive(reader, files, &extraction_dir, pb.clone()).await
                }

                ArchiveFormat::TarXz => {
                    let reader = TarReader::new(XzDecoder::new(BufReader::new(file)));
                    extract_archive(reader, files, &extraction_dir, pb.clone()).await
                }

                ArchiveFormat::Zip => {
                    let reader = ZipReader::new(BufReader::new(file)).await?;
                    extract_archive(reader, files, &extraction_dir, pb.clone()).await
                }
            }
        }
    }
}

pub struct ExtractionResult {
    pub binaries: Vec<ExtractedBinary>,
}

pub struct ExtractedBinary {
    pub path: PathBuf,
    pub name: String,
}

async fn extract_archive(
    mut reader: impl ArchiveReader,
    files: &[BinaryInArchive],
    tmp_dir: &Path,
    pb: ProgressBar,
) -> Result<ExtractionResult> {
    pb.set_message(format!("searching 1/{}...", files.len()));

    let mut extracted = Vec::with_capacity(files.len());
    extracted.resize_with(files.len(), || None::<(String, ExtractedBinary)>);

    let mut paths_in_archive = vec![];

    let mut extracted_count = 0;

    while let Some(entry) = reader.next().await {
        let (path, mut entry_reader) = entry?;

        for (i, file) in files.iter().enumerate() {
            let BinaryInArchive {
                path_matcher,
                rename_as,
            } = file;

            let path_in_archive = simplify_path(&path);

            paths_in_archive.push(path_in_archive.clone());

            if !path_matcher.is_match(&path_in_archive) {
                continue;
            }

            if let Some((clashing_path_in_archive, _)) = &extracted[i] {
                bail!(
                    "Pattern '{}' matched two different files in archive:\n\n* {}\n* {}",
                    path_matcher.to_string().bright_blue(),
                    clashing_path_in_archive.bright_yellow(),
                    path_in_archive.bright_yellow()
                );
            }

            if let Some((clashing_bin_idx, _)) = extracted.iter().enumerate().find(|(_, entry)| {
                entry.as_ref().is_some_and(|(other_path_in_archive, _)| {
                    *other_path_in_archive == path_in_archive
                })
            }) {
                bail!("File at path '{}' in archive was matched by two different regular expressions:\n\n* {}\n* {}", 
                path_in_archive.bright_yellow(),
                    files[clashing_bin_idx].path_matcher.to_string().bright_blue(),
                    path_matcher.to_string().bright_blue(),
                );
            }

            let name = rename_as
                .as_deref()
                .unwrap_or_else(|| path_in_archive.split('/').last().unwrap())
                .to_owned();

            extracted_count += 1;

            pb.set_message(format!(
                "extracting {extracted_count}/{}: '{name}'...",
                files.len()
            ));

            let extraction_path = tmp_dir.join(format!("{i}-{name}"));

            let mut out_file = File::create_new(&extraction_path)
                .await
                .context("Failed to create temporary file to extract binary")?;

            tokio::io::copy(&mut entry_reader, &mut out_file).await?;

            pb.set_message(if extracted_count < files.len() {
                format!("searching  {}/{}...", extracted_count + 1, files.len())
            } else {
                "checking end of archive...".to_owned()
            });

            extracted[i] = Some((
                path_in_archive,
                ExtractedBinary {
                    name,
                    path: extraction_path,
                },
            ));
        }
    }

    let binaries = extracted
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            item.map(|(_, bin)| bin).with_context(|| {
                format!(
                    "Pattern '{}' matched none of the archive's files:\n\n{}",
                    files[i].path_matcher.to_string().bright_blue(),
                    join_iter(
                        paths_in_archive
                            .iter()
                            .map(|path| format!("* {}", path.bright_yellow())),
                        "\n"
                    )
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ExtractionResult { binaries })
}

fn simplify_path(path: &Path) -> String {
    let mut out = vec![];

    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(str) => {
                if str.is_empty() {
                    continue;
                }

                out.push(str.to_string_lossy())
            }
        }
    }

    out.join("/")
}
