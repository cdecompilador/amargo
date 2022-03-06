//! Contains all the related

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    config::{BuildType, ProjectConfig},
    error::*,
    tool::Tool,
    EXE_EXTENSION,
};

use log::info;

/// Any type that can be extracted from a directory in group
trait FromDir: From<(PathBuf, SystemTime)> {
    const EXTS: &'static [&'static str];

    /// Return a list of `Self` through navigating recursively a directory and
    /// selecting the ones with extension `EXT`
    fn from_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<Self>> {
        let mut result = Vec::new();

        // Check that the path exists
        if !dir.as_ref().is_dir() {
            return Err(Error::DirNotExist(dir.as_ref().to_path_buf()));
        }

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
                    path: val.0,
                    modif: val.1,
                }
            }
        }
        impl FromDir for $ty {
            const EXTS: &'static [&'static str] = $ext;
        }
    };
}

/// A source file *.c, *.cpp or *.cxx
#[derive(Debug, Clone)]
pub(crate) struct Source {
    path: PathBuf,
    modif: SystemTime,
}
impl_from_dir!(Source, &["c", "cpp", "cxx"]);

/// A header file *.h, *.hpp or *.hxx
#[derive(Debug, Clone)]
pub(crate) struct Header {
    path: PathBuf,
    modif: SystemTime,
}
impl_from_dir!(Header, &["h", "hpp", "hxx"]);

/// An object file *.o or *.obj
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    pub path: PathBuf,
    modif: SystemTime,
}
impl_from_dir!(Object, &["o", "obj"]);

