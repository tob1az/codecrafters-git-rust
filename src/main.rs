mod git;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use reqwest::Url;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

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
    Clone(CloneRepo),
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

#[derive(Args, Debug)]
struct CloneRepo {
    url: String,
    path: PathBuf,
}

impl Command {
    fn run(&self) -> Result<()> {
        match self {
            Self::Init => {
                git::init(".")
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
            Self::Clone(ref command) => {
                let remote_url = if command.url.ends_with('/') {
                    Url::from_str(&command.url)?
                } else {
                    let url = command.url.clone() + "/";
                    Url::from_str(&url)?
                };
                let refs = git::remote::discover_references(&remote_url)?;
                let pack = git::remote::fetch_pack(&remote_url, &refs)?;
                let objects = git::pack::parse(pack)?;
                git::init(&command.path)?;
                // init
                // store objects
                // write refs
                // checkout HEAD
                todo!()
            }
        }
    }
}

fn main() -> Result<()> {
    let args = CommandLine::parse();
    args.command.run()?;
    Ok(())
}
