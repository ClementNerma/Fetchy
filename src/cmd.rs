use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
    Path(PathArgs),

    #[clap(name = "repos:add", about = "Add a repository")]
    AddRepo(AddRepoArgs),

    #[clap(name = "repos:update", about = "Update repositories")]
    UpdateRepos(UpdateReposArgs),

    #[clap(name = "repos:list", about = "List registered repositories")]
    ListRepos(ListReposArgs),

    // #[clap(about = "Remove repositories")]
    // RemoveRepos(RemoveReposArgs),
    #[clap(about = "Install packages")]
    Install(InstallArgs),

    #[clap(about = "Require some packages to be installed")]
    Require(RequireArgs),

    #[clap(about = "Check if a list of packages is installed")]
    CheckInstalled(CheckInstalledArgs),

    #[clap(about = "Update installed packages")]
    Update(UpdateArgs),
    // #[clap(about = "Uninstall packages")]
    // Uninstall(UninstallArgs),
}

#[derive(Args)]
pub struct PathArgs {}

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
pub struct UpdateReposArgs {}

#[derive(Args)]
pub struct ListReposArgs {}

#[derive(Args)]
pub struct InstallArgs {
    #[clap(help = "Name of the package(s) to install")]
    pub names: Vec<String>,
}

#[derive(Args)]
pub struct RequireArgs {
    #[clap(help = "Name of the package(s) to install")]
    pub names: Vec<String>,

    #[clap(short, long, help = "Ask for confirmation before installing")]
    pub confirm: bool,
}

#[derive(Args)]
pub struct CheckInstalledArgs {
    #[clap(help = "Name of the package(s) to check")]
    pub names: Vec<String>,
}

#[derive(Args)]
pub struct UpdateArgs {
    #[clap(help = "List of packages to update (all if none provided)")]
    pub names: Vec<String>,
}
