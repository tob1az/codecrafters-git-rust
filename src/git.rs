use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{prelude::*, stdout, Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

pub struct Object {
    content: Vec<u8>,
}

impl Object {
    pub fn from_hash(hash: &str) -> Result<Self> {
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

    pub fn print(&self) -> Result<()> {
        stdout().write_all(&self.content)
    }
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
    let (subdir, filename) = hash.split_at(2);
    let mut filepath = PathBuf::new();
    filepath.push(".git");
    filepath.push("objects");
    filepath.push(subdir);
    fs::create_dir_all(filepath.clone())?;
    filepath.push(filename);
    let file = fs::File::create(filepath)?;
    let mut encoder = ZlibEncoder::new(file, Compression::best());
    encoder.write_all(&blob)?;
    encoder.finish()?;
    Ok(hash)
}
