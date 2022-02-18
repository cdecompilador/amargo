use std::{
    time::SystemTime,
    path::{Path, PathBuf},
    fs,
    collections::VecDeque,
    ffi::{OsString, OsStr},
    process::Command,
};
use crate::{
    EXE_EXTENSION,
    error::Error,
};

use log::info;

/// Any type that can be extracted from a directory in group
trait FromDir: From<(PathBuf, SystemTime)> {
    const EXTS: &'static [&'static str];
    fn new(path: PathBuf, modif: SystemTime) -> Self;

    /// Return a list of `Self` through navigating recursively a directory and 
    /// selecting the ones with extension `EXT`
    fn from_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<Self>, Error> {
        let mut result = Vec::new();

        // Look for existing files in the `dir` dir with extension EXT and add 
        // them to `result`
        for entry in walkdir::WalkDir::new(dir) {
            // DirEntry -> PathBuf
            let path = entry.map_err(Error::FileListing)?.into_path();

            if !path.is_file() || path.extension().is_none() {
                continue;
            }

            // Push the found object files and the last build time
            let extension = path.extension().unwrap();
            if Self::EXTS.contains(&extension.to_str().unwrap()) {
                let modif = path.metadata().unwrap().modified().unwrap();
                result.push(Self::from((path, modif)));
            }
        }

        Ok(result)
    }
}

// Macro to easily implement `FromDir` to a type that just have
// two fields (PathBuf, SystemTime)
macro_rules! impl_from_dir {
    ($ty:ty, $ext:expr) => {
        impl From<(PathBuf, SystemTime)> for $ty {
            fn from(val: (PathBuf, SystemTime)) -> Self {
                Self {
                    path: val.0, modif: val.1
                }
            }
        }
        impl FromDir for $ty {
            const EXTS: &'static [&'static str] = $ext;
            fn new(path: PathBuf, modif: SystemTime) -> Self {
                Self { path, modif }
            }
        }
    };
}

/// A source file *.c, *.cpp or *.cxx
#[derive(Debug, Clone)]
struct Source {
    path:  PathBuf,
    modif: SystemTime,
}
impl_from_dir!(Source, &["c", "cpp", "cxx"]);

/// A header file *.h, *.hpp or *.hxx
#[derive(Debug, Clone)]
struct Header {
    path:  PathBuf,
    modif: SystemTime,
}
impl_from_dir!(Header, &["h", "hpp", "hxx"]);

/// An object file *.o or *.obj
#[derive(Debug, Clone, PartialEq, Eq)]
struct Object {
    path: PathBuf,
    modif: SystemTime
}
impl_from_dir!(Object, &["o", "obj"]);

