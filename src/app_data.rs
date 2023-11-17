use std::{path::PathBuf, time::SystemTime};

use serde::{Deserialize, Serialize};

use crate::repository::Repository;

#[derive(Default, Serialize, Deserialize)]
pub struct AppState {
    pub installed: Vec<InstalledPackage>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Repositories {
    pub list: Vec<SourcedRepository>,
}

#[derive(Serialize, Deserialize)]
pub struct SourcedRepository {
    pub content: Repository,
    pub source: RepositorySource,
}

#[derive(Serialize, Deserialize)]
pub enum RepositorySource {
    File(PathBuf),
    // Url(String),
}

#[derive(Serialize, Deserialize)]
pub struct PackageVersion(pub String);

#[derive(Serialize, Deserialize)]
pub struct InstalledPackage {
    pub pkg_name: String,
    pub repo_name: String,
    pub version: String,
    pub at: SystemTime,
    pub binaries: Vec<String>,
    pub libraries: Vec<String>,
    pub data_dirs: Vec<String>,
}
