use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;
use xz2::read::XzDecoder;
use zip::ZipArchive;

use crate::repository::ArchiveFormat;

pub fn extract_archive(
    archive_path: PathBuf,
    format: &ArchiveFormat,
    extract_to: PathBuf,
) -> Result<()> {
    match format {
        ArchiveFormat::TarGz | ArchiveFormat::TarXz => {
            let tar_file_path = extract_to.join("tarball.tmp");

            let mut tar_file = File::create(&tar_file_path)
                .context("Failed to create a temporary file for tarball extraction")?;

            let dl_file = File::open(&archive_path).context("Failed to open downloaded file")?;

            match *format {
                ArchiveFormat::TarGz => {
                    io::copy(&mut GzDecoder::new(dl_file), &mut tar_file)
                        .context("Failed to decompress GZip archive")?;
                }
                ArchiveFormat::TarXz => {
                    io::copy(&mut XzDecoder::new(dl_file), &mut tar_file)
                        .context("Failed to decompress Xz archive")?;
                }
                ArchiveFormat::Zip => unreachable!(),
            }

            extract_tar_sync(&tar_file_path, &extract_to)
                .context("Failed to extract TAR archive")?;

            fs::remove_file(tar_file_path)
                .context("Failed to remove the temporary tarball file")?;
        }

        ArchiveFormat::Zip => {
            extract_zip_sync(&archive_path, &extract_to)
                .context("Failed to extract ZIP archive")?;
        }
    }

    Ok(())
}

fn extract_zip_sync(zip_path: &Path, extract_to: &Path) -> Result<()> {
    let file = File::open(zip_path).context("Failed to open ZIP file")?;

    let mut zip = ZipArchive::new(file).unwrap();

    zip.extract(extract_to)
        .context("Failed to extract ZIP archive")?;

    Ok(())
}

fn extract_tar_sync(tar_path: &Path, extract_to: &Path) -> Result<()> {
    let tar_file = File::open(tar_path).context("Failed to open the tarball archive")?;

    let mut tarball = Archive::new(tar_file);

    tarball
        .unpack(extract_to)
        .context("Failed to extract tarball archive")?;

    Ok(())
}
