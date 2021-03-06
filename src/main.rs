#![warn(rust_2018_idioms)]

use std::{
    mem::MaybeUninit,
    path::PathBuf,
    process,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
};

use log::info;

use glfw::{Action, Context, Key, WindowEvent};
use luminance_derive::UniformInterface;
use luminance_front::{
    context::GraphicsContext,
    framebuffer::{Framebuffer, FramebufferError},
    pipeline::{PipelineState, TextureBinding},
    pixel::{NormR8UI, NormUnsigned, Pixel},
    render_state::RenderState,
    shader::{BuiltProgram, Program, Uniform},
    tess::{Mode, Tess, TessBuilder},
    texture::{Dim2, GenMipmaps, Sampler, Texture},
};
use luminance_glfw::GlfwSurface;
use luminance_windowing::{WindowDim, WindowOpt};

use spin_sleep::LoopHelper;

use structopt::StructOpt;

use space_invaders::{Port1, Port2, SpaceInvaders};

#[derive(Debug, StructOpt)]
#[structopt(about)]
struct Opt {
    /// A directory that contains invaders.{e,f,g,h}
    #[structopt(parse(from_os_str))]
    roms: PathBuf,

    /// A directory that contains {0..8}.wav
    #[structopt(parse(from_os_str))]
    samples: Option<PathBuf>,
}

