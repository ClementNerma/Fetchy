use once_cell::sync::Lazy;
use pomsky_macro::pomsky;
use regex::Regex;
use serde::{de::Error, Deserialize, Deserializer, Serialize};

use crate::{arch::PlatformDependent, pattern::Pattern};

#[derive(Serialize, Deserialize)]
pub struct Repository {
    #[serde(deserialize_with = "deserialize_name")]
    pub name: String,
    pub description: String,
    pub packages: Vec<Package>,
}

#[derive(Serialize, Deserialize)]
pub struct Package {
    #[serde(deserialize_with = "deserialize_name")]
    pub name: String,
    pub download: PackageDownload,
}

#[derive(Serialize, Deserialize)]
pub struct PackageDownload {
    pub source: DownloadSource,
    pub file_format: FileFormat,
    pub version_extraction: VersionExtraction,
}

#[derive(Serialize, Deserialize)]
pub enum DownloadSource {
    Direct {
        url: String,
    },
    GitHub {
        author: String,
        repo_name: String,
        asset_pattern: PlatformDependent<Pattern>,
    },
}

#[derive(Serialize, Deserialize)]
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
pub struct VersionExtraction {
    pub source: VersionExtractionSource,
    pub regex: Option<Pattern>,
    pub skip_validation: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub enum VersionExtractionSource {
    Url,
    ReleaseTitle,
    DownloadedFileName,
    TagName,
}

#[derive(Serialize, Deserialize)]
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
