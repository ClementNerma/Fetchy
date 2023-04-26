use std::path::Path;

use anyhow::{Context, Result};
use async_compression::tokio::write::{GzipDecoder, XzDecoder};
use tar::Archive;
use tokio::{
    fs::{self, File},
    io,
};
use zip::ZipArchive;

use crate::repository::ArchiveFormat;

pub async fn extract_archive(
    archive_path: &Path,
    format: &ArchiveFormat,
    extract_to: &Path,
) -> Result<()> {
    match format {
        ArchiveFormat::TarGz | ArchiveFormat::TarXz => {
            let tar_file_path = extract_to.join("tarball.tmp");

            let mut tar_file = File::create(&tar_file_path)
                .await
                .context("Failed to create a temporary file for tarball extraction")?;

            let mut dl_file = File::open(&archive_path)
                .await
                .context("Failed to open downloaded file")?;

            if *format == ArchiveFormat::TarGz {
                io::copy(&mut dl_file, &mut GzipDecoder::new(&mut tar_file))
                    .await
                    .context("Failed to decompress GZip archive")?
            } else {
                io::copy(&mut dl_file, &mut XzDecoder::new(&mut tar_file))
                    .await
                    .context("Failed to decompress Xz archive")?
            };

            let archive_path = tar_file_path.clone();
            let tmp_dir = extract_to.to_path_buf();

            tokio::spawn(async move { extract_tar_sync(&archive_path, &tmp_dir) })
                .await
                .context("Failed to run TAR decompression task")?
                .context("Failed to extract TAR archive")?;

            fs::remove_file(tar_file_path)
                .await
                .context("Failed to remove the temporary tarball file")?;
        }

        ArchiveFormat::Zip => {
            let archive_path = archive_path.to_path_buf();
            let tmp_dir = extract_to.to_path_buf();

            tokio::spawn(async move { extract_zip_sync(&archive_path, &tmp_dir) })
                .await
                .context("Failed to run ZIP decompression task")?
                .context("Failed to extract ZIP archive")?;
        }
    }

    Ok(())
}

fn extract_zip_sync(zip_path: &Path, extract_to: &Path) -> Result<()> {
    use std::fs::File;

    let file = File::open(zip_path).context("Failed to open ZIP file")?;

    let mut zip = ZipArchive::new(file).unwrap();

    zip.extract(extract_to)
        .context("Failed to extract ZIP archive")?;

    Ok(())
}

fn extract_tar_sync(tar_path: &Path, extract_to: &Path) -> Result<()> {
    use std::fs::File;

    let tar_file = File::open(tar_path).context("Failed to open the tarball archive")?;

    let mut tarball = Archive::new(tar_file);

    tarball
        .unpack(extract_to)
        .context("Failed to extract tarball archive")?;

    Ok(())
}
