#![allow(dead_code)]

use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{prelude::*, stdout, BufReader, Error, ErrorKind, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
            _ => Err(Error::from(ErrorKind::Unsupported)),
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
    pub fn from_hash(hash: &str) -> Result<Self> {
        let filepath = object_path(hash)?;
        let file = BufReader::new(fs::File::open(filepath)?);
        let mut decoded_file = ZlibDecoder::new(file);
        let mut data = vec![];
        decoded_file.read_to_end(&mut data)?;
        let header_end_index = data
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::from(ErrorKind::InvalidData))?;
        let content = data.split_off(header_end_index + 1);
        let mut header = data;
        let _ = header.pop(); // remove the separator byte

        // TODO: verify header
        Ok(Self { header, content })
    }

    pub fn print(&self) -> Result<()> {
        stdout().write_all(&self.content)
    }

    pub fn parse(&self) -> Result<ParsedObject> {
        let kind = self
            .header
            .split(|&b| b == b' ')
            .next()
            .ok_or_else(|| Error::from(ErrorKind::InvalidData))?;
        match kind {
            b"blob" => Ok(ParsedObject::Blob(self.content.clone())),
            b"commit" => Ok(ParsedObject::Commit),
            b"tag" => Ok(ParsedObject::Tag),
            b"tree" => Ok(parse_tree(&self.content)?),
            _ => Err(Error::from(ErrorKind::InvalidData)),
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
            .map_err(|_| Error::from(ErrorKind::InvalidData))?;
        let name = read_field(&mut reader, 0)?;
        let mut hash = vec![0; 20];
        reader.read_exact(&mut hash)?;
        entries.push(TreeEntry { mode, name, hash });
    }
    Ok(ParsedObject::Tree(entries))
}

fn read_field<R: BufRead>(reader: &mut R, separator: u8) -> Result<String> {
    let mut field = vec![];
    reader.read_until(separator, &mut field)?;
    let _ = field.pop(); // remove separator

    // TODO: move to anyhow
    Ok(String::from_utf8(field).map_err(|_| Error::from(ErrorKind::InvalidData))?)
}

pub type Hash = Vec<u8>;

pub fn blobify(filepath: &Path) -> Result<Hash> {
    let content_size = filepath.metadata()?.len() as usize;
    let mut header = vec![];
    header.write(b"blob ")?;
    header.write(content_size.to_string().as_bytes())?;
    let mut content = Vec::with_capacity(content_size);
    content.resize(content_size, 0);
    fs::File::open(filepath)?.read_exact(&mut content)?;
    Object { header, content }.serialize()
}

fn object_path(hash: &str) -> Result<PathBuf> {
    const HASH_SIZE: usize = 40; // hex string of SHA1
    if hash.len() != HASH_SIZE {
        return Err(Error::from(ErrorKind::InvalidInput));
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
    let mut header = vec![];
    header.write(b"tree ")?;
    header.write(content.len().to_string().as_bytes())?;
    Object { header, content }.serialize()
}

fn build_tree_content(directory: &Path) -> Result<Vec<u8>> {
    assert!(directory.is_dir());
    let content = directory
        .read_dir()?
        .into_iter()
        .flatten()
        .map(|entry| {
            let meta = entry.metadata()?;
            let hash = if meta.is_dir() {
                write_tree(&entry.path())?
            } else if meta.is_file() {
                blobify(&entry.path())?
            } else {
                return Err(Error::from(ErrorKind::Unsupported));
            };
            let mut buffer = vec![];
            write!(
                &mut buffer,
                "{:o} {}",
                meta.permissions().mode(),
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
