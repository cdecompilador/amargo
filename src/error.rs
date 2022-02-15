use std::{
    path::PathBuf
};

/// Error type used in the program
/// TODO: Use some macro crate so the displayed message on panic is better
#[derive(Debug)]
pub enum Error {
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


