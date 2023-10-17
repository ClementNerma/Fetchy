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
    #[clap(about = "Path to the binaries directory")]
    Path,

    #[clap(subcommand, about = "Manage repositories")]
    Repos(ReposAction),

    #[clap(about = "Search available packages")]
    Search(SearchArgs),

    #[clap(about = "Install packages")]
    Install(InstallArgs),

    #[clap(about = "List installed packages")]
    Installed(InstalledArgs),

    #[clap(about = "Require some packages to be installed")]
    Require(RequireArgs),

    #[clap(about = "Update installed packages")]
    Update(UpdateArgs),

    #[clap(about = "Uninstall packages")]
    Uninstall(UninstallArgs),
}

// #[derive(Args)]
// pub struct PathArgs {}

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

// #[derive(Args)]
// pub struct UpdateReposArgs {}

// #[derive(Args)]
// pub struct ListReposArgs {}

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
pub struct RequireArgs {
    #[clap(help = "Name of the package(s) to install")]
    pub names: Vec<String>,

    #[clap(short, long, help = "Don't install missing packages")]
    pub no_install: bool,

    #[clap(
        short,
        long,
        conflicts_with = "no_install",
        help = "Ask for confirmation before installing"
    )]
    pub confirm: bool,
}

#[derive(Args)]
pub struct UpdateArgs {
    #[clap(help = "List of packages to update (all if none provided)")]
    pub names: Vec<String>,

    #[clap(help = "Force update even if no update was detected")]
    pub force: bool,
}

#[derive(Args)]
pub struct UninstallArgs {
    #[clap(help = "Name of the package to uninstall")]
    pub name: String,
}
