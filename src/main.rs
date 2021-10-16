#![feature(command_access)]

use std::{
    convert::AsRef,
    path::{Path, PathBuf},
    fs,
    time::Instant,
    process::Command,
    ffi::{OsString, OsStr},
    iter::IntoIterator
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

#[cfg(target_os = "windows")]
const EXE_EXTENSION: &str = "exe";
#[cfg(not(target_os = "windows"))]
const EXE_EXTENSION: &str = "out";

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

    /// Cannot read a certain file
    CannotRead(PathBuf, std::io::Error),

    /// While recursive listing files in `src` or `include` some unexpected io
    /// error happened
    FileListing(walkdir::Error),

    /// Error while executing command
    ProcessExec(std::io::Error),

    /// Error when command cannot be spawned
    ProcessCreation(PathBuf, std::io::Error),

    /// Raised when on include relationship lookup some include or includes
    /// couldn't be resolved
    MissingIncludes(PathBuf, Vec<String>),

    /// Error while compilating
    ///
    /// TODO: For the moment this contains nothing, but in the future I'd like
    /// the tool to have a check subcommand like cargo that statically checks 
    CompilationError,

    /// Couldn't find a default compiler
    ///
    /// TODO: In the future this might have an associated `PathBuf` because it
    /// can be a custom compiler path what couldn't be found
    NoCompilerFound
}

struct Imports {
    source: PathBuf,
    includes: Vec<PathBuf>
}

impl Imports {
    /// Given a `source` path and the various include dirs, get the paths of 
    /// its includes
    ///
    /// FIXME: This is probably so slow to call multriple times, a better 
    /// implementation must be made in the future
    /// FIXME: Also must be improved the implementation as it does not resolve
    /// header <- header <- source, does not detect double indirection, a 
    /// possible solution would be to preprocess all the sources and compare
    /// not just the date but the checksum of the preprocessed file
    fn from(source: PathBuf, include_dirs: Vec<PathBuf>) -> Result<Self, Error> {
        let mut imports = Imports {
            source,
            includes: Vec::new()
        };

        // Extract the source data
        let source_data = fs::read_to_string(&imports.source)
            .map_err(|e| Error::CannotRead(imports.source.clone(), e))?;

        // Find in the source all the #include "<header>" then lookup inside 
        // the include dirs and add the path to the includes
        let re = regex::Regex::new(r#"^#include\s+"(?P<header>\w*\.h)"$"#)
            .unwrap();
        let caps = re.captures_iter(&source_data[..]);
        let mut headers = caps.map(|cap| cap["header"].to_string())
            .collect::<Vec<String>>();
        for include_dir in include_dirs {
            // Iterate every entry in the `include_dir`, check if its a header
            // and if it is, check if matches the name of the headers that the
            // source imports
            for entry in walkdir::WalkDir::new(include_dir) {
                // DirEntry -> PathBuf
                let path = entry.map_err(Error::FileListing)?.into_path();

                if path.is_file() {
                    let mut idx = 0;
                    let header_path = headers.iter().enumerate().find_map(|(i, h)| 
                        if path.file_name().unwrap() 
                            == <String as AsRef<OsStr>>::as_ref(h) {
                            idx = i;
                            // FIXME: Can this clone be avoided??
                            Some(path.clone())
                        } else {
                            None
                        }
                    );
                    
                    // If the entry corresponded to a include remove it from
                    // the `headers` and push in into `Self.includes`
                    if let Some(header_path) = header_path {
                        headers.remove(idx);
                        imports.includes.push(header_path);
                    }
                }
            }
        }

        // Check if all the includes were resolved
        if headers.len() != 0 {
            // minimize
            return Err(Error::MissingIncludes(imports.source, headers));
        }

        Ok(imports)
    }
}

/// An abstraction around the build process of a crate, in the future this
/// might be just a trait with a check and build method to allow different 
/// types of crates
#[derive(Clone, Debug, Default)]
struct Build<'a> {
    /// The project name extracted from the working directory
    ///
    /// TODO: Extract it from the config file
    project_name: OsString,

    /// The include dirs for .h (for the moment just /include)
    include_directories: Vec<PathBuf>,

    /// The objects needed at linkage
    objects: Vec<PathBuf>,

    /// The sources need to compile, not necesarely all the project sources
    /// just the ones that need to recompile
    sources: Vec<PathBuf>,

