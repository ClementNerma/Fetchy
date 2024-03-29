use serde::{Deserialize, Serialize};

use crate::{
    pattern::Pattern,
    sources::{direct::DirectSourceParams, github::GitHubSourceParams},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub name: String,
    pub description: String,
    pub packages: Vec<Package>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub source: DownloadSource,
    pub depends_on: Option<Vec<String>>
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DownloadSource {
    Direct(DirectSourceParams),
    GitHub(GitHubSourceParams),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub enum FileExtraction {
    Binary {
        copy_as: String,
    },
    Archive {
        format: ArchiveFormat,
        files: Vec<SingleFileExtraction>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum ArchiveFormat {
    TarGz,
    TarXz,
    Zip,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SingleFileExtraction {
    pub relative_path: Pattern,
    pub nature: FileNature,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum FileNature {
    Binary { copy_as: String },
    Library { name: String },
    IsolatedDir { name: String },
}
