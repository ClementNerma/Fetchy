use std::{io::Read, path::PathBuf};

use anyhow::{Context, Result};
use tar::{Archive, Entries};

use super::AssetContentIter;

pub struct TarReader<R: Read> {
    archive: Archive<R>,
}

impl<R: Read + Unpin> TarReader<R> {
    pub fn new(read: R) -> Self {
        Self {
            archive: Archive::new(read),
        }
    }

    pub fn iter(&mut self) -> Result<TarReaderIter<R>> {
        let entries = self
            .archive
            .entries()
            .context("Failed to get entries from tarball")?;

        Ok(TarReaderIter { entries })
    }
}

pub struct TarReaderIter<'a, R: Read> {
    entries: Entries<'a, R>,
}

impl<'a, R: Read> AssetContentIter for TarReaderIter<'a, R> {
    fn next_file(&mut self) -> Option<Result<(PathBuf, impl Read)>> {
        self.entries.next().map(|result| {
            let entry = result.context("Failed to read entry from tarball archive")?;

            let path = entry
                .path()
                .context("Failed to get entry pat from tarball archive")?;

            Ok((path.into_owned(), entry))
        })
    }
}
