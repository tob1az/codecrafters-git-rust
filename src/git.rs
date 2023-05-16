#![allow(dead_code)]

pub mod pack;
pub mod remote;

use anyhow::{anyhow, bail, Context, Result};
use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::io::{prelude::*, stdout, BufReader};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{env, fs};

const HASH_SIZE: usize = 20; // hex string of SHA1
const HASH_HEX_SIZE: usize = 40; // hex string of SHA1
const DIRECTORY_MODE: u32 = 0o40000;

pub enum ParsedObject {
    Blob(Vec<u8>),
    Commit(remote::Sha1),
    Tag,
    Tree(Vec<TreeEntry>),
}

impl ParsedObject {
    pub fn print_tree_names(&self) -> Result<()> {
        match &self {
            ParsedObject::Tree(ref tree) => {
                for entry in tree {
                    println!("{}", entry.name);
                }
                Ok(())
            }
            _ => Err(anyhow!("Unsupported object")),
        }
    }
}

pub struct TreeEntry {
    mode: u32,
    name: String,
    hash: Hash,
}

pub struct Object {
    header: Vec<u8>,
    content: Vec<u8>,
}

impl Object {
    fn new(kind: &[u8], content: &[u8]) -> Self {
        let mut header = vec![];
        header.write(kind).unwrap();
        header.write(b" ").unwrap();
        header.write(content.len().to_string().as_bytes()).unwrap();

        Object {
            header,
            content: content.to_vec(),
        }
    }

    pub fn from_hash(hash: &str) -> Result<Self> {
        let filepath = object_path(hash)?;
        let file = BufReader::new(fs::File::open(filepath)?);
        let mut decoded_file = ZlibDecoder::new(file);
        let mut data = vec![];
        decoded_file.read_to_end(&mut data)?;
        let header_end_index = data
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| anyhow!("Header not found"))?;
        let content = data.split_off(header_end_index + 1);
        let mut header = data;
        let _ = header.pop(); // remove the separator byte

        // TODO: verify header
        Ok(Self { header, content })
    }

    pub fn print(&self) -> Result<()> {
        stdout()
            .write_all(&self.content)
            .with_context(|| "Failed to print object")
    }

    pub fn parse(&self) -> Result<ParsedObject> {
        let kind = self
            .header
            .split(|&b| b == b' ')
            .next()
            .ok_or_else(|| anyhow!("Invalid object header"))?;
        match kind {
            b"blob" => Ok(ParsedObject::Blob(self.content.clone())),
            b"commit" => Ok(ParsedObject::Commit(parse_commit(&self.content)?)),
            b"tag" => Ok(ParsedObject::Tag),
            b"tree" => Ok(parse_tree(&self.content)?),
            _ => Err(anyhow!("Unsupported object type")),
        }
    }
    pub fn serialize(&self) -> Result<Hash> {
        // TODO: extract separator?
        let separator = [b'\0'; 1];
        let hash = self.hash();
        let filepath = object_path(&hex::encode(&hash))?;
        fs::create_dir_all(filepath.parent().unwrap())?;
        let file = fs::File::create(filepath)?;
        let mut encoder = ZlibEncoder::new(file, Compression::best());
        encoder.write_all(&self.header)?;
        encoder.write_all(&separator)?;
        encoder.write_all(&self.content)?;
        encoder.finish()?;
        Ok(hash)
    }

    fn hash(&self) -> Hash {
        let mut hasher = Sha1::new();
        let separator = [b'\0'; 1];
        hasher.update(&self.header);
        hasher.update(&separator);
        hasher.update(&self.content);
        hasher.finalize().into_iter().collect::<Vec<_>>()
    }
}

fn parse_tree(data: &[u8]) -> Result<ParsedObject> {
    let mut entries = vec![];
    let mut reader = BufReader::new(data);
    while !reader.fill_buf()?.is_empty() {
        let mode = u32::from_str_radix(&read_field(&mut reader, b' ')?, 8)
            .with_context(|| "Failed to read file mode")?;
        let name = read_field(&mut reader, 0)?;
        let mut hash = vec![0; HASH_SIZE];
        reader.read_exact(&mut hash)?;
        entries.push(TreeEntry { mode, name, hash });
    }
    Ok(ParsedObject::Tree(entries))
}

fn read_field<R: BufRead>(reader: &mut R, separator: u8) -> Result<String> {
    let mut field = vec![];
    reader.read_until(separator, &mut field)?;
    let _ = field.pop(); // remove separator

    Ok(String::from_utf8(field).with_context(|| anyhow!("Failed to read field"))?)
}

pub type Hash = Vec<u8>;

pub fn blobify(filepath: &Path) -> Result<Hash> {
    let content_size: usize = filepath.metadata()?.len() as usize;
    let mut content = Vec::with_capacity(content_size);
    content.resize(content_size, 0);
    fs::File::open(filepath)?.read_exact(&mut content)?;
    Object::new(b"blob", &content).serialize()
}

