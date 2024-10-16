use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[clap(version, about, author)]
pub struct Cmd {
    #[clap(subcommand)]
    pub action: Action,

    #[clap(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[clap(short, long, global = true, conflicts_with = "quiet")]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Action {
    #[clap(
        subcommand,
        about = "Path to the binaries directory (or the provided one)"
    )]
    Path(PathAction),

    #[clap(subcommand, about = "Manage repositories")]
    Repos(ReposAction),

    #[clap(about = "Search available packages")]
    Search(SearchArgs),

    #[clap(about = "Install packages")]
    Install(InstallArgs),

    #[clap(about = "List installed packages")]
    Installed(InstalledArgs),

    #[clap(about = "Update installed packages")]
    Update(UpdateArgs),

    #[clap(about = "Uninstall packages")]
    Uninstall(UninstallArgs),
}

#[derive(Subcommand)]
pub enum PathAction {
    #[clap(about = "Get path to the binaries directory")]
    Binaries,

    #[clap(about = "Get full path to an installed package")]
    ProgramBinary { name: String },
    //
    // #[clap(about = "Get the isolated data directory of a specific program")]
    // ProgramData { name: String },
}

#[derive(Subcommand)]
pub enum ReposAction {
    #[clap(about = "Add a repository")]
    Add(AddRepoArgs),

    #[clap(about = "Update repositories")]
    Update,

    #[clap(about = "List registered repositories")]
    List,

    #[clap(about = "Validate a standalone repository file, without adding it")]
    Validate(ValidateRepoFileArgs),
}

#[derive(Args)]
pub struct AddRepoArgs {
    #[clap(help = "Fetch repository from a local file")]
    pub file: PathBuf,

    #[clap(
        short,
        long,
        help = "Ignore if a repository with this name is already registered"
    )]
    pub ignore: bool,
}

#[derive(Args)]
pub struct ValidateRepoFileArgs {
    #[clap(help = "Path to the file to validate")]
    pub file: PathBuf,
}

#[derive(Args)]
pub struct SearchArgs {
    #[clap(help = "Filter packages with a glob pattern")]
    pub filter: Option<String>,

    #[clap(short, long, help = "Don't hide installed packages")]
    pub show_installed: bool,
}

#[derive(Args)]
pub struct InstallArgs {
    #[clap(help = "Name of the package(s) to install")]
    pub names: Vec<String>,

    #[clap(
        short,
        long,
        help = "Install packages even if they are already installed"
    )]
    pub force: bool,
}

#[derive(Args)]
pub struct InstalledArgs {
    #[clap(help = "Sort packages")]
    pub sort_by: Option<PkgSortBy>,

    #[clap(short, long, help = "Reverse sort order")]
    pub rev_sort: bool,
}

#[derive(ValueEnum, Clone, Copy)]
pub enum PkgSortBy {
    Name,
    InstallDate,
}

#[derive(Args)]
pub struct UpdateArgs {
    #[clap(help = "List of packages to update (all if none provided)")]
    pub names: Vec<String>,
}

#[derive(Args)]
pub struct UninstallArgs {
    #[clap(help = "Name of the package to uninstall")]
    pub name: String,
}
