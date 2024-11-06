use std::path::PathBuf;

use clap::{Parser, Subcommand};
use log::LevelFilter;

use crate::sources::pattern::Pattern;

#[derive(Parser)]
#[clap(version, about, author)]
pub struct Args {
    #[clap(short, long, help = "Level of verbosity", default_value = "info")]
    pub verbosity: LevelFilter,

    #[clap(subcommand)]
    pub action: Action,
}

#[derive(Subcommand)]
pub enum Action {
    #[clap(about = "Install package(s)")]
    Install {
        #[clap(help = "Name of the package(s) to install", required = true)]
        names: Vec<String>,

        // #[clap(short, long, help = "Install from a specific repository")]
        // repo: Option<String>,
        //
        #[clap(short, long, help = "Reinstall the package if it's already installed")]
        force: bool,

        #[clap(
            short,
            long,
            conflicts_with = "force",
            help = "Check updates of installed packages"
        )]
        check_updates: bool,

        #[clap(short, long, help = "Display less informations")]
        discreet: bool,
    },

    #[clap(about = "Update package(s)")]
    Update {
        #[clap(help = "Only update some package(s)")]
        names: Vec<String>,
    },

    #[clap(about = "Uninstall package(s)")]
    Uninstall {
        #[clap(help = "Name of the package(s) to uninstall", required = true)]
        names: Vec<String>,

        #[clap(
            short,
            long,
            help = "Remove their dependencies if they are not used by other packages"
        )]
        deps: bool,
    },

    #[clap(about = "List installed packages")]
    List {},

    #[clap(about = "Repair broken packages")]
    Repair {
        #[clap(help = "Only repair specific package(s)")]
        names: Vec<String>,
    },

    #[clap(about = "Search for a package in the repositories")]
    Search {
        #[clap(help = "Pattern to search (regular expression)")]
        pattern: Pattern,

        #[clap(short = 'r', long, help = "Search in a specific set of repositories")]
        in_repos: Vec<String>,

        #[clap(short, long, help = "Show installed packages as well")]
        show_installed: bool,
    },

    #[clap(about = "Add a repository")]
    AddRepo {
        #[clap(help = "Path to the repository's file")]
        path: PathBuf,

        #[clap(long, help = "Parse the repository as JSON instead of Fetchy format")]
        json: bool,

        #[clap(
            short,
            long,
            help = "Don't show warning message if repository is already registered"
        )]
        ignore: bool,
    },

    #[clap(about = "Update repositories")]
    UpdateRepos {},

    #[clap(about = "Remove one or more repositories")]
    RemoveRepos {
        #[clap(help = "Name of the repositories to remove", required = true)]
        names: Vec<String>,
    },

    #[clap(about = "List registered repositories")]
    ListRepos {},

    #[clap(about = "Get path to the binaries directory")]
    BinPath,
}
