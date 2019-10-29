#![warn(rust_2018_idioms)]

use std::{
    fmt::{self, Display, Formatter},
    mem::MaybeUninit,
    path::PathBuf,
    process,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
};

use luminance::{
    context::GraphicsContext,
    framebuffer::Framebuffer,
    pipeline::{BoundTexture, PipelineState},
    pixel::{NormR8UI, NormUnsigned, Pixel},
    render_state::RenderState,
    shader::program::{BuiltProgram, Program, ProgramError, Uniform},
    tess::{Mode, Tess, TessBuilder, TessError},
    texture::{Dim2, GenMipmaps, Sampler, Texture, TextureError},
};
use luminance_derive::UniformInterface;
use luminance_glfw::{GlfwSurface, GlfwSurfaceError, Surface, WindowDim, WindowEvent, WindowOpt};

use spin_sleep::LoopHelper;

use structopt::StructOpt;

use space_invaders::SpaceInvaders;

#[derive(Debug)]
enum Error {
    GlfwSurface(GlfwSurfaceError),
    Program(ProgramError),
    SpaceInvaders { source: space_invaders::Error },
    Tess(TessError),
    Texture(TextureError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::GlfwSurface(e) => e.fmt(f),
            Error::Program(e) => e.fmt(f),
            Error::SpaceInvaders { source } => source.fmt(f),
            Error::Tess(e) => write!(f, "{:?}", e),
            Error::Texture(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::GlfwSurface(_) => None,
            Error::Program(_) => None,
            Error::SpaceInvaders { source } => Some(source),
            Error::Tess(_) => None,
            Error::Texture(_) => None,
        }
    }
}

impl From<GlfwSurfaceError> for Error {
    fn from(e: GlfwSurfaceError) -> Self {
        Error::GlfwSurface(e)
    }
}

impl From<ProgramError> for Error {
    fn from(e: ProgramError) -> Self {
        Error::Program(e)
    }
}

impl From<space_invaders::Error> for Error {
    fn from(source: space_invaders::Error) -> Self {
        Error::SpaceInvaders { source }
    }
}

impl From<TessError> for Error {
    fn from(e: TessError) -> Self {
        Error::Tess(e)
    }
}

impl From<TextureError> for Error {
    fn from(e: TextureError) -> Self {
        Error::Texture(e)
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, StructOpt)]
#[structopt(about)]
struct Opt {
    /// A directory that contains invaders.{e,f,g,h}
    #[structopt(parse(from_os_str))]
    roms: PathBuf,
}

#[derive(UniformInterface)]
struct Uniforms {
    sampler: Uniform<&'static BoundTexture<'static, Dim2, NormUnsigned>>,
}

const VERTEX_SHADER: &str = include_str!("vertex.vert");
const FRAGMENT_SHADER: &str = include_str!("fragment.frag");

const FRAMEBUFFER_LEN: usize =
    space_invaders::SCREEN_HEIGHT as usize / 8 * space_invaders::SCREEN_WIDTH as usize;
const TEXELS_LEN: usize =
    space_invaders::SCREEN_HEIGHT as usize * space_invaders::SCREEN_WIDTH as usize;

