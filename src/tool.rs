use std::{
    time::SystemTime,
    path::{Path, PathBuf},
    fs,
    ffi::{OsString, OsStr},
    process::Command,
};
use crate::{
    EXE_EXTENSION,
    error::Error,
};

/// All the includes that a given sources have, it needs to be extracted from 
/// the source file via parsing it
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

        // Find in the source all the #include "<header>"  
        let re = regex::Regex::new(r#"^#include\s+"(?P<header>\w*\.h)"$"#)
            .unwrap();
        let caps = re.captures_iter(&source_data[..]);
        let mut headers = caps.map(|cap| cap["header"].to_string())
            .collect::<Vec<String>>();

        // Lookup inside the `include_dirs` and add the path to the includes
        for include_dir in include_dirs {
            // Iterate every entry in the `include_dir`, check if its a header
            // and if it is, check if matches the name of the headers that the
            // source imports
            for entry in walkdir::WalkDir::new(include_dir) {
                // DirEntry -> PathBuf
                let path = entry.map_err(Error::FileListing)?.into_path();

                // Skip entry folders
                if !path.is_file() {
                    continue;
                }

                let path_filename = path.file_name().unwrap().to_str().unwrap();

                // If an matching `path_filename` is found in the headers listed in
                // the C source, remove it from the vec, as later on if the len of 
                // `headers` != 0 will mean that there were unresolved imports
                headers.retain(|header| {
                    if header == path_filename {
                        imports.includes.push(path.clone());
                        false
                    } else {
                        true
                    }
                });
            }
        }

        // Check if all the includes were resolved
        if headers.len() != 0 {
            return Err(Error::MissingIncludes(imports.source, headers));
        }

        Ok(imports)
    }

    /// Returns the last time a file and all its imports got a modification
    /// TODO: Compute this before
    fn last_modified(&self) -> SystemTime {
        let mut last = self.source.metadata().unwrap().modified().unwrap();
        for include in &self.includes {
            let inc_time = include.metadata().unwrap().modified().unwrap();
            if inc_time > last {
                last = inc_time;
            }
        }

        last
    }
}

