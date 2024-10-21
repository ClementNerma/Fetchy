use std::collections::BTreeMap;

use jiff::Zoned;
use serde::{Deserialize, Serialize};

use crate::{
    fetch_repos::RepositorySource,
    repos::ast::{PackageManifest, Repository},
};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    pub repositories: BTreeMap<String, SourcedRepository>,
    pub installed: BTreeMap<String, InstalledPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcedRepository {
    pub content: Repository,
    pub source: RepositorySource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub manifest: PackageManifest,
    pub repo_name: String,
    pub version: String,
    pub at: Zoned,
    pub binaries: Vec<String>,
    pub installed_as_dep: bool,
}