    /// "release" or "debug" for the moment
    mode: &'a str,

    /// The tool used for compilation, abstraction over the compiler, this
    /// in the future must encompass linker, assembler and even external tools
    /// to reduce binary/library size
    tool: Tool,

    /// The directory where to put the target (influenced by the `mode`)
    out_dir: PathBuf
}

impl<'a> Build<'a> {
    /// Construct a new instance of a blank set of configurations
    fn new(mode: &'a str) -> Result<Build<'a>, Error> {
        // Get the working dir
        let working_dir = std::env::current_dir()
            .and_then(|d| d.canonicalize())
            .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;

        // Create the "default" `Build` struct
        let mut build = Build {
            project_name: working_dir.components().last().unwrap().as_os_str()
                .to_owned(),
            mode,
            out_dir: Path::new("target").join(mode),
            ..Default::default()
        };

        let family = build.tool.family;
        build.tool.push_cc_arg(family.warnings_flags().into());
        if mode == "debug" {
            build.tool.push_cc_arg(family.debug_flags().into());
        } else if mode == "release" {
            build.tool.push_cc_arg(family.release_flags().into());
        }

        // Append the default include dir
        build.include(Path::new("include"));

        // Look for existing object files in the `target/<mode>` dir and add 
        // them to the `Build`
        let mut object_files = Vec::new();
        if build.out_dir.is_dir() {
            for entry in walkdir::WalkDir::new(&build.out_dir) {
                // DirEntry -> PathBuf
                let path = entry.map_err(Error::FileListing)?.into_path();
                // Just push the object files
                if path.is_file() 
                    && path.extension().unwrap_or(OsStr::new("")) == "o" {
                    object_files.push(path);
                }
            }
        }

        build.objects = object_files;

        // Retrieve the source files
        let mut source_files = Vec::new();
        for entry in walkdir::WalkDir::new(working_dir.join("src")) {
            // DirEntry -> PathBuf
            let path = entry.map_err(Error::FileListing)?.into_path();
            // Just push the source files
            if path.is_file() && path.extension().unwrap() == "c" {
                // Strip the prefix of the path to avoid verbose errors
                source_files.push(
                    path.strip_prefix(&working_dir).unwrap().to_path_buf()
                );
            }
        }
        
        // TODO: Analize in depth the sources to rebuild also the dependents of 
        // the headers. For example change in `a.h` result in recompilation of
        // `a.c` and `b.c` if they include that header

        // Filter the source files by their asociated object if:
        // LastModificationDate(source) <= LastModificationDate(object)
        // O(n^2) T-T
        for source in source_files.iter() {
            let mut found = false;
            for object in build.objects.iter() {
                if source.file_stem() == object.file_stem() {
                    found = true;
                    let object_last_m = object.metadata().unwrap()
                        .modified().unwrap();
                    let source_last_m = source.metadata().unwrap()
                        .modified().unwrap();
                    
                    if source_last_m > object_last_m {
                        build.sources.push(source.clone());
                    }
                }
            }

            if !found {
                build.sources.push(source.clone());
            }
        }

        Ok(build)
    }

    /// Add include dir
    /// TODO: Check if it exists because it can be provided by a config file
    /// in the future
    #[inline]
    fn include<P: AsRef<Path>>(&mut self, dir: P) -> &mut Build<'a> {
        self.include_directories.push(dir.as_ref().to_path_buf());
        self
    }

