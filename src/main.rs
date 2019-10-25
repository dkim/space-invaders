#![warn(rust_2018_idioms)]

use std::{path::PathBuf, process};

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about)]
struct Opt {
    /// A path to INVADERS.E
    #[structopt(name = "INVADERS.E", parse(from_os_str))]
    invaders_e: PathBuf,
    /// A path to INVADERS.F
    #[structopt(name = "INVADERS.F", parse(from_os_str))]
    invaders_f: PathBuf,
    /// A path to INVADERS.G
    #[structopt(name = "INVADERS.G", parse(from_os_str))]
    invaders_g: PathBuf,
    /// A path to INVADERS.H
    #[structopt(name = "INVADERS.H", parse(from_os_str))]
    invaders_h: PathBuf,
}

fn main() {
    if let Err(err) = run(Opt::from_args()) {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}

fn run(_opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
