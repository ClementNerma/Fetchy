use anyhow::Result;
use colored::Colorize;
use reqwest::{header::HeaderMap, Url};
use serde::{Deserialize, Serialize};

use crate::{repos::arch::PlatformDependent, validator::validate_asset_type};

use super::{AssetInfos, AssetSource, AssetType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectSource {
    pub urls: PlatformDependent<(String, AssetType)>,
    pub hardcoded_version: String,
}

impl AssetSource for DirectSource {
    fn validate(&self) -> Vec<String> {
        let Self {
            urls,
            hardcoded_version: _,
        } = self;

        let mut errors = vec![];

        for (url, asset_typ) in urls.values() {
            if let Err(err) = Url::parse(url) {
                errors.push(format!(
                    "Invalid asset URL {}: {err}",
                    format!("{url:?}").bright_magenta()
                ));
            }

            validate_asset_type(asset_typ, &mut errors);
        }

        errors
    }

    async fn fetch_infos(&self) -> Result<AssetInfos> {
        let Self {
            urls,
            hardcoded_version,
        } = self;

        let (url, content) = urls.get_for_current_platform()?;

        Ok(AssetInfos {
            url: url.clone(),
            headers: HeaderMap::new(),
            version: hardcoded_version.clone(),
            typ: content.clone(),
        })
    }
}