// Returns the direct dependencies `Vec<Header>` of a `Header` or a `Source`
// given the actual `path` of the file to extract dependencies, the expected
// extensions to find and the list of possible dependencies
//
// TODO: allow detecting "#include "something.c"
macro_rules! direct_dependencies {
    ($path:expr, $dep_exts:expr, $deps:expr) => {{
        let mut deps = Vec::new();

        // Extract the source data
        let source_data = fs::read_to_string(&$path)
            .map_err(|e| Error::CannotRead($path.clone(), e))?;

        // Find in the source all the #include "<header>"
        // FIXME: Maybe bug in regex but the '^' and '$' doesn't seem to work
        // very well (and they are mandatory unless I use a full custom c
        // parser)
        let re = &format!(
            r#"#include\s*"(?P<dep_name>\w*\.({}))""#,
            $dep_exts.join("|")
        );
        let re = regex::Regex::new(re).unwrap();
        let caps = re.captures_iter(&source_data[..]);
        let mut dep_names = caps
            .map(|cap| cap["dep_name"].to_string())
            .collect::<Vec<String>>();

        // Iterate over all the possible dependencies
        for (i, dep) in $deps.iter().enumerate() {
            // If an matching `dep.path.filename` is found in the headers listed
            // in the C source, remove it from the vec, as later on
            // if the len of `headers` != 0 will mean that there
            // were unresolved imports
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

/// This let us build given a config a project
#[derive(Clone)]
pub struct Build<'a> {
    /// Configs of the project extracted from the cli and the `Amargo.toml`
    /// config file
    config: &'a ProjectConfig,

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

    /// Contains the last build time of the last target (if exists)
    last_time: Option<SystemTime>,

    /// The tool used for compilation, abstraction over the compiler, this
    /// in the future must encompass linker, assembler and even external tools
    /// to reduce binary/library size
    tool: Tool,

    /// The directory where to put the target (influenced by the `mode`)
    out_dir: PathBuf,
}

impl<'a> Build<'a> {
    /// Construct a new instance of a blank set of configurations
    pub fn new(
        config: &'a ProjectConfig,
        mode: BuildType,
    ) -> Result<Build<'a>> {
        let project_name = &config.config.as_ref().unwrap().project.name;

        // Push compiler args depending on the `mode`
        let mut tool = Tool::default();
        tool.push_cc_arg(tool.family.warnings_flags().into());
        if mode == BuildType::Debug {
            tool.push_cc_arg(tool.family.debug_flags().into());
        } else {
            tool.push_cc_arg(tool.family.release_flags().into());
        }

        // Create the "default" `Build` struct
        //
        // TODO: check if ..Default::default() works
        let mut build = Build {
            config,
            header_dirs: Vec::new(),
            objects: Vec::new(),
            sources: Vec::new(),
            headers: Vec::new(),
            dependency_graph: Vec::new(),
            last_time: None,
            tool,
            out_dir: mode.into(),
        };

        info!("Selected build tool: {:?}", &build.tool);

        // Create the build target dir if it does not exist
        fs::create_dir_all(&build.out_dir)
            .map_err(|e| Error::CannotCreate(build.out_dir.clone(), e))?;

        // Look for existing object files in the `target/<mode>` dir and add
        // them to the `Build`
        build.objects = Object::from_dir(&build.out_dir)?;

        // Get last build time retrieving looking at the path of the last build
        // target at `target/<mode>/<project_name>.EXE_EXTENSION`, if the last
        // build time is less than any of the objects delete the target
        let target_path = build
            .out_dir
            .join(project_name)
            .with_extension(EXE_EXTENSION);
        if target_path.exists() {
            build.last_time = target_path.metadata().unwrap().modified().ok();
            info!(
                "Found target {:?} with last build time {:?} ago",
                target_path,
                build.last_time.unwrap().elapsed().unwrap()
            );
        }

        info!("Found objects at {:?}: {:#?}", build.out_dir, build.objects);

        Ok(build)
    }

    /// Add a directory to lookup sources
    #[inline]
    pub fn files<P: AsRef<Path>>(
        &mut self,
        files_dir: P,
    ) -> Result<&mut Build<'a>> {
        let dir = self.config.working_dir.join(files_dir);
        self.sources.extend(Source::from_dir(dir)?);

        info!("Added sources: {:#?}", &self.sources);

        Ok(self)
    }

    /// Add include dir
    #[inline]
    pub fn include<P: AsRef<Path>>(
        &mut self,
        dir: P,
    ) -> Result<&mut Build<'a>> {
        let dir = self.config.working_dir.join(dir);
        self.header_dirs.push(dir.clone());
        self.headers.extend(Header::from_dir(dir)?);

        info!("Added headers: {:#?}", &self.headers);

        Ok(self)
    }

    /// Compile the sources to objects (if they need to)
    pub fn compile(&mut self) -> Result<&mut Build<'a>> {
        // Just do the incremental compilation if this is not the first build
        //
        // Representing the dependencies as a graph and updating the source
        // `.modif` to the bigger `.modif` of him within his
        // dependencies, then sorting the sources thet need compilation
        if let Some(last_time) = self.last_time {
            // Initialize the dependency_graph full of 0s (falses)
            let size = self.sources.len() + self.headers.len();
            self.dependency_graph = vec![vec![]; size];

            // Fill the adjacency matrix of the graph of dependencies with
            // `Source`s and `Include`s
            for (src_idx, source) in self.sources.iter().enumerate() {
                let mut dep_indices = direct_dependencies!(
                    source.path,
                    &["h", "hpp", "hxx"],
                    self.headers
                );
                // FIXME: Dirty fix until #include "name.c" is supported
                dep_indices
                    .iter_mut()
                    .for_each(|i| *i += self.sources.len());

                self.dependency_graph[src_idx] = dep_indices;
            }
            for (src_idx, header) in self.headers.iter().enumerate() {
                let src_idx = src_idx + self.sources.len();
                let mut dep_indices = direct_dependencies!(
                    header.path,
                    &["h", "hpp", "hxx"],
                    self.headers
                );

                // FIXME: Dirty fix until #include "name.c" is supported
                dep_indices
                    .iter_mut()
                    .for_each(|i| *i += self.sources.len());

                self.dependency_graph[src_idx] = dep_indices;
            }

            info!("Dependency graph: {:?}", self.dependency_graph);

            // Update the sources last modification time traversing the graph
            // (DFS) for each his dependencies and taking the last
            // time
            let mut visited = vec![false; size];
            for src_idx in 0..self.sources.len() {
                // Mark all vertices as not visited
                visited.fill(false);

                // Create a stack for the DFS
                let mut stack = VecDeque::new();
                stack.push_front(src_idx);

                // Set the track of the max `SystemTime` detected
                let old_modif = self.sources[src_idx].modif;

                while !stack.is_empty() {
                    let i = stack.pop_front().unwrap();

                    // Check if this node has already been visited
                    if visited[i] {
                        continue;
                    }

                    // Set as visited
                    visited[i] = true;

                    // Update last_time (index `sources` if `0 < i <
                    // sources.len()`), otherwise access
                    // `headers`
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
                    info!(
                        "New `{:?}` last modif time: {:?}",
                        self.sources[src_idx].modif.elapsed().unwrap(),
                        old_modif.elapsed().unwrap()
                    );
                }
            }

            // Filter from the sources all of them with a modification time
            // lower than the modification time of the last build
            self.sources.retain(|src| src.modif > last_time);
        }

        // Compile all the sources and place them in `self.out_dir` the
        // already configured tool will take care of providing a correct
        // command
        //
        // TODO: Compile in parallel according to the avaible threads
        let mut childs = Vec::new();
        for chunk in self.sources.chunks(4) {
            for source in chunk {
                let mut command = self.tool.to_build_command(&self.header_dirs);

                // FIXME: Maybe no need to specify "-o <source_name>.o" to the
                // compiler
                let out_file = self.out_dir.join(
                    Path::new(source.path.file_name().unwrap())
                        .with_extension("o"),
                );

                info!("Compiling {:?}", source);

                let cmd = command.arg(&out_file).arg(&source.path);
                childs.push(cmd.spawn().map_err(|e| {
                    Error::ProcessCreation(self.tool.path.clone(), e)
                })?);

                // Wait for each thread to finish
                for child in childs.iter_mut() {
                    if !child.wait().map_err(Error::ProcessExec)?.success() {
                        return Err(Error::Compilation);
                    }
                }
            }
        }

        Ok(self)
    }

    /// Links the objects (if needed) and returns a boolean indicating if it
    /// wasn't needed to link the executable or not
    pub fn link(&mut self) -> Result<bool> {
        let project_name = &self.config.config.as_ref().unwrap().project.name;

        // Extract all the objects again (but now they should be recompiled)
        self.objects = Object::from_dir(&self.out_dir)?;

        // Generate the path of the existing (or not) target to generate
        let target_path = self
            .out_dir
            .join(project_name)
            .with_extension(EXE_EXTENSION);

        // If the executable exist and its up to date do not recompile
        if target_path.is_file() {
            let target_path_modif =
                target_path.metadata().unwrap().modified().unwrap();
            let objects_max_modif =
                self.objects.iter().map(|o| o.modif).max().unwrap();
            if target_path_modif > objects_max_modif {
                return Ok(false);
            }
        }

        info!("Linking {:?}", &target_path);

        // Link everything into an executable
        //
        // TODO: Capture output and parse it
        let mut command = self.tool.to_link_command(target_path, &self.objects);
        command
            .status()
            .map_err(|e| Error::ProcessCreation(self.tool.path.clone(), e))?;

        Ok(true)
    }
}
