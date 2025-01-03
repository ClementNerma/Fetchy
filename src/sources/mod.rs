use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{de::DeserializeOwned, Serialize};

use crate::ast_friendly;

use self::pattern::Pattern;

pub mod direct;
pub mod github;
pub mod pattern;

pub trait AssetSource: Serialize + DeserializeOwned {
    fn validate(&self) -> Vec<String>;
    async fn fetch_infos(&self) -> Result<AssetInfos>;
}

#[derive(Debug, Clone)]
pub struct AssetInfos {
    pub url: String,
    pub headers: HeaderMap<HeaderValue>,
    pub version: String,
    pub typ: AssetType,
}

ast_friendly! {
    pub enum AssetType {
        Binary {
            copy_as: String,
        },
        Archive {
            format: ArchiveFormat,
            files: Vec<BinaryInArchive>,
        },
    }

    #[derive(Copy)]
    pub enum ArchiveFormat {
        TarGz,
        TarXz,
        Zip,
    }

    pub struct BinaryInArchive {
        pub path_matcher: Pattern,
        pub copy_as: String,
    }
}
