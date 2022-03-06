use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

/// The main cli of the app
#[derive(Parser)]
#[clap(author="@cdecompilador", 
       version,
       about="Easy to use c build system", 
       long_about = None)]
pub(crate) struct Cli {
    /// Subcommands of `amargo` here is the important stuff like `run`, `build,
    /// `new`.
    #[clap(subcommand)]
    pub commands: Command,
}

/// The configurations extracted from the `Amargo.toml`
#[derive(serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub project: Project,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Project {
    pub name: String,
}

/// All the configs needed of the project to execute any subcommand in `amargo`
/// or call the build AP
pub struct ProjectConfig {
    pub(crate) cli: Cli,
    pub config: Option<Config>,
    pub working_dir: PathBuf,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        unreachable!()
    }
}

/// Types of projects that can be created
/// TODO: Figure out how to call them like `--binary`, `--static` and so on.
#[derive(parse_display::Display, clap::ArgEnum, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    /// Binary project that generates an executable, creates a layout with a
    /// main.c
    #[display("binary (application)")]
    #[clap(name = "binary")]
    Binary,

    /// Library project with a entry lib.c that will compile to a
    /// <project_name>.h and a <project_name>.a/lib
    #[display("library (static)")]
    #[clap(name = "static")]
    StaticLib,

    /// Library project with a entry lib.c that will compile to a
    /// <project_name>.h and a <project_name>.so/dll
    #[display("library (dynamic)")]
    #[clap(name = "dynamic")]
    DynamicLib,

    /// Header only project that will group all the headers into a single one
    #[display("library (header-only)")]
    #[clap(name = "header")]
    HeaderOnly,
}

/// Needed by the `Tool` to know which command to output
#[derive(parse_display::Display, clap::ArgEnum, Clone, Copy, PartialEq, Eq)]
pub enum BuildType {
    /// No symbols + optimizations
    #[display("release [optimized]")]
    Release,

    /// Symbols and no optimizations
    #[display("debug [debug symbols + no optimized]")]
    Debug,
}

impl From<BuildType> for PathBuf {
    fn from(build_type: BuildType) -> PathBuf {
        match build_type {
            BuildType::Release => Path::new("target").join("release"),
            BuildType::Debug => Path::new("target").join("debug"),
        }
    }
}

#[derive(Subcommand, PartialEq, Eq)]
pub(crate) enum Command {
    /// Create a new project of a certain type with `project_name`
    New {
        /// The name of the project
        project_name: String,

        /// The type of the project
        #[clap(arg_enum, default_value_t=ProjectType::Binary)]
        project_type: ProjectType,
    },

    /// Builds the project if it has benn updated
    #[clap(visible_alias = "b")]
    Build {
        #[clap(arg_enum, default_value_t=BuildType::Debug)]
        mode: BuildType,
    },

    /// Builds the project if it has been updated and runs it (build + run)
    #[clap(visible_alias = "r")]
    Run {
        #[clap(arg_enum, default_value_t=BuildType::Debug)]
        mode: BuildType,

        /// The arguments provided in the form `-- <exe_args..>` they are
        /// passed as arguments to the target to run (if any)
        #[clap(last = true)]
        exe_args: Vec<String>,
    },

    /// Removes the `target` folder and other intermediate artifacts created
    /// by a compilation
    #[clap(visible_alias = "c")]
    Clean,
}
