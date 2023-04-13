use clap::{Parser, Subcommand, Args};
use flate2::read::ZlibDecoder;
#[allow(unused_imports)]
use std::env;
use std::io::{prelude::*, stdout, Error, ErrorKind, Result};
use std::path::PathBuf;
use std::fs;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CommandLine {
    #[command(subcommand)]
    command: Command,
}


#[derive(Subcommand, Debug)]
enum Command {
    Init,
    CatFile(CatFile),
}

#[derive(Args, Debug)]
struct CatFile {
    #[arg(short)]
    pretty: bool,
    hash: String,
}

impl Command {
    fn run(&self) -> Result<()> {
        match self {
            Self::Init => {
                fs::create_dir(".git")?;
                fs::create_dir(".git/objects")?;
                fs::create_dir(".git/refs")?;
                fs::write(".git/HEAD", "ref: refs/heads/master\n")
            }
            Self::CatFile(ref command) => {
                let object = Object::from_hash(&command.hash)?;
                stdout().write_all(&object.content)
            }
        }
    }
}

struct Object {
    content: Vec<u8>,
}

impl Object {
    fn from_hash(hash: &str) -> Result<Self> {
        const HASH_SIZE: usize = 40; // hex string of SHA1
        if hash.len() != HASH_SIZE {
            return Err(Error::from(ErrorKind::InvalidInput));
        }
        let (subdir, filename) = hash
            .split_at(2);
        let mut filepath = PathBuf::new();
        filepath.push(".git");
        filepath.push("objects");
        filepath.push(subdir);
        filepath.push(filename);
        let file = fs::File::open(filepath)?;
        let decoded_file = ZlibDecoder::new(file);
        // TODO: verify header
        Ok(Self {
            content: decoded_file
                .bytes()
                .skip_while(|b| b.is_ok() && b.as_ref().unwrap() != &0)
                .skip(1)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

fn main() -> Result<()> {
    let args = CommandLine::parse();
    args.command.run()?;
    Ok(())
}