/// An abstraction around the build process of a crate, in the future this
/// might be just a trait with a check and build method to allow different 
/// types of crates
#[derive(Clone, Debug, Default)]
pub struct Build<'a> {
    /// The project name extracted from the working directory
    ///
    /// TODO: Extract it from the config file
    project_name: OsString,

    /// The working directory of this build
    working_dir: PathBuf,

    /// The include dirs for .h
    include_directories: Vec<PathBuf>,

    /// The objects needed at linkage
    objects: Vec<PathBuf>,

    /// The sources need to compile, not necesarely all the project sources
    /// just the ones that need to recompile
    sources: Vec<PathBuf>,

    /// Contains the last build time (if exists)
    last_time: Option<SystemTime>,

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
    pub fn new(mode: &'a str) -> Result<Build<'a>, Error> {
        // Get the working dir
        let working_dir = std::env::current_dir()
            .and_then(|d| d.canonicalize())
            .map_err(|e| Error::CurrentDirInvalid(PathBuf::from("."), e))?;

        // Create the "default" `Build` struct
        let mut build = Build {
            project_name: working_dir.components().last().unwrap().as_os_str()
                .to_owned(),
            mode,
            working_dir,
            out_dir: Path::new("target").join(mode),
            ..Default::default()
        };

        // Push compiler args depending on the `mode`
        let family = build.tool.family;
        build.tool.push_cc_arg(family.warnings_flags().into());
        if mode == "debug" {
            build.tool.push_cc_arg(family.debug_flags().into());
        } else if mode == "release" {
            build.tool.push_cc_arg(family.release_flags().into());
        }

        // Look for existing object files in the `target/<mode>` dir and add 
        // them to the `Build`, also update the last build time
        if build.out_dir.is_dir() {
            for entry in walkdir::WalkDir::new(&build.out_dir) {
                // DirEntry -> PathBuf
                let path = entry.map_err(Error::FileListing)?.into_path();

                if !path.is_file() {
                    continue
                }

                // Push the found object files and the last build time
                if path.extension().unwrap_or(OsStr::new("")) == "o" {
                    build.objects.push(path.clone());
                } if path.extension().unwrap_or(OsStr::new("")) == EXE_EXTENSION {
                    build.last_time = path.metadata().unwrap().modified().ok();
                }
            }
        }

        Ok(build)
    }

    /// Add a directory to lookup sources
    pub fn files<P: AsRef<Path>>(&mut self, files_dir: P) 
            -> Result<&mut Build<'a>, Error> {
        // Retrieve all the source recursively
        for entry in walkdir::WalkDir::new(self.working_dir.join(files_dir)) {
            // DirEntry -> PathBuf
            let path = entry.map_err(Error::FileListing)?.into_path();

            // Just push the source files
            if path.is_file() && path.extension().unwrap() == "c" {
                // Strip the prefix of the path to avoid verbose errors
                self.sources.push(
                    path.strip_prefix(&self.working_dir).unwrap().to_path_buf());
            }
        }
        Ok(self)
    }
    
    /// Add include dir
    /// TODO: Check if it exists because it can be provided by a config file
    /// in the future
    #[inline]
    pub fn include<P: AsRef<Path>>(&mut self, dir: P) -> &mut Build<'a> {
        self.include_directories.push(dir.as_ref().to_path_buf());
        self
    }

    /// Add an arbitrary object file to link in
    #[inline]
    pub fn object<P: AsRef<Path>>(&mut self, obj: P) -> &mut Build<'a> {
        self.objects.push(obj.as_ref().to_path_buf());
        self
    }

    /// Compile the sources to objects (if they need to)
    pub fn compile(&mut self) -> Result<&mut Build<'a>, Error> {
        // Create the build target dir if it does not exist
        fs::create_dir_all(&self.out_dir)
            .map_err(|e| Error::CannotCreate(self.out_dir.clone(), e))?;

        // Just do the incremental compilation of this is not the first build
        if self.last_time.is_some() {
            // Pair every source with it's object file (or None if this does 
            // not exist)
            // TODO: Use a boolean instead of an object
            let mut pairs = Vec::new();
            for source in self.sources.drain(..) {
                for object in &self.objects {
                    let source_stem = source.file_stem().unwrap();
                    let source_time = source.metadata().unwrap().modified().unwrap();
                    let object_stem = object.file_stem().unwrap();

                    if source_stem == object_stem {
                        pairs.push(((source.clone(), source_time), Some(object)));
                        continue;
                    }

                    pairs.push(((source.clone(), source_time), None));
                }
            }

            // Change every source last modification time to the latest modification time 
            // of its included headers
            for ((source, mut source_time), _) in &pairs {
                let total_source_time = 
                    Imports::from(source.clone(), 
                                  self.include_directories.clone())?.last_modified();
                if total_source_time > source_time {
                    source_time = total_source_time;
                }
            }

            // Remove all the sources with a `last_modification` > `executable_last_build`
            // that have already an object file and set the new correct `self.sources`
            self.sources = pairs.into_iter()
                    .filter_map(|((source, source_time), object)| {
                if object.is_some() && source_time > self.last_time.unwrap() {
                    None
                } else {
                    Some(source)
                }
            }).collect();
        }

        // Compile all the sources and place them in `self.out_dir` the 
        // already configured tool will take care of providing a correct 
        // command
        // TODO: Compile in parallel according to the avaible threads
        let mut childs = Vec::new();
        for chunk in self.sources.chunks(4) {
            for source in chunk {
                let mut command = self.tool.to_build_command(&self.include_directories);
                let out_file = self.out_dir.join(
                    Path::new(source.file_name().unwrap()).with_extension("o")
                );

                // TODO: Display each compiled source in some fashion (logging)

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
    pub fn link(&mut self) -> Result<bool, Error> {
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

/// Find an avaible tool on the system
/// TODO: On windows try to put mscv on the environment first
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

/// Configuration used to represent an invocation of a C compiler (or another tool).
///
/// This can be used to figure out what compiler is in use, what the arguments
/// to it are, and what the environment variables look like for the compiler.
/// This can be used to further configure other build systems (e.g. forward
/// along CC and/or CFLAGS) or the `to_command` method can be used to run the
/// compiler itself.
#[derive(Clone, Debug)]
struct Tool {
    /// Path to the compiler source
    pub path: PathBuf,

    /// Arguments added
    args: Vec<OsString>,

    /// Specifies the family, needed as some flags differ between compiler families
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
/// Detection of a family is done on best-effort basis and may not accurately reflect 
/// the tool.
#[derive(Copy, Clone, Debug, PartialEq)]
enum ToolFamily {
    /// Tool is GNU Compiler Collection-like.
    Gnu,

    /// Tool is Clang-like. It differs from the GCC in a sense that it accepts 
    /// superset of flags
    /// and its cross-compilation approach is different.
    Clang,

    /// Tool is the MSVC cl.exe. (or the clang one with command signature equal to cl.exe)
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
