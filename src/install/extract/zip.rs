use std::{
    io::{Read, Seek},
    ops::Range,
    path::PathBuf,
};

use anyhow::{Context, Result};
use zip::ZipArchive;

use super::AssetContentIter;

pub struct ZipReader<R: Read + Seek> {
    archive: ZipArchive<R>,
}

impl<R: Read + Seek> ZipReader<R> {
    pub fn new(read: R) -> Result<Self> {
        Ok(Self {
            archive: ZipArchive::new(read).context("Failed to open ZIP archive")?,
        })
    }

    pub fn iter(&mut self) -> ZipReaderIter<R> {
        ZipReaderIter {
            files: (0..self.archive.len()),
            archive: &mut self.archive,
        }
    }
}

pub struct ZipReaderIter<'a, R: Read + Seek> {
    archive: &'a mut ZipArchive<R>,
    files: Range<usize>,
}

impl<R: Read + Seek> AssetContentIter for ZipReaderIter<'_, R> {
    fn next_file(&mut self) -> Option<Result<(PathBuf, impl Read)>> {
        self.files.next().map(move |idx| {
            let entry = self
                .archive
                .by_index(idx)
                .context("Failed to get entry from ZIP archive")?;

            Ok((PathBuf::from(entry.name()), entry))
        })
    }
}
