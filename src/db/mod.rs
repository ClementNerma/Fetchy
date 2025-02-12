use std::{
    ops::Deref,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use tokio::fs;

use self::data::AppData;

pub mod data;

pub struct Db {
    // data_dir: PathBuf,
    bin_dir: PathBuf,
    db_path: PathBuf,
    db_data: AppData,
}

impl Deref for Db {
    type Target = AppData;

    fn deref(&self) -> &Self::Target {
        &self.db_data
    }
}

impl Db {
    pub async fn open_data_dir(data_dir: PathBuf, bin_dir: PathBuf) -> Result<Self> {
        if !fs::try_exists(&data_dir).await.with_context(|| {
            format!(
                "Failed to check if data directory exists at path: {}",
                data_dir.display()
            )
        })? {
            fs::create_dir_all(&data_dir).await.with_context(|| {
                format!("Failed to create data directory at: {}", data_dir.display())
            })?;
        }

        if !fs::try_exists(&bin_dir).await.with_context(|| {
            format!(
                "Failed to check if binaries directory exists at path: {}",
                bin_dir.display()
            )
        })? {
            fs::create_dir_all(&bin_dir).await.with_context(|| {
                format!(
                    "Failed to create binaries directory at: {}",
                    bin_dir.display()
                )
            })?;
        }

        let db_path = data_dir.join("data.db");

        let db_data = if db_path.exists() {
            let data = fs::read_to_string(&db_path)
                .await
                .context("Failed to read database file")?;

            serde_json::from_str(&data).with_context(|| {
                format!(
                    "Failed to parse database file at: {}",
                    db_path.to_string_lossy()
                )
            })?
        } else {
            AppData::default()
        };

        Ok(Self {
            // data_dir,
            bin_dir,
            db_path,
            db_data,
        })
    }

    pub async fn update(&mut self, with: impl FnOnce(&mut AppData)) -> Result<()> {
        with(&mut self.db_data);

        let data = serde_json::to_string(&self.db_data)
            .map_err(|err| anyhow!("Failed to serialize database: {err:?}"))?;

        fs::write(&self.db_path, data)
            .await
            .context("Failed to write database content to disk")?;

        Ok(())
    }

    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }
}
