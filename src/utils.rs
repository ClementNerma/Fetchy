use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use crate::warn;

pub fn read_dir_tree(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = vec![];

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();

        if path.is_symlink() {
            warn!("> Ignoring symbolic link '{}'", path.to_string_lossy());
            continue;
        }

        if path.is_dir() {
            let sub = read_dir_tree(&path).with_context(|| {
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

pub fn progress_bar(len: usize, progress_style: &str) -> ProgressBar {
    ProgressBar::new(u64::try_from(len).unwrap()).with_style(
        ProgressStyle::with_template(&format!(
            "{}{progress_style}{}",
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] ", " {eta_precise} {msg}"
        ))
        .unwrap()
        .progress_chars("#>-"),
    )
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
