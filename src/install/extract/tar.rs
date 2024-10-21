use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, BufReader};
use tokio_stream::StreamExt;
use tokio_tar::{Archive, Entries};

use super::ArchiveReader;

pub struct TarReader<R: AsyncRead + Unpin> {
    entries: Entries<BufReader<R>>,
}

impl<R: AsyncRead + Unpin> TarReader<R> {
    pub fn new(read: R) -> Self {
        let mut archive = Archive::new(BufReader::new(read));

        Self {
            entries: archive.entries().unwrap(),
        }
    }
}

impl<R: AsyncRead + Unpin> ArchiveReader for TarReader<R> {
    async fn next(&mut self) -> Option<Result<(PathBuf, impl AsyncRead)>> {
        self.entries.next().await.map(|result| {
            let entry = result.context("Failed to read entry from tarball archive")?;

            let path = entry
                .path()
                .context("Failed to get entry pat from tarball archive")?;

            Ok((path.into_owned(), entry))
        })
    }
}
