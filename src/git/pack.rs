use super::{Object, HASH_HEX_SIZE};
use anyhow::{anyhow, bail, Ok, Result};
use bytes::{Buf, Bytes};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::{collections::HashMap, io::Read};

const SIGNATURE_SIZE: usize = 4;
const SIGNATURE: &[u8; SIGNATURE_SIZE] = b"PACK";
const VERSION: u32 = 2;
const HASH_SIZE: usize = HASH_HEX_SIZE / 2;
const PACK_FRAME_SIZE: usize = SIGNATURE_SIZE + std::mem::size_of::<u32>() * 2 + HASH_SIZE;

#[repr(u8)]
enum ObjectTypeId {
    Commit = 1,
    Tree = 2,
    Blob = 3,
    Tag = 4,
    OffsetDelta = 6,
    ReferenceDelta = 7,
}

impl ToString for ObjectTypeId {
    fn to_string(&self) -> String {
        match self {
            Self::Commit => "commit".to_owned(),
            Self::Tree => "tree".to_owned(),
            Self::Blob => "blob".to_owned(),
            Self::Tag => "tag".to_owned(),
            Self::OffsetDelta => "ofs_delta".to_owned(),
            Self::ReferenceDelta => "ref_delta".to_owned(),
        }
    }
}

impl TryFrom<usize> for ObjectTypeId {
    type Error = anyhow::Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            x if x == ObjectTypeId::Commit as usize => Ok(ObjectTypeId::Commit),
            x if x == ObjectTypeId::Tree as usize => Ok(ObjectTypeId::Tree),
            x if x == ObjectTypeId::Blob as usize => Ok(ObjectTypeId::Blob),
            x if x == ObjectTypeId::Tag as usize => Ok(ObjectTypeId::Tag),
            x if x == ObjectTypeId::OffsetDelta as usize => Ok(ObjectTypeId::OffsetDelta),
            x if x == ObjectTypeId::ReferenceDelta as usize => Ok(ObjectTypeId::ReferenceDelta),
            _ => Err(anyhow!("Unsupported object ID {value}")),
        }
    }
}

pub fn parse(pack_buffer: Vec<u8>) -> Result<Vec<Object>> {
    let mut parser = Bytes::from(pack_buffer);
    verify_pack(&mut parser)?;
    let object_number = parser.get_u32();
    println!("Object number: {object_number}");
    let mut objects = Vec::with_capacity(object_number as usize);
    let mut ref_to_index = HashMap::new();
    for _ in 0..object_number {
        let (id, size) = parse_object_header(&mut parser)?;
        println!("{} {size}", id.to_string());
        use ObjectTypeId::*;
        match id {
            Commit | Tree | Blob | Tag => {
                let content = unpack_content(size, &mut parser)?;

                let object = Object::new(id.to_string().as_bytes(), &content);
                ref_to_index.insert(object.hash(), objects.len());
                println!("hash {}", hex::encode(object.hash()));
                objects.push(object);
            }
            ReferenceDelta => {
                let reference = parser.copy_to_bytes(HASH_SIZE).to_vec();
                if let Some(index) = ref_to_index.get(&reference) {
                    let source_object = &objects[*index];
                    let object = patch_object(source_object, size, &mut parser)?;
                    ref_to_index.insert(object.hash(), objects.len());
                    objects.push(object);
                } else {
                    bail!("Unknown object reference {}", hex::encode(reference));
                }
            }
            OffsetDelta => {
                let offset = parse_multibyte_number(&mut parser)?;
                if offset > objects.len() {
                    bail!(
                        "Wrong object offset {offset}, current idx {}",
                        objects.len()
                    );
                }
                let index = objects.len() - offset;
                let source_object = &mut objects[index];
                let object = patch_object(source_object, size, &mut parser)?;
                ref_to_index.insert(object.hash(), objects.len());
                objects.push(object);
            }
        }
    }
    println!("Parsed {} objects", objects.len());
    Ok(objects)
}

fn patch_object(object: &Object, delta_size: usize, parser: &mut Bytes) -> Result<Object> {
    let mut delta_instructions = Bytes::from(unpack_content(delta_size, parser)?);
    let _source_size = parse_multibyte_number(&mut delta_instructions)?;
    let target_size = parse_multibyte_number(&mut delta_instructions)?;
    let patched_content = patch_content(delta_instructions, target_size, &object.content)?;
    // TODO: get object kind correctly
    Ok(Object::new(
        object.header.split(|c| *c == b' ').next().unwrap(),
        &patched_content,
    ))
}