fn main() {
    if let Err(err) = run(Opt::from_args()) {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}

fn run(opt: Opt) -> Result<()> {
    let (interrupt_sender, interrupt_receiver) = mpsc::sync_channel(0);
    let space_invaders = Arc::new(Mutex::new(SpaceInvaders::new(
        &[
            opt.roms.join("invaders.h"),
            opt.roms.join("invaders.g"),
            opt.roms.join("invaders.f"),
            opt.roms.join("invaders.e"),
        ],
        interrupt_receiver,
    )?));
    thread::spawn(update_space_invaders(Arc::clone(&space_invaders)));
    thread::spawn(generate_interrupts(interrupt_sender));

    let mut surface = GlfwSurface::new(
        WindowDim::Windowed(space_invaders::SCREEN_WIDTH * 2, space_invaders::SCREEN_HEIGHT * 2),
        "Space Invaders",
        WindowOpt::default(),
    )?;
    let mut graphics = Graphics::new(&mut surface)?;

    let mut loop_helper = LoopHelper::builder().build_with_target_rate(60.0);
    loop {
        loop_helper.loop_start();
        if let Ok(false) = process_input(&mut surface, &mut graphics) {
            break;
        }
        graphics.render(&space_invaders, &mut surface)?;
        loop_helper.loop_sleep();
    }
    Ok(())
}

fn update_space_invaders(space_invaders: Arc<Mutex<SpaceInvaders>>) -> impl FnOnce() -> () {
    move || {
        let mut loop_helper = LoopHelper::builder().build_with_target_rate(120.0);
        loop {
            // 2 MHz = 2,000,000 states per second = 2 states per microsecond
            let elapsed_states = loop_helper.loop_start().as_micros() * 2;
            let mut states = 0;
            while elapsed_states > states {
                states += u128::from(space_invaders.lock().unwrap().update());
            }
            loop_helper.loop_sleep();
        }
    }
}

fn generate_interrupts(interrupt_sender: SyncSender<[u8; 3]>) -> impl FnOnce() -> () {
    move || {
        let mut loop_helper = LoopHelper::builder().build_with_target_rate(120.0);
        loop {
            loop_helper.loop_start();
            if interrupt_sender.send([0xCF, 0, 0] /* RST 1 */).is_err() {
                break;
            }
            loop_helper.loop_sleep();
            loop_helper.loop_start();
            if interrupt_sender.send([0xD7, 0, 0] /* RST 2 */).is_err() {
                break;
            }
            loop_helper.loop_sleep();
        }
    }
}

struct Graphics {
    back_buffer: Framebuffer<Dim2, (), ()>,
    program: Program<(), (), Uniforms>,
    tessellation: Tess,
    texture: Texture<Dim2, NormR8UI>,
    texels: [<NormR8UI as Pixel>::Encoding; TEXELS_LEN],
}

impl Graphics {
    fn new(surface: &mut GlfwSurface) -> Result<Self> {
        let back_buffer = surface.back_buffer()?;
        let BuiltProgram { program, warnings } = Program::<(), (), Uniforms>::from_strings(
            None, // tessellation shaders
            VERTEX_SHADER,
            None, // geometry shader
            FRAGMENT_SHADER,
        )?;
        assert!(warnings.is_empty(), "{:?}", warnings);
        let tessellation =
            TessBuilder::new(surface).set_vertex_nb(4).set_mode(Mode::TriangleFan).build()?;
        let texture = Texture::<Dim2, NormR8UI>::new(
            surface,
            [space_invaders::SCREEN_HEIGHT, space_invaders::SCREEN_WIDTH],
            0, // mipmaps
            Sampler::default(),
        )?;
        let texels = [0; TEXELS_LEN];
        Ok(Self { back_buffer, program, tessellation, texture, texels })
    }

    fn render(
        &mut self,
        space_invaders: &Mutex<SpaceInvaders>,
        surface: &mut GlfwSurface,
    ) -> Result<()> {
        let framebuffer = unsafe {
            let mut framebuffer = MaybeUninit::<[u8; FRAMEBUFFER_LEN]>::uninit();
            (framebuffer.as_mut_ptr() as *mut u8).copy_from_nonoverlapping(
                space_invaders.lock().unwrap().framebuffer() as *const [u8] as *const u8,
                FRAMEBUFFER_LEN,
            );
            framebuffer.assume_init()
        };
        framebuffer_to_texels(&framebuffer, &mut self.texels);
        self.texture.upload_raw(GenMipmaps::No, &self.texels)?;
        surface.pipeline_builder().pipeline(
            &self.back_buffer,
            &PipelineState::default().enable_clear_depth(false),
            |pipeline, mut shading_gate| {
                let bound_texture = pipeline.bind_texture(&self.texture);
                shading_gate.shade(&self.program, |program_interface, mut render_gate| {
                    program_interface.sampler.update(&bound_texture);
                    render_gate.render(
                        &RenderState::default().set_depth_test(None),
                        |mut tess_gate| {
                            tess_gate.render(&self.tessellation);
                        },
                    )
                })
            },
        );
        surface.swap_buffers();
        Ok(())
    }
}

fn framebuffer_to_texels(
    framebuffer: &[u8],
    texels: &mut [<NormR8UI as Pixel>::Encoding; TEXELS_LEN],
) {
    framebuffer.iter().enumerate().for_each(|(i, byte)| {
        texels[i * 8..(i + 1) * 8].copy_from_slice(&[
            if byte & 0x01 > 0 { 0xFF } else { 0x00 },
            if byte & 0x02 > 0 { 0xFF } else { 0x00 },
            if byte & 0x04 > 0 { 0xFF } else { 0x00 },
            if byte & 0x08 > 0 { 0xFF } else { 0x00 },
            if byte & 0x10 > 0 { 0xFF } else { 0x00 },
            if byte & 0x20 > 0 { 0xFF } else { 0x00 },
            if byte & 0x40 > 0 { 0xFF } else { 0x00 },
            if byte & 0x80 > 0 { 0xFF } else { 0x00 },
        ]);
    });
}

fn process_input(surface: &mut GlfwSurface, graphics: &mut Graphics) -> Result<bool> {
    let mut resized = false;
    for event in surface.poll_events() {
        match event {
            WindowEvent::FramebufferSize(_, _) => resized = true,
            WindowEvent::Close => return Ok(false),
            _ => (),
        }
    }
    if resized {
        graphics.back_buffer = surface.back_buffer()?;
    }
    Ok(true)
}
