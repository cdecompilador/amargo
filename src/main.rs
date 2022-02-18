use std::{
    path::{Path, PathBuf},
    fs,
    time::Instant,
    process,
};

mod tool;
mod error;
mod config;
mod build;

use crate::{
    build::Build,
    error::Error,
    config::{ProjectType, BuildType, ProjectConfig, Config, Cli, Command, Project}
};

use clap::Parser;
use console::style;
use log::info;
use include_dir::{Dir, include_dir};

// Import the template dirs
// TODO: Add the rest
const BINARY_TEMPLATE: Dir = include_dir!("./templates/binary");

// The extension of the executable is platform dependent
#[cfg(target_os = "windows")]
const EXE_EXTENSION: &str = "exe";
#[cfg(not(target_os = "windows"))]
const EXE_EXTENSION: &str = "out";

/// Created a project of type `CrateType` on the location and with name `path`
fn create_project(
    config:       &ProjectConfig,
    project_type: ProjectType
) -> Result<(), Error> {
    let project_config = config.config.as_ref().unwrap();
    let project_name   = &project_config.project.name;
    let project_path   = config.working_dir.join(project_name);

    // Check if the project already exists
    if project_path.join("Amargo.toml").is_file() {
        println!("Project with name {} already exists", project_name);
        std::process::exit(0);
    }

    // Extract the project template on `project_path`
    match project_type {
        ProjectType::Binary => {
            BINARY_TEMPLATE.extract(&project_path)
        },
        p => todo!("Project type {} not implemented yet", p)
    }.map_err(|e| Error::CannotCreate(project_path.clone(), e))?;

    // Write the `Amargo.toml` from the already generated config
    let toml_path = project_path.join("Amargo.toml");
    let toml = toml::to_string(project_config).unwrap();
    std::fs::write(&toml_path, &toml[..])
        .map_err(|e| Error::CannotCreate(toml_path, e))?;

    // Print to console that the package has been created
    println!("{:>12} {} `{}` package", 
        style("Created").cyan(), 
        project_type.to_string(),
        project_name);

    Ok(())
}

/// Builds the binary of a project
fn build_project(config: &ProjectConfig, mode: BuildType) -> Result<bool, Error> {
    // Compile and link the project given the mode
    Build::new(config, mode)?
        .include("include")?
        .files("src")?
        .compile()?
        .link()
}

fn main() -> Result<(), Error> {
    // Initialize the log backend and retrieve the argument matches
    pretty_env_logger::init();
    
    // Extract all the configs
    let mut config = ProjectConfig {
        cli: Cli::parse(),
        config: {
            if !Path::new("Amargo.toml").is_file() {
                None
            } else {
                let config_file_data = std::fs::read("Amargo.toml").unwrap();
                toml::from_slice(&config_file_data[..]).ok()
            }

        },
        // TODO: Recursive ascend detect the project, for example you can be inside 
        // <project_name>/src/subdir and the `amargo b` still would need to work
        working_dir: std::env::current_dir().and_then(|d| d.canonicalize())
            .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?,
    };

    info!("Working dir {:?}", &config.working_dir);



    match &config.cli.commands {
        // Create a new project with the `project_name` provided on the cli
        Command::New { project_name, project_type } => {
            // Generate the config of the project
            config.config = Some(Config {
                project: Project {
                    name: project_name.clone()
                },
            });

            info!("Creating project {} of kind {}", project_name, project_type);
            create_project(&config, *project_type)?;
        },
        // Build the project in the provided `mode` on the cli
        Command::Build { mode } => { 
            let it = Instant::now();
            let project_name = &config.config.as_ref().unwrap().project.name;

            // Check if this an amargo project 
            if !config.working_dir.join("Amargo.toml").is_file() {
                println!("No project at {:?} found", config.working_dir);
                std::process::exit(0);
            }

            info!("building {:?}", project_name);

            // Print that compilation has started
            println!("{:>12} {:?}", 
                style("Compiling").cyan(), 
                project_name);

            // Build the project and retrieve a boolean that indicates if any source needed
            // recompilation
            let changes = build_project(&config, *mode)?;

            // Print to console that compilation has finished
            if !changes {
                println!("{:>12} {} {:?} Already up to date", 
                    style("Finished").cyan(), 
                    project_name,
                    mode.to_string());

            } else {
                let elapsed = (Instant::now() - it).as_secs_f64();
                println!("{:>12} {} {:?} in {:.2}s", 
                    style("Finished").cyan(), 
                    project_name,
                    mode.to_string(), 
                    elapsed);
            }
        },
        Command::Run { mode, exe_args } => {
            let it = Instant::now();
            let project_name = &config.config.as_ref().unwrap().project.name;

            // Check if this an amargo project 
            if !config.working_dir.join("Amargo.toml").is_file() {
                println!("No project at {:?} found", config.working_dir);
                std::process::exit(0);
            }

            info!("Selected run option of {:?}", project_name);

            // Print that compilation has started
            println!("{:>12} {:?}", 
                style("Compiling").cyan(), 
                project_name);

            // First compile the project.
            let changes = build_project(&config, *mode)?;

            // Print to console that compilation has finished
            if !changes {
                println!("{:>12} {} {:?} Already up to date", 
                    style("Finished").cyan(), 
                    project_name,
                    mode.to_string());
            } else {
                let elapsed = (Instant::now() - it).as_secs_f64();
                println!("{:>12} {} {:?} in {:.2}s", 
                    style("Finished").cyan(), 
                    project_name,
                    mode.to_string(), 
                    elapsed);
            }

            // Generate the path to the executable, get the project name (that is
            // the same as the executable name)
            let executable_path = PathBuf::from(*mode)
                .join(project_name)
                .with_extension(EXE_EXTENSION);

            // Spawn the process of the binary application supplying the arguments
            // passed to `amargo` via `-- <args...>`
            process::Command::new(&executable_path)
                .args(exe_args)
                .spawn()
                .expect("Couldn't exectute binary application");

            // Print that the executable is being run
            println!("{:>12} `{} {:?}`\n", 
                style("Running").cyan(), 
                executable_path.display(),
                exe_args);
        },
        Command::Clean => {
            // Check if this an amargo project 
            if !config.working_dir.join("Amargo.toml").is_file() {
                println!("No project at {:?} found", config.working_dir);
                std::process::exit(0);
            }

            fs::remove_dir_all(&config.working_dir.join("target"))
                .expect("Cannot remove target dir");
        }
    };

    Ok(())
}
