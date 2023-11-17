use serde::{Deserialize, Serialize};

use crate::{arch::PlatformDependent, fetcher::AssetInfos, repository::FileExtraction};

use super::AssetSource;

#[derive(Serialize, Deserialize)]
pub struct DirectSourceParams {
    pub urls: PlatformDependent<(String, FileExtraction)>,
    pub hardcoded_version: String,
}

pub struct DirectSource;

impl AssetSource for DirectSource {
    type Params = DirectSourceParams;

    fn make_parser() -> Box<dyn parsy::Parser<Self>> {
        todo!()
    }

    fn fetch(params: &Self::Params) -> anyhow::Result<AssetInfos> {
        let DirectSourceParams {
            urls,
            hardcoded_version,
        } = params;

        let (url, extraction) = urls.get_for_current_platform()?.clone();

        Ok(AssetInfos {
            url,
            version: hardcoded_version.clone(),
            extraction,
        })
    }
}
