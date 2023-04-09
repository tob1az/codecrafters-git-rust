#[allow(unused_imports)]
use std::env;
use std::fs;
use std::io::Result;
use clap::{Parser, Subcommand};


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Init,
}

impl Command  {
    fn run(&self) -> Result<()> {
        match self {
            Self::Init => {
                fs::create_dir(".git")?;
                fs::create_dir(".git/objects")?;
                fs::create_dir(".git/refs")?;
                fs::write(".git/HEAD", "ref: refs/heads/master\n")
            }
        }
    }
}



fn main() -> Result<()> {
    let args = Args::parse();
    args.command.run()?;
    Ok(())
}