    /// Add an arbitrary object file to link in
    #[inline]
    fn object<P: AsRef<Path>>(&mut self, obj: P) -> &mut Build<'a> {
        self.objects.push(obj.as_ref().to_path_buf());
        self
    }

    /// Compile the sources to objects (if they need to)
    fn compile(&mut self) -> Result<&mut Build<'a>, Error> {
        // Create the build target dir if it does not exist
        fs::create_dir_all(&self.out_dir)
            .map_err(|e| Error::CannotCreate(self.out_dir.clone(), e))?;

        // Compile all the sources and place them in `self.out_dir` the 
        // already configured tool will take care of providing a correct 
        // command
        let mut childs = Vec::new();
        for chunk in self.sources.chunks(4) {
            for source in chunk {
                let mut command = self.tool.to_build_command(&self.include_directories);
                let out_file = self.out_dir.join(
                    Path::new(source.file_name().unwrap()).with_extension("o")
                );

                // TODO: Display each compiled source in some fashion

                let cmd = command.arg(&out_file).arg(source);
                childs.push(cmd.spawn()
                    .map_err(|e| 
                        Error::ProcessCreation(self.tool.path.clone(), e))?);
                
                // The object file was not contained in the `self.objects` add
                // it
                let contained = self.objects.contains(&out_file);
                if !contained {
                    self.objects.push(out_file);
                }

                for child in childs.iter_mut() {
                    if !child.wait()
                        .map_err(|e| Error::ProcessExec(e))?.success() {
                        return Err(Error::CompilationError);
                    }
                }
            }
        }
        
        Ok(self)
    }

    /// Links the objects (if needed) and returns a boolean indicating if it
    /// wasn't needed to link the executable or not
    fn link(&mut self) -> Result<bool, Error> {
        // Generate the path of the existing (or not) executable to generate
        // (or not)
        let exe_path = self.out_dir.join(&self.project_name)
            .with_extension(EXE_EXTENSION);

        // If the executable does not exist (case objects up to date but 
        // executable is not generate) compile it, also in case it was needed
        // to recompile any source
        if !exe_path.is_file() || self.sources.len() != 0 {
            // Link everything into an executable
            let mut command = self.tool
                .to_link_command(exe_path, &self.objects);

            // TODO: Capture output and parse it
            command.status().map_err(|e| 
                Error::ProcessCreation(self.tool.path.clone(), e))?;
        } else {
            return Ok(false);
        }

        Ok(true)
    }
}

fn find_tool() -> Result<(PathBuf, ToolFamily), Error> {
    // Macro that checks if command exists
    macro_rules! exists_command {
        ($command_name:literal) => {
            Command::new($command_name)
                .arg("-v")
                .output().is_ok()
        };
    }

    // Check with priorities, and retrieve the full compiler path and the
    // ToolFamily
    //  * first: clang,
    //  * second: 
    //      Windows -> clang-cl
    //      _ -> Gnu
    //  * third
    //      Windows -> msvc
    if exists_command!("clang") {
        Ok((which::which("clang").unwrap(), ToolFamily::Clang))
    } else if cfg!(target_os = "windows") {
        if exists_command!("clang-cl") {
            Ok((which::which("clang-cl").unwrap(),
                    ToolFamily::Msvc { clang_cl: true }))
        } else if exists_command!("cl") {
            Ok((which::which("cl").unwrap(), 
                    ToolFamily::Msvc { clang_cl: false }))
        } else if exists_command!("gcc") {
            Ok((which::which("gcc").unwrap(), ToolFamily::Gnu))
        } else {
            Err(Error::NoCompilerFound)
        }
    } else if exists_command!("gcc") {
        Ok((which::which("gcc").unwrap(), ToolFamily::Gnu))
    } else {
        Err(Error::NoCompilerFound)
    }
}

/// Configuration used to represent an invocation of a C compiler.
///
/// This can be used to figure out what compiler is in use, what the arguments
/// to it are, and what the environment variables look like for the compiler.
/// This can be used to further configure other build systems (e.g. forward
/// along CC and/or CFLAGS) or the `to_command` method can be used to run the
/// compiler itself.
#[derive(Clone, Debug)]
struct Tool {
    pub path: PathBuf,
    args: Vec<OsString>,
    pub family: ToolFamily,
}

impl Default for Tool {
    fn default() -> Self {
        let (path, family) = find_tool().unwrap();

        Tool {
            path,
            args: Vec::new(),
            family
        }
    }
}

impl Tool {
    /// Instantiates a new tool given the compiler `path`
    fn new(_path: Option<PathBuf>) -> Self { 
        // Extract the compiler family and path
        // TODO: First try to retrieve this from the config file
        let (path, family) = find_tool().unwrap();

        Tool {
            path,
            args: Vec::new(),
            family
        }
    }

    /// Add an argument
    fn push_cc_arg(&mut self, arg: OsString) {
        self.args.push(arg);
    }

