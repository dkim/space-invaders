#![warn(rust_2018_idioms)]

use std::{
    fmt::{self, Display, Formatter},
    io,
    path::Path,
};

use i8080::Intel8080;

/// An error that can occur in this crate.
#[derive(Debug)]
pub enum Error {
    /// An error from crate `i8080`.
    I8080 { source: i8080::Error },
    /// An I/O error.
    Io { source: io::Error },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::I8080 { source } => source.fmt(f),
            Error::Io { source } => source.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::I8080 { source } => Some(source),
            Error::Io { source } => Some(source),
        }
    }
}

impl From<i8080::Error> for Error {
    fn from(e: i8080::Error) -> Self {
        Error::I8080 { source: e }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io { source: e }
    }
}

/// A specialized `std::result::Result` type for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// A Space Invaders arcade machine.
pub struct SpaceInvaders {
    /// The Intel 8080 CPU.
    pub i8080: Intel8080,
}

impl SpaceInvaders {
    pub fn new<P: AsRef<Path>>(roms: &[P]) -> Result<Self> {
        Ok(Self { i8080: Intel8080::new(roms, 0)? })
    }
}
