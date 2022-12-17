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
use tempfile::TempDir;
use tokio::{
    fs::{self, File},
    io,
};
use tokio_tar::Archive;

use crate::{
    app_data::InstalledPackage,
    repository::{ArchiveFormat, FileFormat, Package},
};

pub async fn install_package(
    pkg: &Package,
    dl_file_path: PathBuf,
    tmp_dir: TempDir,
    bin_dir: &Path,
    repo_name: &str,
    version: String,
    on_message: &Box<dyn Fn(&str)>,
) -> Result<InstalledPackage> {
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
                        if !file.relative_path.regex.is_match(path_str) {
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

struct FileToCopy {
    // original_path: Option<String>,
    current_path: PathBuf,
    rename_to: String,
}
