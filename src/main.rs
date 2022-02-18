use std::{
    convert::AsRef,
    path::{Path, PathBuf},
    fs,
    time::Instant,
    process::Command,
    ffi::{OsString, OsStr},
    iter::IntoIterator
};

mod tool;
mod error;

use crate::{
    tool::Build,
    error::Error,
};

use clap::{App, SubCommand, Arg};
use console::style;
use log::{trace, warn, info};

// Import the startup binaries
const BINARY_MAIN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/main.c"
));
const BINARY_LIB_C: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/add.c"
));
const BINARY_LIB_H: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/add.h"
));

#[cfg(target_os = "windows")]
const EXE_EXTENSION: &str = "exe";
#[cfg(not(target_os = "windows"))]
const EXE_EXTENSION: &str = "out";

/// Types of crates that can be created 
#[derive(parse_display::Display, PartialEq, educe::Educe)]
#[educe(Default)]
enum CrateType {
    #[display("binary (application)")]
    #[educe(Default)]
    Binary,
    #[display("library (static)")]
    StaticLib,
    #[display("library (dynamic)")]
    DynamicLib,
    #[display("library (header-only)")]
    HeaderOnly
}

/// Created a project of type `CrateType` on the location and with name `path`
fn create_project(path: impl AsRef<Path>, ty: CrateType) -> Result<(), Error> {
    // Check if the path already exists, or is the current one (".") to skip
    // folder creation, and working path selection
    let mut working_dir = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| Error::CurrentDirInvalid(path.as_ref().to_path_buf(), e))?;
    if path.as_ref().is_dir() {
        if path.as_ref().canonicalize().unwrap() != working_dir {
            // TODO: Check if its a amargo project and report that project 
            // already exists
            return Err(Error::InvalidProjectPath(path.as_ref().to_path_buf()));
        }
    } else {
        working_dir.push(path.as_ref());
        fs::create_dir_all(&working_dir)
            .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;
    }

    // TODO: Handle other `CrateType`s apart from Binary
    // Create the `src` and `include` dir, and its inner files
    fs::create_dir(&working_dir.join("src"))
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;
    fs::write(&working_dir.join("src").join("main.c"), BINARY_MAIN)
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;
    fs::write(&working_dir.join("src").join("add.c"), BINARY_LIB_C)
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;
    fs::create_dir(&working_dir.join("include"))
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;
    fs::write(&working_dir.join("include").join("add.h"), BINARY_LIB_H)
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;

    // Print to console that the package has been created
    println!("{:>12} {} `{}` package", 
        style("Created").cyan(), 
        ty.to_string(),
        path.as_ref().display());

    Ok(())
}

/// TODO: Realize if its inside a project dir like for example 
/// `<project_name>/src/folder_a`
/// Builds the binary of a project
fn build_project(mode: &str) -> Result<bool, Error> {
    // TODO: Check if this an amargo project

    // Compile and link the project given the mode
    // TODO: If something went wrong remove all the object files from this 
    // compilation stage, so they might need to compile to different locations
    // TODO: Also check includes for incremental compilation
    Ok(Build::new(mode)?
        .include("include")?
        .files("src")?
        .compile()?
        .link()?)
}

