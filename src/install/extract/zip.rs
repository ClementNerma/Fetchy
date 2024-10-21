use std::{ops::Range, path::PathBuf};

use anyhow::{Context, Result};
use async_zip::base::read::seek::ZipFileReader;
use tokio::io::{AsyncRead, AsyncSeek, BufReader};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};

use super::ArchiveReader;

pub struct ZipReader<R: AsyncRead + AsyncSeek + Unpin> {
    archive: ZipFileReader<Compat<BufReader<R>>>,
    file_range: Range<usize>,
}

impl<R: AsyncRead + AsyncSeek + Unpin> ZipReader<R> {
    pub async fn new(read: R) -> Result<Self> {
        let archive = ZipFileReader::with_tokio(BufReader::new(read))
            .await
            .context("Failed to open ZIP archive")?;

        Ok(Self {
            file_range: (0..archive.file().entries().len()),
            archive,
        })
    }

    async fn read_entry(&mut self, idx: usize) -> Result<(PathBuf, impl AsyncRead + '_)> {
        let entry = self
            .archive
            .file()
            .entries()
            .get(idx)
            .context("Failed to get entry from ZIP archive")?;

        let path = entry
            .filename()
            .as_str()
            .context("Failed to decode entry's filename in ZIP archive")?;

        let path = PathBuf::from(path);

        let entry_reader = self
            .archive
            .reader_without_entry(idx)
            .await
            .context("Failed to get reader for entry in ZIP archive")?;

        Ok((path, entry_reader.compat()))
    }
}

impl<R: AsyncRead + AsyncSeek + Unpin> ArchiveReader for ZipReader<R> {
    async fn next(&mut self) -> Option<Result<(PathBuf, impl AsyncRead)>> {
        let idx = self.file_range.next()?;
        Some(self.read_entry(idx).await)
    }
}
