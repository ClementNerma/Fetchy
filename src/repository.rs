use once_cell::sync::Lazy;
use pomsky_macro::pomsky;
use regex::Regex;
use serde::{de::Error, Deserialize, Deserializer, Serialize};

use crate::{arch::PlatformDependent, pattern::Pattern};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    #[serde(deserialize_with = "deserialize_name")]
    pub name: String,
    pub description: String,
    pub packages: Vec<Package>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    #[serde(deserialize_with = "deserialize_name")]
    pub name: String,
    pub download: PackageDownload,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDownload {
    pub source: DownloadSource,
    pub file_format: FileFormat,
    pub version_extraction: VersionExtraction,
    pub skip_version_validation: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DownloadSource {
    Direct {
        url: PlatformDependent<String>,
    },
    GitHub {
        author: String,
        repo_name: String,
        asset_pattern: PlatformDependent<Pattern>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum FileFormat {
    Binary {
        #[serde(deserialize_with = "deserialize_filename")]
        filename: String,
    },
    Archive {
        format: ArchiveFormat,
        files: Vec<FileExtraction>,
    },
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    TarXz,
    Zip,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum VersionExtraction {
    Url { regex: Pattern },
    ReleaseTitle { regex: Option<Pattern> },
    TagName { regex: Option<Pattern> },
    DownloadedFileName { regex: Pattern },
    Hardcoded(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileExtraction {
    pub relative_path: Pattern,
    pub file_type: AssetFileType,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum AssetFileType {
    Binary { copy_as: String },
    ConfigDir,
    ConfigSubDir { copy_as: String },
}

fn deserialize_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let slug = String::deserialize(deserializer)?;

    if !VALIDATE_SLUG_ID.is_match(&slug) {
        Err(D::Error::custom(&format!("Invalid slug provided: {slug}")))
    } else {
        Ok(slug)
    }
}

fn deserialize_filename<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let slug = String::deserialize(deserializer)?;

    if slug.starts_with('.') || !VALIDATE_FILENAME.is_match(&slug) {
        Err(D::Error::custom(&format!(
            "Invalid filename provided: {slug}"
        )))
    } else {
        Ok(slug)
    }
}

static VALIDATE_SLUG_ID: Lazy<Regex> = Lazy::new(|| {
    Regex::new(pomsky!(
        Start ['a'-'z' 'A'-'Z' '0'-'9' '_' '-' '.']+ End
    ))
    .unwrap()
});

static VALIDATE_FILENAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(pomsky!(
        Start ['a'-'z' 'A'-'Z' '0'-'9' '_' '-' '.']+ End
    ))
    .unwrap()
});