fn main() -> Result<(), Error> {
    // Initialize the log backend and retrieve the argument matches
    pretty_env_logger::init();
    let matches = App::new("amargo")
                    .subcommand(SubCommand::with_name("new")
                        .about("Creates a new amargo project")
                        .arg(Arg::with_name("project_name")
                            .required(true)
                            .help("The project name")
                            .index(1))
                        .arg(Arg::with_name("lib").long("lib"))
                        .arg(Arg::with_name("dylib").long("dylib"))
                        .arg(Arg::with_name("header-only").long("header-only")))
                    .subcommand(SubCommand::with_name("build")
                        .alias("b")
                        .about("Builds the package")
                        .arg(Arg::with_name("mode")
                            .possible_values(&["debug", "release"])
                            .help("The optimization level of compilation")))
                    .subcommand(SubCommand::with_name("run")
                        .alias("r")
                        .about("Build and run the binary application (if exists)")
                        .arg(Arg::with_name("mode")
                            .possible_values(&["debug", "release"])
                            .help("The optimization level of compilation"))
                        .arg(Arg::with_name("exe_args")
                            .short("--")
                            .multiple(true)
                            .takes_value(true)
                            .value_delimiter(" ")
                            .last(true)
                            .help("Provide cli args to the executable")))
                    .subcommand(SubCommand::with_name("clean")
                        .alias("c")
                        .about("Cleans the build targets if the application"))
                    .get_matches();

    // Get the working directory
    let working_dir = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;
    info!("Working dir {:?}", &working_dir);

    if let Some(matches) = matches.subcommand_matches("new") {
        // Select the `CrateType`, if none is provided select Binary
        let crate_type = if matches.is_present("lib") {
            CrateType::StaticLib
        } else if matches.is_present("dylib") {
            CrateType::DynamicLib
        } else if matches.is_present("header-only") {
            CrateType::HeaderOnly
        } else {
            CrateType::Binary
        };

        info!("Selected project creation option: `{}`", crate_type);

        // Get the project path creation (that is the same as the name) and 
        // create it
        let project_path = matches.value_of("project_name").unwrap();
        create_project(project_path, crate_type)?;
    } else if let Some(matches) = matches.subcommand_matches("build") {
        let project_name =
            working_dir.components().last().unwrap().as_os_str().to_os_string();
        let it = Instant::now();

        info!("Selected build option of {:?}", &project_name);

        // Print that compilation has started
        println!("{:>12} {:?}", 
            style("Compiling").cyan(), 
            project_name);

        // Build the project passing the `mode` supplied via cli, if none was
        // supplied `--debug` assumed
        let mode = matches.value_of("mode").unwrap_or("debug");
        let changes = build_project(mode)?;

        // Print to console that compilation has finished
        if !changes {
            println!("{:>12} {} {:?} {}", 
                style("Finished").cyan(), 
                format!("{} [{}]",
                    mode, 
                    if mode == "release" { "optimized" } else { "debug symbols" }),
                    project_name,
                    "Already up to date");
        } else {
            let elapsed = (Instant::now() - it).as_secs_f64();
            println!("{:>12} {} {:?} in {:.2}s", 
                style("Finished").cyan(), 
                format!("{} [{}]",
                    mode, 
                    if mode == "release" { "optimized" } else { "debug symbols" }),
                    project_name,
                    elapsed);
        }

    } else if let Some(matches) = matches.subcommand_matches("run") {
        // TODO: This later might be in a *.toml file
        let project_name =
            working_dir.components().last().unwrap().as_os_str().to_os_string();
        info!("Selected run option of {:?}", &project_name);
        let it = Instant::now();

        // Print that compilation has started
        println!("{:>12} {:?}", 
            style("Compiling").cyan(), 
            project_name);

        // First compile the project.
        let mode = matches.value_of("mode").unwrap_or("debug");
        let changes = build_project(mode)?;

        // Print to console that compilation has finished
        if !changes {
            println!("{:>12} {} {:?} {}", 
                style("Finished").cyan(), 
                format!("{} [{}]",
                    mode, 
                    if mode == "release" { "optimized" } else { "debug symbols" }),
                    project_name,
                    "Already up to date");
        } else {
            let elapsed = (Instant::now() - it).as_secs_f64();
            println!("{:>12} {} {:?} in {:.2}s", 
                style("Finished").cyan(), 
                format!("{} [{}]",
                    mode, 
                    if mode == "release" { "optimized" } else { "debug symbols" }),
                    project_name,
                    elapsed);
        }
        
        // Generate the path to the executable, get the project name (that is
        // the same as the executable name)
        let executable_path = Path::new("target")
            .join(mode)
            .join(project_name)
            .with_extension(EXE_EXTENSION);

        // Spawn the process of the binary application supplying the arguments
        // passed to `amargo` via `-- <args>...`
        let args = matches.values_of("exe_args")
            .map(|vals| vals.collect::<Vec<_>>())
            .unwrap_or_default();
        Command::new(&executable_path)
            .args(&args)
            .spawn().expect("Couldn't exectute binary application");

        // Print that the executable is being run
        let mut formatted_args: String = args[..].join(" ");
        if formatted_args.len() != 0 {
            formatted_args.insert(0, ' ');
        }
        println!("{:>12} `{}{}`\n", 
            style("Running").cyan(), 
            executable_path.display(),
            formatted_args);

    } else if let Some(matches) = matches.subcommand_matches("clean") {
        fs::remove_dir_all(working_dir.join("target"))
            .expect("Cannot remove target dir");
    }

    Ok(())
}
