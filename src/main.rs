use std::{
    convert::AsRef,
    path::{Path, PathBuf},
    fs,
    time::Instant
};

use clap::{App, SubCommand, Arg};
use console::style;

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

/// Error type used in the program
/// TODO: Use some macro crate so the displayed message on panic is better
#[derive(Debug)]
enum Error {
    /// The current dir is invalid (not enough perms or just it does not exist)
    CurrentDirInvalid(PathBuf, std::io::Error),

    /// Invalid new project path
    InvalidProjectPath(PathBuf),

    /// Impossible to create an object (also used in case is impossible to 
    /// create and then write)
    CannotCreate(PathBuf, std::io::Error),

    /// While building, running or checking the program has relized that this is
    /// not an amargo project
    NotAProject(PathBuf),

    /// While recursive listing files in `src` or `include` some unexpected io
    /// error happened
    FileListing(walkdir::Error),

    /// Error while executing command
    ProcessExec(std::io::Error),

    /// Error while compilating
    /// TODO: For the moment this contains nothing, but in the future I'd like
    /// the tool to have a check subcommand like cargo that statically checks 
    CompilationError,
}

#[derive(parse_display::Display, PartialEq)]
enum CrateType {
    #[display("binary (application)")]
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

/// Returns `true` if a `Entry` is a source file, c
fn is_source(entry: &walkdir::DirEntry) -> bool {
    entry.path().is_file() && entry.path().extension().unwrap() == "c"
}

/// TODO: Handle multriple compilers, for the moment just clang, but it would 
/// be great to also allow gcc, msvc, cl-clang
/// TODO: Realize if its inside a project dir like for example 
/// `<project_name>/src/folder_a`
/// Builds the binary of a project
fn build_project(mode: &str) -> Result<(), Error> {
    // TODO: Check if this an amargo project

    // TODO: Retrieve the last build instant from the last modification on the 
    // `target` folder, if it does not exist set it to None and build everything
    // let last_build = std::time::SystemTime::now();
    let working_dir = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;
    // TODO: This later might be in a *.toml file
    let project_name = working_dir.components().last().unwrap().as_os_str();
    let opt_args = if mode == "debug" { "-g" } else { "-Ofast" };

    // Use walkdir to retrieve all the source files
    // TODO: Check the for files that don't need to be recompiled
    let mut source_files = Vec::new();
    for entry in walkdir::WalkDir::new(working_dir.join("src")) {
        // DirEntry -> PathBuf
        let path = entry.map_err(Error::FileListing)?.into_path();
        // Just push the source files
        if path.is_file() && path.extension().unwrap() == "c" {
            // Strip the prefix of the path for them the clang errors not be
            // too verbose
            //
            // Example: \\?\C:\Users\username\amargo\hello\src\main.c:2:10  :d
            source_files.push(
                path.strip_prefix(&working_dir).unwrap().to_path_buf()
            );
        }
    }

    // Create the build target dir if it does not exist
    let out_dir = Path::new("target")
        .join(mode);
    fs::create_dir_all(&out_dir)
        .map_err(|e| Error::CannotCreate(working_dir.clone(), e))?;

    // Format source files to create the command
    let out_dir = out_dir.join(project_name).with_extension("exe");
    let mut args = vec![
        opt_args.to_string(), 
        "-Iinclude".to_string(),
        "-o".to_string(), 
        out_dir.to_str().unwrap().to_string()
    ];
    let source_files = source_files.into_iter()
        .map(|entry| entry.to_str().unwrap().to_owned());
    args.extend(source_files);

    // Execute the compilation command, saving the objects and executable in
    // target/<mode>/
    //
    // If the compilation fails print the stdout and stderr, if succeds just 
    // stdout (if succeed it does nothing with stderr... right??)
    // TODO: Compile in separate stages: Sources => Objects -> Executable
    let res = std::process::Command::new("clang")
        .args(args)
        .output()
        .map_err(Error::ProcessExec)?;
    println!("{}", std::str::from_utf8(&res.stdout).unwrap());
    if !res.status.success() {
        println!("{}", std::str::from_utf8(&res.stderr).unwrap());
    }
    
    // TODO: If something went wrong remove all the object files from this 
    // compilation stage, so they might need to compile to different locations

    Ok(())
}

fn main() -> Result<(), Error> {
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
                    .get_matches();

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

        // Get the project path creation (that is the same as the name) and 
        // create it
        let project_path = matches.value_of("project_name").unwrap();
        create_project(project_path, crate_type)?;
    } else if let Some(matches) = matches.subcommand_matches("build") {
        // Build the project passing the `mode` supplied via cli, if none was
        // supplied `--debug` assumed
        build_project(matches.value_of("mode").unwrap_or("debug"))?;
    } else if let Some(matches) = matches.subcommand_matches("run") {
        // TODO: This later might be in a *.toml file
        let project_name = {
            let working_dir = std::env::current_dir()
                .and_then(|d| d.canonicalize())
                .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;
            working_dir.components().last().unwrap().as_os_str().to_os_string()
        };

        let it = Instant::now();

        // Print that compilation has started
        print!("{:>12} {:?}", 
            style("Compiling").cyan(), 
            project_name);

        // First compile the project.
        // TODO: Check if it's needed to recompile and just rebuild what needs
        // to be rebuild
        let mode = matches.value_of("mode").unwrap_or("debug");
        build_project(mode)?;

        // Print to console that compilation has finished
        let elapsed = (Instant::now() - it).as_secs_f64();
        println!("{:>12} {} {:?} in {}s", 
            style("Finished").cyan(), 
            format!("{} [{}]",
                mode, 
                if mode == "release" { "optimized" } else { "debug symbols" }),
            project_name,
            elapsed);
        
        // Generate the path to the executable, get the project name (that is
        // the same as the executable name)
        //

        let executable_path = Path::new("target")
            .join(mode)
            .join(project_name)
            .with_extension("exe");

        // Spawn the process of the binary application supplying the arguments
        // passed to `amargo` via `-- <args>...`
        let args = matches.values_of("exe_args")
            .map(|vals| vals.collect::<Vec<_>>())
            .unwrap_or_default();
        std::process::Command::new(&executable_path)
            .args(&args)
            .spawn().expect("Couldn't exectute binary application");

        // Print that compilation has started
        println!("{:>12} `{} {}`\n", 
            style("Running").cyan(), 
            executable_path.display(),
            &args[..].join(" "));
    }

    Ok(())
}
