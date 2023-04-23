mod git;

use clap::{Args, Parser, Subcommand};
use std::fs;
use std::io::Result;
use std::path::PathBuf;

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
    HashObject(HashObject),
    LsTree(LsTree),
}

#[derive(Args, Debug)]
struct CatFile {
    #[arg(short)]
    pretty: bool,
    hash: String,
}

#[derive(Args, Debug)]
struct HashObject {
    #[arg(short)]
    write: bool,
    path: PathBuf,
}

#[derive(Args, Debug)]
struct LsTree {
    #[arg(long)]
    name_only: bool,
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
            Self::CatFile(ref command) => git::Object::from_hash(&command.hash)?.print(),
            Self::HashObject(ref command) => {
                let hash = git::blobify(&command.path)?;
                println!("{}", hex::encode(&hash));
                Ok(())
            }
            Self::LsTree(ref command) => git::Object::from_hash(&command.hash)?
                .parse()?
                .print_tree_names(),
        }
    }
}

fn main() -> Result<()> {
    let args = CommandLine::parse();
    args.command.run()?;
    Ok(())
}
