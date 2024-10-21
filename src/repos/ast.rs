use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::sources::{direct::DirectSource, github::GitHubSource};

#[macro_export]
macro_rules! ast_friendly {
    ($($typedecl: item)+) => {
        $(
            #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            $typedecl
        )+
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub name: String,
    pub description: String,
    pub packages: HashMap<String, PackageManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub name: String,
    pub source: DownloadSource,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DownloadSource {
    Direct(DirectSource),
    GitHub(GitHubSource),
}
