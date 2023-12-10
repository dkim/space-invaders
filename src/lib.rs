#![warn(rust_2018_idioms)]

use std::{
    fmt::{self, Display, Formatter},
    fs,
    io::{self, Cursor},
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

use bitflags::bitflags;

use log::warn;

use rodio::{Decoder, OutputStreamHandle, Sink, Source};

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
    port3: Port3,
    port5: Port5,
    video_shifter: VideoShifter,
    samples: Samples,
}

impl SpaceInvaders {
    /// Constructs a new `SpaceInvaders`.
    ///
    /// # Arguments
    ///
    /// * `roms` - a reference to a slice of paths to ROMs to be loaded starting at address 0.
    /// * `samples` - an optional array of paths to 9 audio samples.
    /// * `audio_stream_handle` - an optional reference to OutputStreamHandle,
    /// * `interrupt_receiver` - a `std::sync::mpsc::Receiver` to receive interrupts from.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::sync::mpsc;
    /// use rodio::OutputStream;
    /// use space_invaders::SpaceInvaders;
    ///
    /// let (_audio_stream, audio_stream_handle) = OutputStream::try_default()?;
    /// let (interrupt_sender, interrupt_receiver) = mpsc::sync_channel(0);
    /// let space_invaders = SpaceInvaders::new(
    ///     &["invaders.h", "invaders.g", "invaders.f", "invaders.e"],
    ///     Some(["1.wav", "2.wav", "3.wav", "4.wav", "5.wav", "6.wav", "7.wav", "8.wav", "9.wav"]),
    ///     Some(&audio_stream_handle),
    ///     interrupt_receiver,
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new<P: AsRef<Path>, Q: AsRef<Path>>(
        roms: &[P],
        samples: Option<[Q; 9]>,
        audio_stream_handle: Option<&OutputStreamHandle>,
        interrupt_receiver: Receiver<[u8; 3]>,
    ) -> Result<Self> {
        let samples = Samples::new(audio_stream_handle, samples);
        Ok(Self {
            i8080: Intel8080::new(roms, 0)?,
            interrupt_receiver,
            port1: Port1::default(),
            port2: Port2::default(),
            port3: Port3::default(),
            port5: Port5::default(),
            video_shifter: VideoShifter::default(),
            samples,
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
                3 => {
                    // from_bits_unchecked() is used instead of from_bits() because the
                    // functionalities of some bits of port 3 are not clear and they are ignored
                    // for now.
                    let port3 = unsafe { Port3::from_bits_unchecked(self.i8080.cpu.a) };
                    if let Some((wav, sink)) = &self.samples.ufo_low_pitch {
                        if port3.contains(Port3::UFO_LOW_PITCH) {
                            if !self.port3.contains(Port3::UFO_LOW_PITCH) {
                                match Decoder::new(Cursor::new(wav.clone())) {
                                    Ok(source) => sink.append(source.repeat_infinite()),
                                    Err(err) => warn!("{:?}", err),
                                }
                            }
                        } else if self.port3.contains(Port3::UFO_LOW_PITCH) {
                            sink.stop();
                        }
                    }
                    for (audio, bit) in &mut [
                        (&self.samples.shoot, Port3::SHOOT),
                        (&self.samples.explosion, Port3::EXPLOSION),
                        (&self.samples.invader_killed, Port3::INVADER_KILLED),
                    ] {
                        if let Some((wav, sink)) = audio {
                            if port3.contains(*bit) && !self.port3.contains(*bit) {
                                match Decoder::new(Cursor::new(wav.clone())) {
                                    Ok(source) => sink.append(source),
                                    Err(err) => warn!("{:?}", err),
                                }
                            }
                        }
                    }
                    self.port3 = port3;
                }
                4 => self.video_shifter.shift_right(self.i8080.cpu.a),
                5 => {
                    let port5 = Port5::from_bits(self.i8080.cpu.a).unwrap();
                    for (audio, bit) in &mut [
                        (&self.samples.fast_invader_1, Port5::FAST_INVADER_1),
                        (&self.samples.fast_invader_2, Port5::FAST_INVADER_2),
                        (&self.samples.fast_invader_3, Port5::FAST_INVADER_3),
                        (&self.samples.fast_invader_4, Port5::FAST_INVADER_4),
                        (&self.samples.ufo_high_pitch, Port5::UFO_HIGH_PITCH),
                    ] {
                        if let Some((wav, sink)) = audio {
                            if port5.contains(*bit) && !self.port5.contains(*bit) {
                                match Decoder::new(Cursor::new(wav.clone())) {
                                    Ok(source) => sink.append(source),
                                    Err(err) => warn!("{:?}", err),
                                }
                            }
                        }
                    }
                    self.port5 = port5;
                }
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
        const EXTRA_LIFE_AT = 0b0000_1000;
        const PLAYER_2_FIRE = 0b0001_0000;
        const PLAYER_2_LEFT = 0b0010_0000;
        const PLAYER_2_RIGHT = 0b0100_0000;
        const PRICING_DISPLAY = 0b1000_0000;
    }
}

bitflags! {
    // Some bits of port 3 are missing here because their functionalities are not clear.
    #[derive(Default)]
    struct Port3: u8 {
        const UFO_LOW_PITCH = 0b0000_0001;
        const SHOOT = 0b0000_0010;
        const EXPLOSION = 0b0000_0100;
        const INVADER_KILLED = 0b0000_1000;
    }
}

bitflags! {
    #[derive(Default)]
    struct Port5: u8 {
        const FAST_INVADER_1 = 0b0000_0001;
        const FAST_INVADER_2 = 0b0000_0010;
        const FAST_INVADER_3 = 0b0000_0100;
        const FAST_INVADER_4 = 0b0000_1000;
        const UFO_HIGH_PITCH = 0b0001_0000;
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

impl From<VideoShifter> for u8 {
    fn from(video_shifter: VideoShifter) -> Self {
        (video_shifter.register >> (8 - video_shifter.offset)) as u8
    }
}

struct Samples {
    ufo_low_pitch: Option<(Vec<u8>, Sink)>,
    shoot: Option<(Vec<u8>, Sink)>,
    explosion: Option<(Vec<u8>, Sink)>,
    invader_killed: Option<(Vec<u8>, Sink)>,
    fast_invader_1: Option<(Vec<u8>, Sink)>,
    fast_invader_2: Option<(Vec<u8>, Sink)>,
    fast_invader_3: Option<(Vec<u8>, Sink)>,
    fast_invader_4: Option<(Vec<u8>, Sink)>,
    ufo_high_pitch: Option<(Vec<u8>, Sink)>,
}

impl Samples {
    fn new<P: AsRef<Path>>(
        audio_stream_handle: Option<&OutputStreamHandle>,
        samples: Option<[P; 9]>,
    ) -> Self {
        let mut ufo_low_pitch = None;
        let mut shoot = None;
        let mut explosion = None;
        let mut invader_killed = None;
        let mut fast_invader_1 = None;
        let mut fast_invader_2 = None;
        let mut fast_invader_3 = None;
        let mut fast_invader_4 = None;
        let mut ufo_high_pitch = None;
        if let Some(samples) = samples {
            if let Some(audio_stream_handle) = audio_stream_handle {
                for (path, audio) in &mut [
                    (&samples[0], &mut ufo_high_pitch),
                    (&samples[1], &mut shoot),
                    (&samples[2], &mut explosion),
                    (&samples[3], &mut invader_killed),
                    (&samples[4], &mut fast_invader_1),
                    (&samples[5], &mut fast_invader_2),
                    (&samples[6], &mut fast_invader_3),
                    (&samples[7], &mut fast_invader_4),
                    (&samples[8], &mut ufo_low_pitch),
                ] {
                    let path = path.as_ref();
                    match fs::read(path) {
                        Ok(wav) => match Sink::try_new(audio_stream_handle) {
                            Ok(sink) => **audio = Some((wav, sink)),
                            Err(err) => warn!("{:?}", err),
                        },
                        Err(err) => {
                            if let io::ErrorKind::NotFound = err.kind() {
                                warn!("{:?}: '{}'", err, path.display());
                            } else {
                                warn!("{:?}", err);
                            }
                        }
                    }
                }
            }
        }
        Self {
            ufo_low_pitch,
            shoot,
            explosion,
            invader_killed,
            fast_invader_1,
            fast_invader_2,
            fast_invader_3,
            fast_invader_4,
            ufo_high_pitch,
        }
    }
}
