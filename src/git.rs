#![allow(dead_code)]

pub mod pack;
pub mod remote;

use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{prelude::*, stdout, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use anyhow::{anyhow, Context, Result, bail};

const HASH_SIZE: usize = 40; // hex string of SHA1

pub enum ParsedObject {
    Blob(Vec<u8>),
    Commit,
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
        stdout().write_all(&self.content).with_context(|| "Failed to print object")
    }

    pub fn parse(&self) -> Result<ParsedObject> {
        let kind = self
            .header
            .split(|&b| b == b' ')
            .next()
            .ok_or_else(|| anyhow!("Invalid object header"))?;
        match kind {
            b"blob" => Ok(ParsedObject::Blob(self.content.clone())),
            b"commit" => Ok(ParsedObject::Commit),
            b"tag" => Ok(ParsedObject::Tag),
            b"tree" => Ok(parse_tree(&self.content)?),
            _ => Err(anyhow!("Unsupported object type")),
        }
    }
    fn serialize(&self) -> Result<Hash> {
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
        let mode = read_field(&mut reader, b' ')?
            .parse::<u32>()
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

    Ok(String::from_utf8(field).with_context(|| "Failed to read field")?)
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
    if hash.len() != HASH_SIZE {
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
                const DIRECTORY: u32 = 0o40000;
                (DIRECTORY, write_tree(&entry.path())?)
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
    if hash.len() != HASH_SIZE {
        bail!("Invalid hash size {}", hash.len());
    }
    hex::decode(hash).with_context(|| "Invalid hash")
}
