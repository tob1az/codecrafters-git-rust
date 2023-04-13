use flate2::read::ZlibDecoder;
use std::io::{prelude::*, stdout, Error, ErrorKind, Result};
use std::path::PathBuf;
use std::fs;

pub struct Object {
    content: Vec<u8>,
}

impl Object {
    pub fn from_hash(hash: &str) -> Result<Self> {
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

    pub fn print(&self) -> Result<()> {
        stdout().write_all(&self.content)
    }
}