#[derive(UniformInterface)]
struct Uniforms {
    sampler: Uniform<TextureBinding<Dim2, NormUnsigned>>,
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

fn run(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let (interrupt_sender, interrupt_receiver) = mpsc::sync_channel(0);
    let space_invaders = Arc::new(Mutex::new(SpaceInvaders::new(
        &[
            opt.roms.join("invaders.h"),
            opt.roms.join("invaders.g"),
            opt.roms.join("invaders.f"),
            opt.roms.join("invaders.e"),
        ],
        opt.samples.map(|samples| {
            [
                samples.join("0.wav"),
                samples.join("1.wav"),
                samples.join("2.wav"),
                samples.join("3.wav"),
                samples.join("4.wav"),
                samples.join("5.wav"),
                samples.join("6.wav"),
                samples.join("7.wav"),
                samples.join("8.wav"),
            ]
        }),
        interrupt_receiver,
    )?));
    thread::spawn(update_space_invaders(Arc::clone(&space_invaders)));
    thread::spawn(generate_interrupts(interrupt_sender));

    let mut surface = GlfwSurface::new_gl33(
        "Space Invaders",
        WindowOpt::default().set_dim(WindowDim::Windowed {
            width: space_invaders::SCREEN_WIDTH * 2,
            height: space_invaders::SCREEN_HEIGHT * 2,
        }),
    )?;
    let mut graphics = Graphics::new(&mut surface)?;

    let mut loop_helper = LoopHelper::builder().build_with_target_rate(60.0);
    loop {
        loop_helper.loop_start();
        if !(process_input(&mut surface, &mut graphics, &space_invaders)?) {
            break;
        }
        graphics.render(&space_invaders, &mut surface)?;
        loop_helper.loop_sleep();
    }
    Ok(())
}

fn update_space_invaders(space_invaders: Arc<Mutex<SpaceInvaders>>) -> impl FnOnce() {
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

fn generate_interrupts(interrupt_sender: SyncSender<[u8; 3]>) -> impl FnOnce() {
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
    pipeline_state: PipelineState,
    program: Program<(), (), Uniforms>,
    render_state: RenderState,
    vertices: Tess<()>,
    texture: Texture<Dim2, NormR8UI>,
    texels: [<NormR8UI as Pixel>::Encoding; TEXELS_LEN],
}

impl Graphics {
    fn new(surface: &mut GlfwSurface) -> Result<Self, Box<dyn std::error::Error>> {
        let back_buffer = surface.back_buffer()?;
        let pipeline_state = PipelineState::default().enable_clear_depth(false);
        let BuiltProgram { program, warnings } =
            surface.new_shader_program::<(), (), Uniforms>().from_strings(
                VERTEX_SHADER,
                None, // tessellation shaders
                None, // geometry shader
                FRAGMENT_SHADER,
            )?;
        assert!(warnings.is_empty(), "{:?}", warnings);
        let render_state = RenderState::default().set_depth_test(None);
        let vertices =
            TessBuilder::new(surface).set_vertex_nb(4).set_mode(Mode::TriangleFan).build()?;
        let texture = Texture::<Dim2, NormR8UI>::new(
            surface,
            [space_invaders::SCREEN_HEIGHT, space_invaders::SCREEN_WIDTH],
            0, // mipmaps
            Sampler::default(),
        )?;
        let texels = [0; TEXELS_LEN];
        Ok(Self { back_buffer, pipeline_state, program, render_state, vertices, texture, texels })
    }

    fn render(
        &mut self,
        space_invaders: &Mutex<SpaceInvaders>,
        surface: &mut GlfwSurface,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Graphics {
            back_buffer,
            pipeline_state,
            program,
            render_state,
            vertices,
            texture,
            texels,
        } = self;

        let framebuffer = unsafe {
            let mut framebuffer = MaybeUninit::<[u8; FRAMEBUFFER_LEN]>::uninit();
            (framebuffer.as_mut_ptr() as *mut u8).copy_from_nonoverlapping(
                space_invaders.lock().unwrap().framebuffer() as *const [u8] as *const u8,
                FRAMEBUFFER_LEN,
            );
            framebuffer.assume_init()
        };
        framebuffer_to_texels(&framebuffer, texels);
        texture.upload_raw(GenMipmaps::No, texels)?;
        surface
            .new_pipeline_gate()
            .pipeline(back_buffer, pipeline_state, |pipeline, mut shading_gate| {
                let bound_texture = pipeline.bind_texture(texture)?;
                shading_gate.shade(program, |mut program_interface, uniforms, mut render_gate| {
                    program_interface.set(&uniforms.sampler, bound_texture.binding());
                    render_gate.render(render_state, |mut tess_gate| tess_gate.render(&*vertices))
                })
            })
            .assume()
            .into_result()?;
        surface.window.swap_buffers();
        Ok(())
    }
}

const BYTE_TO_TEXELS: [[u8; 8]; 256] = byte_to_texels();

const fn byte_to_texels() -> [[u8; 8]; 256] {
    let mut mapping = [[0; 8]; 256];
    let mut i = 0;
    while i < 256 {
        let byte = i as u8;
        mapping[i] = [
            if byte & 0x01 > 0 { 0xFF } else { 0x00 },
            if byte & 0x02 > 0 { 0xFF } else { 0x00 },
            if byte & 0x04 > 0 { 0xFF } else { 0x00 },
            if byte & 0x08 > 0 { 0xFF } else { 0x00 },
            if byte & 0x10 > 0 { 0xFF } else { 0x00 },
            if byte & 0x20 > 0 { 0xFF } else { 0x00 },
            if byte & 0x40 > 0 { 0xFF } else { 0x00 },
            if byte & 0x80 > 0 { 0xFF } else { 0x00 },
        ];
        i += 1;
    }
    mapping
}

fn framebuffer_to_texels(
    framebuffer: &[u8],
    texels: &mut [<NormR8UI as Pixel>::Encoding; TEXELS_LEN],
) {
    framebuffer.iter().enumerate().for_each(|(i, &byte)| {
        texels[i * 8..(i + 1) * 8].copy_from_slice(&BYTE_TO_TEXELS[usize::from(byte)]);
    });
}

fn process_input(
    surface: &mut GlfwSurface,
    graphics: &mut Graphics,
    space_invaders: &Mutex<SpaceInvaders>,
) -> Result<bool, FramebufferError> {
    let mut resized = false;
    surface.window.glfw.poll_events();
    for (_, event) in surface.events_rx.try_iter() {
        match event {
            WindowEvent::Key(Key::Left, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.insert(Port1::PLAYER_1_LEFT);
                    space_invaders.port2.insert(Port2::PLAYER_2_LEFT);
                }
                Action::Release => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.remove(Port1::PLAYER_1_LEFT);
                    space_invaders.port2.remove(Port2::PLAYER_2_LEFT);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::Right, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.insert(Port1::PLAYER_1_RIGHT);
                    space_invaders.port2.insert(Port2::PLAYER_2_RIGHT);
                }
                Action::Release => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.remove(Port1::PLAYER_1_RIGHT);
                    space_invaders.port2.remove(Port2::PLAYER_2_RIGHT);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::Space, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.insert(Port1::PLAYER_1_FIRE);
                    space_invaders.port2.insert(Port2::PLAYER_2_FIRE);
                }
                Action::Release => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port1.remove(Port1::PLAYER_1_FIRE);
                    space_invaders.port2.remove(Port2::PLAYER_2_FIRE);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::C, _, action, _) => match action {
                Action::Press => {
                    space_invaders.lock().unwrap().port1.insert(Port1::COIN);
                }
                Action::Release => {
                    space_invaders.lock().unwrap().port1.remove(Port1::COIN);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::T, _, action, _) => match action {
                Action::Press => {
                    space_invaders.lock().unwrap().port2.insert(Port2::TILT);
                }
                Action::Release => {
                    space_invaders.lock().unwrap().port2.remove(Port2::TILT);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::Num1, _, action, _) => match action {
                Action::Press => {
                    space_invaders.lock().unwrap().port1.insert(Port1::PLAYER_1_START);
                }
                Action::Release => {
                    space_invaders.lock().unwrap().port1.remove(Port1::PLAYER_1_START);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::Num2, _, action, _) => match action {
                Action::Press => {
                    space_invaders.lock().unwrap().port1.insert(Port1::PLAYER_2_START);
                }
                Action::Release => {
                    space_invaders.lock().unwrap().port1.remove(Port1::PLAYER_2_START);
                }
                Action::Repeat => (),
            },
            WindowEvent::Key(Key::F1, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    let mut bits = space_invaders.port2.bits();
                    bits = (bits & 0b1111_1100) | (((bits & 0b0000_0011) + 1) % 4);
                    space_invaders.port2 = unsafe { Port2::from_bits_unchecked(bits) };
                    match space_invaders.port2.bits() & 0b0000_0011 {
                        0 => info!("num of lives: 3"),
                        1 => info!("num of lives: 4"),
                        2 => info!("num of lives: 5"),
                        3 => info!("num of lives: 6"),
                        _ => unreachable!(),
                    }
                }
                Action::Release | Action::Repeat => (),
            },
            WindowEvent::Key(Key::F2, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port2.toggle(Port2::EXTRA_LIFE_AT);
                    if space_invaders.port2.contains(Port2::EXTRA_LIFE_AT) {
                        info!("extra life at: 1000 points");
                    } else {
                        info!("extra life at: 1500 points");
                    }
                }
                Action::Release | Action::Repeat => (),
            },
            WindowEvent::Key(Key::F3, _, action, _) => match action {
                Action::Press => {
                    let mut space_invaders = space_invaders.lock().unwrap();
                    space_invaders.port2.toggle(Port2::PRICING_DISPLAY);
                    if space_invaders.port2.contains(Port2::PRICING_DISPLAY) {
                        info!("pricing display: off");
                    } else {
                        info!("pricing display: on");
                    }
                }
                Action::Release | Action::Repeat => (),
            },
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
