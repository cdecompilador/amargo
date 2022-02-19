use std::{
    path::{Path, PathBuf},
    process::Command,
    ffi::OsString,
};

use crate::{
    error::*,
    build::Object,
};

/// Find an avaible tool on the system
/// TODO: On windows try to put mscv on the environment first
fn find_tool() -> Result<(PathBuf, ToolFamily)> {
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
pub(crate) struct Tool {
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
    pub fn new() -> Self { 
        // Extract the compiler family and path
        // TODO: First try to retrieve this from the config file
        let (path, family) = find_tool().unwrap();

        Tool {
            path,
            args: Vec::new(),
            family
        }
    }

    /// Add an arbitrary argument
    pub fn push_cc_arg(&mut self, arg: OsString) {
        self.args.push(arg);
    }

    /// Converts this compiler into a `Command` that's ready to build objects
    ///
    /// This is useful for when the compiler needs to be executed and the
    /// command returned will already have the initial arguments and environment
    /// variables configured.
    pub fn to_build_command(
        &self, include_dirs: &[PathBuf], 
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
pub(crate) enum ToolFamily {
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
    pub fn debug_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-Z7",
            ToolFamily::Gnu | ToolFamily::Clang => "-g"
        }
    }

    /// Compilation in release mode
    pub fn release_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/O2",
            ToolFamily::Gnu | ToolFamily::Clang => "-O3"
        }
    }

    /// Get the include flags
    pub fn include_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/I ",
            _ => "-I"
        }
    }

    /// Get the compilation flags variant
    pub fn compilation_flags(&self) -> &'static [&'static str] {
        match *self {
            ToolFamily::Msvc { .. } => &["/c", "/Fo:"],
            _ => &["-c", "-o"]
        }
    }

    /// Get the flags to generate a executable
    pub fn exe_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "/Fe:", 
            _ => "-o",
        }
    }

    /// What the flags to enable all warnings
    pub fn warnings_flags(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-W4",
            ToolFamily::Gnu | ToolFamily::Clang => "-Wall",
        }
    }

    /// What the flags to enable extra warnings
    pub fn extra_warnings_flags(&self) -> Option<&'static str> {
        match *self {
            ToolFamily::Msvc { .. } => None,
            ToolFamily::Gnu | ToolFamily::Clang => Some("-Wextra"),
        }
    }

    /// What the flag to turn warning into errors
    pub fn warnings_to_errors_flag(&self) -> &'static str {
        match *self {
            ToolFamily::Msvc { .. } => "-WX",
            ToolFamily::Gnu | ToolFamily::Clang => "-Werror",
        }
    }
}