// Returns the direct dependencies `Vec<Header>` of a `Header` or a `Source`
// given the actual `path` of the file to extract dependencies, the expected
// extensions to find and the list of possible dependencies
// TODO: allow detecting "#include "something.c"
macro_rules! direct_dependencies {
    ($path:expr, $dep_exts:expr, $deps:expr) => {{
        let mut deps = Vec::new();

        // Extract the source data
        let source_data = fs::read_to_string(&$path)
            .map_err(|e| Error::CannotRead($path.clone(), e))?;

        // Find in the source all the #include "<header>"
        // FIXME: Maybe bug in regex but the '^' and '$' doesn't seem to work 
        // very well
        let re = &format!(r#"#include\s*"(?P<dep_name>\w*\.({}))""#, $dep_exts.join("|"));
        let re = regex::Regex::new(re).unwrap();
        let caps = re.captures_iter(&source_data[..]);
        let mut dep_names = caps.map(|cap| cap["dep_name"].to_string())
            .collect::<Vec<String>>();

        // Iterate over all the possible dependencies 
        for (i, dep) in $deps.iter().enumerate() {
            // If an matching `dep.path.filename` is found in the headers listed in
            // the C source, remove it from the vec, as later on if the len of 
            // `headers` != 0 will mean that there were unresolved imports
            dep_names.retain(|dep_name| {
                if dep_name.as_str() == dep.path.file_name().unwrap() {
                    deps.push(i);
                    false
                } else {
                    true
                }
            });
        }

        // Check if all the includes were resolved
        if dep_names.len() != 0 {
            return Err(Error::MissingIncludes($path.clone(), dep_names));
        }

        deps
    }};
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

    /// Locations where to find the headers, needed by the compiler
    header_dirs: Vec<PathBuf>,

    /// The objects needed at linkage
    objects: Vec<Object>,

    /// The sources need to compile, not necesarely all the project sources
    /// just the ones that need to recompile
    sources: Vec<Source>,

    /// The headers found in the include locations
    headers: Vec<Header>,

    /// A graph represented as an adjacency matrix where its rows/columns are
    /// indexed by the indices of the virtual vector `sources` + `headers`
    dependency_graph: Vec<Vec<usize>>,

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

        info!("Selected build tool: {:?}", &build.tool);

        // Create the build target dir if it does not exist
        fs::create_dir_all(&build.out_dir)
            .map_err(|e| Error::CannotCreate(build.out_dir.clone(), e))?;

        // Look for existing object files in the `target/<mode>` dir and add 
        // them to the `Build`
        build.objects = Object::from_dir(&build.out_dir)?;

        // Get last build time retrieving looking at the path of the last build
        // target at `target/<mode>/<project_name>.EXE_EXTENSION`, if the last build
        // time is less than any of the objects delete the target
        let target_path = build.out_dir.join(&build.project_name)
            .with_extension(EXE_EXTENSION);
        if target_path.exists() {
            build.last_time = target_path.metadata().unwrap().modified().ok(); 
            info!("Found target {:?} with last build time {:?} ago", 
                        target_path,
                        build.last_time.unwrap().elapsed().unwrap());
        }

        info!("Found objects at {:?}: {:#?}", build.out_dir, build.objects);

        Ok(build)
    }

    /// Add a directory to lookup sources
    pub fn files<P: AsRef<Path>>(&mut self, files_dir: P) 
            -> Result<&mut Build<'a>, Error> {
        let dir = self.working_dir.join(files_dir);
        self.sources.extend(Source::from_dir(dir)?);

        info!("Added sources: {:#?}", &self.sources);

        Ok(self)
    }
    
    /// Add include dir
    /// TODO: Check if it exists because it can be provided by a config file
    /// in the future
    #[inline]
    pub fn include<P: AsRef<Path>>(&mut self, dir: P) 
            -> Result<&mut Build<'a>, Error> {
        let dir = self.working_dir.join(dir);
        self.header_dirs.push(dir.clone());
        self.headers.extend(Header::from_dir(dir)?);

        info!("Added headers: {:#?}", &self.headers);

        Ok(self)
    }

    /// Compile the sources to objects (if they need to)
    pub fn compile(&mut self) -> Result<&mut Build<'a>, Error> {
        // Just do the incremental compilation if this is not the first build
        if let Some(last_time) = self.last_time {
            // Initialize the dependency_graph full of 0s (falses)
            let size = self.sources.len() + self.headers.len();
            self.dependency_graph = vec![vec![]; size];

            // Fill the adjacency matrix of the graph of dependencies with `Source`s and
            // `Include`s
            for (src_idx, source) in self.sources.iter().enumerate() {
                let mut dep_indices = direct_dependencies!(source.path, 
                    &["h", "hpp", "hxx"],
                    self.headers);
                // FIXME: Dirty fix until #include "name.c" is supported
                dep_indices.iter_mut().for_each(|i| *i += self.sources.len());

                self.dependency_graph[src_idx] = dep_indices;
            }
            for (src_idx, header) in self.headers.iter().enumerate() {
                let src_idx = src_idx + self.sources.len();
                let mut dep_indices = direct_dependencies!(header.path, 
                    &["h", "hpp", "hxx"],
                    self.headers);

                // FIXME: Dirty fix until #include "name.c" is supported
                dep_indices.iter_mut().map(|i| *i += self.sources.len());

                self.dependency_graph[src_idx] = dep_indices;
            }

            info!("Dependency graph: {:?}", self.dependency_graph);

            // Update the sources last modification time traversing the graph (DFS) 
            // for each his dependencies and taking the last time
            let mut visited = vec![false; size];
            for src_idx in 0..self.sources.len() {
                // Mark all vertices as not visited
                visited.fill(false);

                // Create a stack for the DFS
                let mut stack = VecDeque::new();
                stack.push_front(src_idx);

                // Set the track of the max `SystemTime` detected
                let mut old_modif = self.sources[src_idx].modif;

                while stack.len() != 0 {
                    let i = stack.pop_front().unwrap();

                    // Check if this node has already been visited
                    if visited[i] == true { continue; }

                    // Set as visited
                    visited[i] = true;

                    // Update last_time (index `sources` if `0 < i < sources.len()`), 
                    // otherwise access `headers`
                    let child_modif = if i < self.sources.len() {
                        self.sources[i].modif
                    } else {
                        self.headers[i - self.sources.len()].modif
                    };
                    if self.sources[src_idx].modif < child_modif {
                        self.sources[src_idx].modif = child_modif;
                    }

                    // Push the childs of the current node to the stack
                    stack.extend(&self.dependency_graph[i]);
                }

                if old_modif != self.sources[src_idx].modif {
                    info!("New `{:?}` last modif time: {:?}", 
                        self.sources[src_idx].modif.elapsed().unwrap(),
                        old_modif.elapsed().unwrap());
                }
            }

            // Filter from the sources all of them with a modification time lower than
            // the modification time of the last build
            self.sources.retain(|src| src.modif > last_time);
        }

        // Compile all the sources and place them in `self.out_dir` the 
        // already configured tool will take care of providing a correct 
        // command
        // TODO: Compile in parallel according to the avaible threads
        let mut childs = Vec::new();
        for chunk in self.sources.chunks(4) {
            for source in chunk {
                let mut command = self.tool.to_build_command(&self.header_dirs);

                // FIXME: Maybe no need to specify "-o <source_name>.o" to the 
                // compiler
                let out_file = self.out_dir.join(
                    Path::new(source.path.file_name().unwrap()).with_extension("o")
                );

                info!("Compiling {:?}", source);

                let cmd = command.arg(&out_file).arg(&source.path);
                childs.push(cmd.spawn()
                    .map_err(|e| 
                        Error::ProcessCreation(self.tool.path.clone(), e))?);
                
                // Wait for each thread to finish
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
        // Extract all the objects again (but now they should be recompiled)
        self.objects = Object::from_dir(&self.out_dir)?;

        // Generate the path of the existing (or not) executable to generate
        // (or not)
        let exe_path = self.out_dir.join(&self.project_name)
            .with_extension(EXE_EXTENSION);

        // If the executable exist and its up to date do not recompile
        if exe_path.is_file() {
            let exe_path_modif = exe_path.metadata().unwrap().modified().unwrap();
            let objects_max_modif = self.objects.iter().map(|o| o.modif).max().unwrap();
            if exe_path_modif > objects_max_modif {
                return Ok(false);
            }
        }

        info!("Linking {:?}", &exe_path);

        // Link everything into an executable
        let mut command = self.tool
            .to_link_command(exe_path, &self.objects);

        // TODO: Capture output and parse it
        command.status().map_err(|e| 
            Error::ProcessCreation(self.tool.path.clone(), e))?;

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
        objects: &[Object]
    ) -> Command {
        // FIXME: Is really needed to convert to String, shouldn't Command::args accept
        // also a PathBuf?
        let objects = objects.iter()
            .map(|o| o.path.to_str().unwrap().to_string()).collect::<Vec<String>>();
        let mut cmd = Command::new(&self.path);
        cmd.args(objects);
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
