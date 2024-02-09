use serde::{de::DeserializeOwned, Serialize};

use crate::fetcher::AssetInfos;

pub mod direct;
pub mod github;

pub trait AssetSource {
    type Params: Serialize + DeserializeOwned;

    // fn make_parser() -> Box<dyn Parser<Self>>;

    fn fetch(params: &Self::Params) -> anyhow::Result<AssetInfos>;
}