    /// Converts this compiler into a `Command` that's ready to build objects
    ///
    /// This is useful for when the compiler needs to be executed and the
    /// command returned will already have the initial arguments and environment
    /// variables configured.
    pub fn to_build_command(
        &self, include_dirs: &Vec<PathBuf>, 
    ) -> Command {
        let include_dirs = include_dirs.iter()
            .map(|p| {
                let mut inc = p.to_str().unwrap().to_string();
                inc.insert_str(0, self.family.include_flag());
                inc
            }).collect::<Vec<String>>();
        let mut cmd = Command::new(&self.path);
        cmd.args(&self.args);
        cmd.args(include_dirs);
        cmd.args(self.family.compilation_flags());
        cmd
    }

    /// Converts this compiler into a `Command` that's ready to link
    ///
    /// TODO: Support linker flags, and check if the warning level affects
    /// here if we are just linking objects
    /// TODO: Support adding external libraries
    pub fn to_link_command(&self,
        exe_path: impl AsRef<Path>, 
        objects: &Vec<PathBuf>
    ) -> Command {
        let f_objects = objects.iter()
            .map(|p| p.to_str().unwrap().to_string()).collect::<Vec<String>>();
        let mut cmd = Command::new(&self.path);
        cmd.args(f_objects);
        cmd.arg(self.family.exe_flag());
        cmd.arg(exe_path.as_ref().to_str().unwrap());
        cmd
    }
}

/// Represents the family of tools this tool belongs to.
///
/// Each family of tools differs in how and what arguments they accept.
///
/// Detection of a family is done on best-effort basis and may not accurately reflect the tool.
#[derive(Copy, Clone, Debug, PartialEq)]
enum ToolFamily {
    /// Tool is GNU Compiler Collection-like.
    Gnu,
    /// Tool is Clang-like. It differs from the GCC in a sense that it accepts superset of flags
    /// and its cross-compilation approach is different.
    Clang,
    /// Tool is the MSVC cl.exe.
    Msvc { clang_cl: bool },
}

impl ToolFamily {
    /// Compilation in debug mode
    fn debug_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-Z7",
            ToolFamily::Gnu | ToolFamily::Clang => "-g"
        }
    }

    /// Compilation in release mode
    fn release_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/O2",
            ToolFamily::Gnu | ToolFamily::Clang => "-O3"
        }
    }

    /// Get the include flags
    fn include_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/I ",
            _ => "-I"
        }
    }

    /// Get the compilation flags variant
    fn compilation_flags(&self) -> &'static [&'static str] {
        match *self {
            ToolFamily::Msvc { .. } => &["/c", "/Fo:"],
            _ => &["-c", "-o"]
        }
    }

    /// Get the flags to generate a executable
    fn exe_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/Fe:", 
            _ => "-o",
        }
    }

    /// What the flags to enable all warnings
    fn warnings_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-W4",
            ToolFamily::Gnu | ToolFamily::Clang => "-Wall",
        }
    }

    /// What the flags to enable extra warnings
    fn extra_warnings_flags(&self) -> Option<&'static str> {
        match *self {
            ToolFamily::Msvc { .. } => None,
            ToolFamily::Gnu | ToolFamily::Clang => Some("-Wextra"),
        }
    }

    /// What the flag to turn warning into errors
    fn warnings_to_errors_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-WX",
            ToolFamily::Gnu | ToolFamily::Clang => "-Werror",
        }
    }
}

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
        .compile()?
        .link()?)
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
                    .subcommand(SubCommand::with_name("clean")
                        .alias("c")
                        .about("Cleans the build targets if the application"))
                    .get_matches();

    // Get the working directory
    let working_dir = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;

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
        let project_name =
            working_dir.components().last().unwrap().as_os_str().to_os_string();
        let it = Instant::now();

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
            .with_extension("exe");

        // Spawn the process of the binary application supplying the arguments
        // passed to `amargo` via `-- <args>...`
        let args = matches.values_of("exe_args")
            .map(|vals| vals.collect::<Vec<_>>())
            .unwrap_or_default();
        Command::new(&executable_path)
            .args(&args)
            .spawn().expect("Couldn't exectute binary application");

        // Print that compilation has started
        println!("{:>12} `{} {}`\n", 
            style("Running").cyan(), 
            executable_path.display(),
            &args[..].join(" "));

    } else if let Some(matches) = matches.subcommand_matches("clean") {
        fs::remove_dir_all(working_dir.join("target"))
            .expect("Cannot remove target dir");
    }

    Ok(())
}
