#![warn(rust_2018_idioms)]

use std::{path::PathBuf, process};

use structopt::StructOpt;

use space_invaders::SpaceInvaders;

#[derive(Debug, StructOpt)]
#[structopt(about)]
struct Opt {
    /// A directory that contains invaders.{e,f,g,h}
    #[structopt(parse(from_os_str))]
    roms: PathBuf,
}

fn main() {
    if let Err(err) = run(Opt::from_args()) {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}

fn run(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    SpaceInvaders::new(&[
        opt.roms.join("invaders.h"),
        opt.roms.join("invaders.g"),
        opt.roms.join("invaders.f"),
        opt.roms.join("invaders.e"),
    ])?;
    Ok(())
}
