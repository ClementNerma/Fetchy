//! This module is responsible for extracting the packages' downloaded assets
//!
//! It is a fully-blocking module, as async doesn't make a lot of sense here.
//!
//! Blocking I/O can benefit from various compiler and operating system optimizations,
//! and this module requires maximum throughput.

use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use flate2::read::GzDecoder;
use indicatif::ProgressBar;
use xz::read::XzDecoder;

use crate::{
    sources::{ArchiveFormat, AssetType, BinaryInArchive},
    utils::join_iter,
};

use self::{tar::TarReader, zip::ZipReader};

mod tar;
mod zip;

trait AssetContentIter {
    fn next_file(&mut self) -> Option<Result<(PathBuf, impl Read)>>;
}

pub fn extract_asset(
    asset_path: &Path,
    content: &AssetType,
    bins_dir: &Path,
    pb: ProgressBar,
) -> Result<()> {
    match content {
        AssetType::Binary { copy_as } => {
            let dest = bins_dir.join(copy_as);

            std::fs::copy(asset_path, &dest)
                .with_context(|| format!("Failed to copy binary '{copy_as}'"))?;

            apply_bin_perms(&dest)?;

            Ok(())
        }

        AssetType::Archive { format, files } => {
            pb.set_message("opening archive...");

            let file = File::open(asset_path).context("Failed to open downloaded archive")?;

            match format {
                ArchiveFormat::TarGz => {
                    let mut reader = TarReader::new(GzDecoder::new(file));
                    extract_archive(reader.iter()?, files, bins_dir, pb.clone())
                }

                ArchiveFormat::TarXz => {
                    let mut reader = TarReader::new(XzDecoder::new(file));
                    extract_archive(reader.iter()?, files, bins_dir, pb.clone())
                }

                ArchiveFormat::Zip => {
                    let mut reader = ZipReader::new(file)?;
                    extract_archive(reader.iter(), files, bins_dir, pb.clone())
                }
            }
        }
    }
}

fn extract_archive(
    mut reader: impl AssetContentIter,
    files: &[BinaryInArchive],
    bins_dir: &Path,
    pb: ProgressBar,
) -> Result<()> {
    pb.set_message(format!("searching 1/{}...", files.len()));

    let mut extracted = Vec::with_capacity(files.len());
    extracted.resize_with(files.len(), || None::<String>);

    let mut paths_in_archive = vec![];

    let mut extracted_count = 0;

    while let Some(entry) = reader.next_file() {
        let (path, mut entry_reader) = entry?;

        for (i, file) in files.iter().enumerate() {
            let BinaryInArchive {
                path_matcher,
                copy_as,
            } = file;

            let path_in_archive = simplify_path(&path);

            paths_in_archive.push(path_in_archive.clone());

            if !path_matcher.is_match(&path_in_archive) {
                continue;
            }

            if let Some(clashing_path_in_archive) = &extracted[i] {
                bail!(
                    "Pattern '{}' matched two different files in archive:\n\n* {}\n* {}",
                    path_matcher.to_string().bright_blue(),
                    clashing_path_in_archive.bright_yellow(),
                    path_in_archive.bright_yellow()
                );
            }

            if let Some((clashing_bin_idx, _)) = extracted.iter().enumerate().find(|(_, entry)| {
                entry
                    .as_ref()
                    .is_some_and(|other_path_in_archive| *other_path_in_archive == path_in_archive)
            }) {
                bail!("File at path '{}' in archive was matched by two different regular expressions:\n\n* {}\n* {}", 
                path_in_archive.bright_yellow(),
                    files[clashing_bin_idx].path_matcher.to_string().bright_blue(),
                    path_matcher.to_string().bright_blue(),
                );
            }

            extracted_count += 1;

            pb.set_message(format!(
                "extracting {extracted_count}/{}: '{copy_as}'...",
                files.len()
            ));

            let dest = bins_dir.join(copy_as);

            let mut out_file =
                File::create(&dest).context("Failed to create temporary file to extract binary")?;

            std::io::copy(&mut entry_reader, &mut out_file)
                .with_context(|| format!("Failed to copy binary '{copy_as}'"))?;

            apply_bin_perms(&dest)?;

            pb.set_message(if extracted_count < files.len() {
                format!("searching  {}/{}...", extracted_count + 1, files.len())
            } else {
                "checking end of archive...".to_owned()
            });

            extracted[i] = Some(path_in_archive)
        }
    }

    for (i, result) in extracted.iter().enumerate() {
        if result.is_none() {
            bail!(
                "Pattern '{}' matched none of the archive's files:\n\n{}",
                files[i].path_matcher.to_string().bright_blue(),
                join_iter(
                    paths_in_archive
                        .iter()
                        .map(|path| format!("* {}", path.bright_yellow())),
                    "\n"
                )
            );
        }
    }

    Ok(())
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

fn apply_bin_perms(path: &Path) -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).with_context(
            || {
                format!(
                    "Failed to set binary at path '{}' executable",
                    path.display()
                )
            },
        )?;
    }

    Ok(())
}
