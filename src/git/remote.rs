use std::io::Read;

use anyhow::{anyhow, bail, Result};
use reqwest::{blocking::Client, header, StatusCode, Url};

type Sha1 = String;
type ReferenceName = String;
type Reference = (Sha1, ReferenceName);

const LENGTH_SIZE: usize = 4;

pub fn discover_references(git_url: &Url) -> Result<Vec<Reference>> {
    let url = git_url.join("info/refs?service=git-upload-pack")?;
    println!("Discover refs: {url}");
    let response = reqwest::blocking::get(url)?;

    if response.status() != StatusCode::OK && response.status() != StatusCode::NOT_MODIFIED {
        bail!(
            "Failed to discover references: unexpected status {}",
            response.status()
        );
    }
    if let Some(content_type) = response.headers().get(header::CONTENT_TYPE) {
        if content_type != "application/x-git-upload-pack-advertisement" {
            bail!("Wrong response content type {}", content_type.to_str()?);
        }
    }
    let content = response.text()?;
    let mut pkt_lines = content.lines();
    let first_line = parse_pkt_line(
        pkt_lines
            .next()
            .ok_or_else(|| anyhow!("Empty discovery response"))?,
    )?;
    if first_line != "# service=git-upload-pack" {
        bail!("Unexpected first discovery response line {first_line}")
    }
    let mut pkt_lines = pkt_lines.rev();
    let last_line = parse_pkt_line(
        pkt_lines
            .next()
            .ok_or_else(|| anyhow!("Discovery response with no terminator line"))?,
    )?;
    if last_line != "" {
        bail!("Unexpected last line {last_line}");
    }
    let mut refs = pkt_lines
        .rev()
        .enumerate()
        .map(|(i, p)| {
            let line = if i == 0 {
                p.get(4..)
                    .ok_or_else(|| anyhow!("First ref line in wrong format"))?
            } else {
                p
            };
            Ok(parse_pkt_line(line)?
                .split_once(' ')
                .map(|(hash, reference)| (hash.to_owned(), reference.to_owned()))
                .ok_or_else(|| anyhow!("Ref line in wrong format"))?)
        })
        .collect::<Result<Vec<_>>>()?;
    let first_pkt_line = refs
        .get_mut(0)
        .ok_or_else(|| anyhow!("Discovery response without capabilities line"))?;
    let capabilities_start = first_pkt_line
        .1
        .find('\0')
        .ok_or_else(|| anyhow!("Discovery response without capabilities"))?;
    let capabilities = first_pkt_line.1.split_off(capabilities_start);
    if !(capabilities.contains("allow-tip-sha1-in-want")
        || capabilities.contains("allow-reachable-sha1-in-want"))
    {
        bail!("Missing git server capabilities");
    }
    Ok(refs)
}

fn parse_pkt_line(data: &str) -> Result<String> {
    let expected_length = usize::from_str_radix(
        data.get(..4)
            .ok_or_else(|| anyhow!("Bad PKT length: {data}"))?,
        16,
    )?;
    // add 1 to account for the deleted newline character
    // handle special case for empty lines
    if (data.len() + 1 != expected_length) && (expected_length == 0 && data.len() != LENGTH_SIZE) {
        bail!(
            "Wrong encoded PKT length: expected {expected_length}, got {}",
            data.len()
        );
    }
    Ok(data[4..].to_owned())
}

pub fn fetch_pack(git_url: &Url, refs: &[Reference]) -> Result<Vec<u8>> {
    let request = refs
        .iter()
        .enumerate()
        .map(|(i, (sha, _))| {
            let want = if i == 0 {
                format!("want {sha} multi_ack\n")
            } else {
                format!("want {sha}\n")
            };
            format!("{:04x}{}", want.len() + LENGTH_SIZE, want)
        })
        .chain(std::iter::once("0000".to_owned()))
        .chain(std::iter::once("0009done\n".to_owned()))
        // join
        .fold(String::new(), |result, line| result + line.as_str());
    let url = git_url.join("git-upload-pack")?;
    let mut response = Client::new()
        .post(url)
        .header(
            header::CONTENT_TYPE,
            "application/x-git-upload-pack-request",
        )
        .body(request.to_owned())
        .send()?;
    let mut body: Vec<u8> = Vec::new();
    response.read_to_end(&mut body)?;
    const PACK_OFFSET: usize = 8;
    Ok(body
        .get(8..)
        .ok_or_else(|| anyhow!("Unexpected fetch response"))?
        .to_vec())
}
