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
    WriteTree,
    CommitTree(CommitTree),
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

#[derive(Args, Debug)]
struct CommitTree {
    #[arg(short)]
    parent_hash: String,
    #[arg(short)]
    message: String,
    tree_hash: String,
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
            Self::WriteTree => {
                let hash = git::write_tree(&PathBuf::from("."))?;
                println!("{}", hex::encode(&hash));
                Ok(())
            }
            Self::CommitTree(ref command) => {
                let hash = git::commit(
                    &git::parse_hash(&command.tree_hash)?,
                    &git::parse_hash(&command.parent_hash)?,
                    &command.message,
                )?;
                println!("{}", hex::encode(&hash));
                Ok(())
            }
        }
    }
}

fn main() -> Result<()> {
    let args = CommandLine::parse();
    args.command.run()?;
    Ok(())
}
