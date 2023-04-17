use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{prelude::*, stdout, BufReader, Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

enum ParsedObject {
    Blob(Vec<u8>),
    Commit,
    Tag,
    Tree(Vec<TreeEntry>),
}

struct TreeEntry {
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
        let content = data.split_off(header_end_index);
        let header = data;

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
}

fn parse_tree(data: &[u8]) -> Result<ParsedObject> {
    let mut entries = vec![];
    let mut reader = BufReader::new(data);
    while reader.fill_buf()?.is_empty() {
        let mut mode = vec![];
        reader.read_until(b' ', &mut mode)?;
        // TODO: move to anyhow
        let mode = String::from_utf8(mode)
            .map_err(|_| Error::from(ErrorKind::InvalidData))?
            .parse::<u32>()
            .map_err(|_| Error::from(ErrorKind::InvalidData))?;
        reader.consume(1); // skip the whitespace
        let mut name = vec![];
        reader.read_until(0, &mut name)?;
        let name = String::from_utf8(name).map_err(|_| Error::from(ErrorKind::InvalidData))?;
        let mut hash = vec![0; 20];
        reader.read_exact(&mut hash)?;
        let hash = String::from_utf8(hash).map_err(|_| Error::from(ErrorKind::InvalidData))?;
        entries.push(TreeEntry { mode, name, hash });
    }
    todo!();
}

pub type Hash = String;

pub fn blobify(filepath: &Path) -> Result<Hash> {
    let content_size = filepath.metadata()?.len() as usize;
    const HEADER_APPROX_SIZE: usize = 20;
    let mut blob = Vec::with_capacity(HEADER_APPROX_SIZE + content_size);
    blob.write(b"blob ")?;
    blob.write(content_size.to_string().as_bytes())?;
    blob.write(&[0])?;
    let header_size = blob.len();
    blob.resize(header_size + content_size, 0);
    fs::File::open(filepath)?.read_exact(&mut blob[header_size..])?;

    let mut hasher = Sha1::new();
    hasher.update(&blob);
    let hash = hex::encode(hasher.finalize());
    let filepath = object_path(&hash)?;
    fs::create_dir_all(filepath.parent().unwrap())?;
    let file = fs::File::create(filepath)?;
    let mut encoder = ZlibEncoder::new(file, Compression::best());
    encoder.write_all(&blob)?;
    encoder.finish()?;
    Ok(hash)
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
