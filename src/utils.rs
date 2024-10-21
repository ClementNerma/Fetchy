use std::{borrow::Cow, fmt::Display, sync::LazyLock, time::Duration};

use anyhow::{Context, Result};
use dialoguer::Select;
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt::Write;
use tokio::task::JoinSet;

pub static PROGRESS_BAR_TICK_CHARS: &str = "##-";

pub static ITEMS_PROGRESS_BAR_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template(
        "{spinner:.green} {prefix}[{bar:40.cyan/blue}] {pos:>2}/{len:>2} ({eta:>3}) {msg}",
    )
    .unwrap()
    .progress_chars(PROGRESS_BAR_TICK_CHARS)
});

pub static BYTES_PROGRESS_BAR_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template(
        "{spinner:.green} {prefix}[{bar:40.cyan/blue}] {bytes:>10}/{total_bytes:>10} ({eta:>3}) {msg}",
    )
    .unwrap().progress_chars(PROGRESS_BAR_TICK_CHARS)
});

pub static SPINNER_PROGRESS_BAR_STYLE: LazyLock<ProgressStyle> =
    LazyLock::new(|| ProgressStyle::with_template("{spinner:.green} {prefix}{msg}").unwrap());

pub fn progress_bar(
    len: usize,
    style: ProgressStyle,
    msg: impl Into<Cow<'static, str>>,
) -> ProgressBar {
    let pb = ProgressBar::new(len.try_into().unwrap())
        .with_style(style)
        .with_message(msg);

    pb.tick();
    pb.enable_steady_tick(Duration::from_millis(125));

    pb
}

pub async fn confirm() -> Result<bool> {
    tokio::task::spawn_blocking(|| {
        Select::new()
            .items(&["Continue", "Abort"])
            .interact()
            .map(|choice| choice == 0)
            .context("Failed to get user choice")
    })
    .await
    .context("Failed to wait on Tokio task")
    .flatten()
    .inspect(|_| println!())
}

pub async fn join_fallible_set<T: 'static>(mut tasks: JoinSet<Result<T>>) -> Result<Vec<T>> {
    let mut results = Vec::with_capacity(tasks.len());

    while let Some(result) = tasks.join_next().await {
        match result.context("Failed to join Tokio task").flatten() {
            Ok(asset_infos) => results.push(asset_infos),

            Err(err) => {
                // If any of the tasks fails, we abort all the others
                tasks.abort_all();

                // Then we wait for all others to complete
                // Note that we can't use `.join_all()` as it would panic because
                // of the task being aborted
                while tasks.join_next().await.is_some() {}

                return Err(err);
            }
        }
    }

    Ok(results)
}

pub async fn join_fallible_ordered_set<T: 'static>(
    tasks: JoinSet<Result<(usize, T)>>,
) -> Result<Vec<T>> {
    let mut results = join_fallible_set(tasks).await?;

    results.sort_by_key(|(pos, _)| *pos);

    Ok(results.into_iter().map(|(_, value)| value).collect())
}

/// Adapted from the `itertools` crate: https://docs.rs/itertools/latest/src/itertools/lib.rs.html
pub fn join_iter<D: Display>(mut iter: impl Iterator<Item = D>, sep: &str) -> String {
    match iter.next() {
        None => String::new(),

        Some(first_elt) => {
            // estimate lower bound of capacity needed
            let (lower, _) = iter.size_hint();

            let mut result = String::with_capacity(sep.len() * lower);

            write!(&mut result, "{first_elt}").unwrap();

            iter.for_each(|elt| {
                result.push_str(sep);
                write!(&mut result, "{elt}").unwrap();
            });

            result
        }
    }
}