fn verify_pack(parser: &mut Bytes) -> Result<()> {
    if parser.len() <= PACK_FRAME_SIZE {
        bail!("Pack too short: {}", parser.len());
    }
    println!("pack length {}", parser.len());
    let expected_hash = parser.split_off(parser.len() - HASH_SIZE);

    let real_hash = Sha1::new()
        .chain_update(&parser[..])
        .finalize()
        .into_iter()
        .collect::<Vec<_>>();
    if real_hash != expected_hash {
        bail!(
            "Corrupted pack: expected {}, got {}",
            hex::encode(expected_hash),
            hex::encode(real_hash)
        );
    }
    let signature = parser.copy_to_bytes(SIGNATURE_SIZE);
    if &signature[..] != SIGNATURE {
        bail!("Wrong signature {signature:?}");
    }
    let version = parser.get_u32();
    if version != VERSION {
        bail!("Wrong version {version}");
    }
    Ok(())
}

fn parse_object_header(parser: &mut Bytes) -> Result<(ObjectTypeId, usize)> {
    if !parser.has_remaining() {
        bail!("Object header is too short");
    }
    let first_byte = parser.get_u8();
    const ID_MASK: u8 = 0b0111_0000;
    const ID_BIT_WIDTH: u32 = 4;
    let id = ObjectTypeId::try_from(((first_byte & ID_MASK) as usize) >> ID_BIT_WIDTH)
        .map_err(|_| anyhow!("Unknown Object ID"))?;
    // clear ID bits
    let first_byte = first_byte & !ID_MASK;
    Ok((
        id,
        parse_multibyte_number_tail(first_byte, ID_BIT_WIDTH, parser)?,
    ))
}

fn parse_multibyte_number_tail(
    first_byte: u8,
    bit_width: u32,
    parser: &mut Bytes,
) -> Result<usize> {
    const MORE_BYTES: u8 = 0x80;
    const SIZE_MASK: u8 = 0x7f;
    let mut bit_shift = bit_width;
    let mut byte = first_byte;
    let mut number = (first_byte & SIZE_MASK) as usize;
    while parser.has_remaining() && (byte & MORE_BYTES != 0) {
        byte = parser.get_u8();
        let bits = (byte & SIZE_MASK) as usize;
        number |= bits
            .checked_shl(bit_shift)
            .ok_or_else(|| anyhow!("Object size overflow"))?;
        bit_shift += 7;
    }
    Ok(number)
}

fn parse_multibyte_number(parser: &mut Bytes) -> Result<usize> {
    if !parser.has_remaining() {
        bail!("number too short");
    }
    const DEFAULT_BIT_COUNT: u32 = 7;
    let first_byte = parser.get_u8();
    parse_multibyte_number_tail(first_byte, DEFAULT_BIT_COUNT, parser)
}

fn unpack_content(size: usize, parser: &mut Bytes) -> Result<Vec<u8>> {
    let packed = parser.clone();
    let mut content = Vec::with_capacity(size);
    let mut decoder = ZlibDecoder::new(packed.as_ref());
    decoder.read_to_end(&mut content)?;
    parser.advance(decoder.total_in() as usize);
    Ok(content)
}

fn patch_content(mut delta: Bytes, target_size: usize, object: &[u8]) -> Result<Vec<u8>> {
    let mut new_content = Vec::with_capacity(target_size);
    while delta.has_remaining() {
        let header = delta.get_u8();
        const COPY_BIT: u8 = 0x80;
        if header & COPY_BIT != 0 {
            let offset = build_number(header, 4, &mut delta)?;
            let header = header >> 4;
            let size = build_number(header, 3, &mut delta)?;
            println!("Copy from {offset} size {size}");
            new_content.extend_from_slice(
                object
                    .get(offset..offset + size)
                    .ok_or_else(|| anyhow!("Wrong delta copy"))?,
            );
        } else {
            let size = header as usize;
            println!("Insert {size} bytes");
            let remaining = delta.remaining();
            if remaining < size {
                bail!("Wrong delta");
            }
            let patch = delta.copy_to_bytes(size);
            new_content.extend(patch.into_iter());
        }
    }
    if new_content.len() != target_size {
        bail!(
            "Unexpected object size (expected {target_size}, got {})",
            new_content.len()
        );
    }
    Ok(new_content)
}

fn build_number(mask: u8, bit_width: u32, data: &mut Bytes) -> Result<usize> {
    // cannot share &mut Bytes with the try_fold closure, need a clone
    let mut number_reader = data.clone();
    let result =
        (0..bit_width)
            .filter(|b| mask & (1 << b) != 0)
            .try_fold(0, |number, byte_index| {
                let shift = 8 * byte_index;
                if !number_reader.has_remaining() {
                    bail!("Unfinished delta");
                }
                Ok(number | (number_reader.get_u8() as usize) << shift)
            });
    let bytes_read = data.remaining() - number_reader.remaining();
    data.advance(bytes_read);
    result
}
