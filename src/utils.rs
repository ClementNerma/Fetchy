use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use tokio::fs;

use crate::warn;

#[async_recursion]
pub async fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    if !to.exists() {
        fs::create_dir_all(&to).await?;
    }

    let mut dir_iter = fs::read_dir(&from).await?;

    while let Some(entry) = dir_iter.next_entry().await? {
        let from = entry.path();
        let to = to.join(entry.file_name());

        if from.is_symlink() {
            bail!(
                "Won't copy symbolic link item '{}'",
                entry.path().to_string_lossy()
            );
        } else if from.is_dir() {
            copy_dir(&from, &to).await.with_context(|| {
                format!(
                    "Failed to extract directory '{}'",
                    entry.file_name().to_string_lossy()
                )
            })?;
        } else if from.is_file() {
            fs::copy(&from, &to).await.with_context(|| {
                format!(
                    "Failed to copy file '{}'",
                    entry.file_name().to_string_lossy()
                )
            })?;
        } else {
            bail!(
                "Won't copy item '{}' that is neither a file nor a directory",
                entry.file_name().to_string_lossy()
            );
        }
    }

    Ok(())
}

#[async_recursion]
pub async fn read_dir_tree(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = vec![];

    let mut dir_iter = fs::read_dir(&dir).await?;

    while let Some(entry) = dir_iter.next_entry().await? {
        let path = entry.path();

        if path.is_symlink() {
            warn!("> Ignoring symbolic link '{}'", path.to_string_lossy());
            continue;
        }

        if path.is_dir() {
            let sub = read_dir_tree(&path).await.with_context(|| {
                format!(
                    "Failed to list content of directory '{:?}'",
                    path.file_name()
                )
            })?;

            out.push(path);
            out.extend(sub);
        } else if path.is_file() {
            out.push(path);
        } else {
            bail!(
                "Found unknown item '{:?}' that is neither a file nor a directory",
                path.file_name()
            );
        }
    }

    Ok(out)
}

#[macro_export]
macro_rules! largest_key_width {
    ($vec: expr, $key: ident) => {
        $vec.iter()
            .map(|value| ::unicode_width::UnicodeWidthStr::width(value.$key.as_str()))
            .max()
            .expect("Provided list is empty")
    };
}
