#![warn(rust_2018_idioms)]

use std::{
    fmt::{self, Display, Formatter},
    io,
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

use bitflags::bitflags;

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
    /// Port 1.
    pub port1: Port1,
    /// Port 2.
    pub port2: Port2,
    video_shifter: VideoShifter,
}

impl SpaceInvaders {
    pub fn new<P: AsRef<Path>>(roms: &[P], interrupt_receiver: Receiver<[u8; 3]>) -> Result<Self> {
        Ok(Self {
            i8080: Intel8080::new(roms, 0)?,
            interrupt_receiver,
            port1: Port1::default(),
            port2: Port2::default(),
            video_shifter: VideoShifter::default(),
        })
    }

    /// Returns a shared reference to the framebuffer.
    pub fn framebuffer(&self) -> &[u8] {
        &self.i8080.memory[0x2400..0x4000]
    }

    /// Handles a pending interrupt, if any; otherwise fetches and executes an instruction.
    pub fn update(&mut self) -> u32 {
        match self.interrupt_receiver.try_recv() {
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {
                self.fetch_execute_instruction()
            }
            Ok(instruction) => self.i8080.interrupt(instruction).unwrap_or(0),
        }
    }

    fn fetch_execute_instruction(&mut self) -> u32 {
        let (instruction, states) = self.i8080.fetch_execute_instruction().unwrap();
        match instruction {
            // OUT port
            [0xD3, port, 0] => match port {
                2 => self.video_shifter.offset = self.i8080.cpu.a,
                3 => (),
                4 => self.video_shifter.shift_right(self.i8080.cpu.a),
                5 => (),
                6 => (), // watchdog
                _ => unreachable!(),
            },
            // IN port
            [0xDB, port, 0] => match port {
                1 => self.i8080.cpu.a = self.port1.bits(),
                2 => self.i8080.cpu.a = self.port2.bits(),
                3 => self.i8080.cpu.a = self.video_shifter.into(),
                _ => unreachable!(),
            },
            _ => (),
        }
        states
    }
}

bitflags! {
    /// Port 1, which consists of bit flags.
    pub struct Port1: u8 {
        const COIN = 0b0000_0001;
        const PLAYER_2_START = 0b0000_0010;
        const PLAYER_1_START = 0b0000_0100;
        const ALWAYS_ONE = 0b0000_1000;
        const PLAYER_1_FIRE = 0b0001_0000;
        const PLAYER_1_LEFT = 0b0010_0000;
        const PLAYER_1_RIGHT = 0b0100_0000;
    }
}

impl Default for Port1 {
    fn default() -> Self {
        Port1::ALWAYS_ONE
    }
}

bitflags! {
    /// Port 2, which consists of bit flags.
    #[derive(Default)]
    pub struct Port2: u8 {
        const TILT = 0b0000_0100;
        const PLAYER_2_FIRE = 0b0001_0000;
        const PLAYER_2_LEFT = 0b0010_0000;
        const PLAYER_2_RIGHT = 0b0100_0000;
    }
}

#[derive(Clone, Copy, Default)]
struct VideoShifter {
    register: u16,
    offset: u8,
}

impl VideoShifter {
    fn shift_right(&mut self, fill_byte: u8) {
        self.register = (u16::from(fill_byte) << 8) | (self.register >> 8);
    }
}

impl Into<u8> for VideoShifter {
    fn into(self) -> u8 {
        (self.register >> (8 - self.offset)) as u8
    }
}