fn object_path(hash: &str) -> Result<PathBuf> {
    if hash.len() != HASH_HEX_SIZE {
        bail!("Invalid hash length {}", hash.len());
    }
    let (subdir, filename) = hash.split_at(2);
    let mut filepath = PathBuf::new();
    filepath.push(".git");
    filepath.push("objects");
    filepath.push(subdir);
    filepath.push(filename);
    Ok(filepath)
}

pub fn write_tree(directory: &Path) -> Result<Hash> {
    let content = build_tree_content(directory)?;
    Object::new(b"tree", &content).serialize()
}

fn build_tree_content(directory: &Path) -> Result<Vec<u8>> {
    assert!(directory.is_dir());
    let mut entries = directory
        .read_dir()?
        .into_iter()
        .flatten()
        .filter(|e| !(e.path().is_dir() && e.path().ends_with(".git")))
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());
    let content = entries
        .into_iter()
        .map(|entry| {
            let meta = entry.metadata()?;
            let (mode, hash) = if meta.is_dir() {
                (DIRECTORY_MODE, write_tree(&entry.path())?)
            } else if meta.is_file() {
                (meta.permissions().mode(), blobify(&entry.path())?)
            } else {
                bail!("Unsupported file type: {}", entry.path().display());
            };
            let mut buffer = vec![];
            write!(
                &mut buffer,
                "{:o} {}",
                mode,
                entry.file_name().to_string_lossy()
            )?;
            buffer.push(0);
            buffer.extend(hash);
            Ok(buffer)
        })
        .collect::<Result<Vec<_>>>()?
        .concat();

    Ok(content)
}

pub fn commit(tree: &Hash, parent: &Hash, message: &str) -> Result<Hash> {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| anyhow!("Failed to read system time"))?
        .as_secs();
    let timestamp = format!("{timestamp} +0000");
    let parent_hash = hex::encode(&parent);
    let tree_hash = hex::encode(&tree);
    let content = format!(
        "tree {tree_hash}
parent {parent_hash}
author Anonymous {timestamp}
committer Anonymous {timestamp}

{message}
"
    );
    let hash = Object::new(b"commit", content.as_bytes()).serialize()?;

    let mut filepath = PathBuf::new();
    filepath.push(".git");
    filepath.push("refs");
    filepath.push("heads");
    filepath.push("master");
    fs::write(filepath, format!("{}\n", hex::encode(&hash)))?;
    Ok(hash)
}

pub fn parse_hash(hash: &str) -> Result<Hash> {
    if hash.len() != HASH_HEX_SIZE {
        bail!("Invalid hash size {}", hash.len());
    }
    hex::decode(hash).with_context(|| "Invalid hash")
}

pub fn init<T>(path: T) -> Result<()>
where
    T: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    env::set_current_dir(&path)?;
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok(())
}

pub fn store_references(refs: &[remote::Reference]) -> Result<String> {
    println!("Store references");
    let mut refs = refs.iter();
    let (head_hash, _) = refs.next().ok_or_else(|| anyhow!("No HEAD reference"))?;
    let dot_git = Path::new(".git");
    for (hash, path) in refs {
        if hash == head_hash {
            fs::write(dot_git.join("HEAD"), format!("ref: {path}"))?;
        }
        let ref_filepath = dot_git.join(path);
        let parent_dir = ref_filepath.parent().unwrap();
        if !parent_dir.exists() {
            fs::create_dir_all(parent_dir)?;
        }
        fs::write(ref_filepath, format!("{hash}\n"))?;
    }
    println!("Stored all references");

    Ok(head_hash.clone())
}

pub fn checkout(hash: &str) -> Result<()> {
    println!("Checkout {hash}");
    if let ParsedObject::Commit(commit) = Object::from_hash(hash)?.parse()? {
        checkout_tree(&commit, &std::env::current_dir()?)
    } else {
        bail!("{hash} is not a commit")
    }
}

fn parse_commit(content: &[u8]) -> Result<remote::Sha1> {
    Ok(String::from_utf8(
        content
            .strip_prefix(b"tree ")
            .ok_or_else(|| anyhow!("commit does not start with the tree line"))?
            .bytes()
            .flatten()
            .take_while(|b| *b != b'\n')
            .collect(),
    )?)
}

fn checkout_tree(tree_hash: &str, target_path: &Path) -> Result<()> {
    if let ParsedObject::Tree(entries) = Object::from_hash(tree_hash)?.parse()? {
        // recurse trees and create objects from blobs
        fs::create_dir_all(target_path)?;
        for entry in entries {
            if entry.mode == DIRECTORY_MODE {
                checkout_tree(&hex::encode(&entry.hash), target_path)?
            } else {
                checkout_file(entry)?
            }
        }
        Ok(())
    } else {
        bail!("{tree_hash} is not a tree")
    }
}

fn checkout_file(file_entry: TreeEntry) -> Result<()> {
    let sha = hex::encode(&file_entry.hash);
    if let ParsedObject::Blob(content) = Object::from_hash(&sha)?.parse()? {
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(file_entry.mode)
            .open(file_entry.name)?
            .write_all(&content)?;
        Ok(())
    } else {
        bail!("{sha} is not a blob")
    }
}
