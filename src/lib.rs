#![warn(rust_2018_idioms)]

use std::{
    fmt::{self, Display, Formatter},
    io,
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
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

/// The width of the screen of the Space Invaders arcade machine.
pub const SCREEN_WIDTH: u32 = 224;
/// The height of the screen of the Space Invaders arcade machine.
pub const SCREEN_HEIGHT: u32 = 256;

/// A Space Invaders arcade machine.
pub struct SpaceInvaders {
    /// The Intel 8080 CPU.
    pub i8080: Intel8080,
    interrupt_receiver: Receiver<[u8; 3]>,
}

impl SpaceInvaders {
    pub fn new<P: AsRef<Path>>(roms: &[P], interrupt_receiver: Receiver<[u8; 3]>) -> Result<Self> {
        Ok(Self { i8080: Intel8080::new(roms, 0)?, interrupt_receiver })
    }

    /// Returns a shared reference to the framebuffer.
    pub fn framebuffer(&self) -> &[u8] {
        &self.i8080.memory[0x2400..0x4000]
    }

    /// Handles a pending interrupt, if any; otherwise fetches and executes an instruction.
    pub fn update(&mut self) -> u32 {
        match self.interrupt_receiver.try_recv() {
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {
                let (_instruction, states) = self.i8080.fetch_execute_instruction().unwrap();
                states
            }
            Ok(instruction) => self.i8080.interrupt(instruction).unwrap_or(0),
        }
    }
}
