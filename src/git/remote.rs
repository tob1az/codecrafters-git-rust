use anyhow::{anyhow, bail, Result};
use reqwest::{blocking::Client, header, StatusCode, Url};

type Sha1 = String;
type ReferenceName = String;
type Reference = (Sha1, ReferenceName);

const LENGTH_SIZE: usize = 4;

pub fn discover_references(git_url: &Url) -> Result<Vec<Reference>> {
    let url = git_url.join("info/refs?service=git-upload-pack")?;
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
    if last_line != "0000" {
        bail!("Unexpected last line {last_line}");
    }
    let mut refs_with_capabilities = pkt_lines
        .rev()
        .filter(|p| p.is_empty())
        .map(|p| {
            // remove null-delimited capabilities if any
            Ok(parse_pkt_line(p)?
                .split_once(' ')
                .map(|(hash, reference)| (hash.to_owned(), reference.to_owned()))
                .ok_or_else(|| anyhow!("Ref line in wrong format"))?)
        })
        .collect::<Result<Vec<_>>>()?;
    let first_pkt_line = refs_with_capabilities
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
    Ok(refs_with_capabilities)
}

fn parse_pkt_line(data: &str) -> Result<String> {
    let expected_length = usize::from_str_radix(
        data.get(..4)
            .ok_or_else(|| anyhow!("Bad PKT length: {data}"))?,
        16,
    )?;
    if data.len() != expected_length {
        bail!("Wrong encoded PKT length {expected_length}");
    }
    Ok(data[4..].to_owned())
}

pub fn fetch_refs(git_url: &Url, refs: &[Reference]) -> Result<Vec<u8>> {
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
        // join
        .fold(String::new(), |result, line| result + line.as_str());
    let url = git_url.join("git-upload-pack")?;
    let response = Client::new()
        .post(url)
        .header(
            header::CONTENT_TYPE,
            "application/x-git-upload-pack-request",
        )
        .body(request.to_owned())
        .send()?;

    const PACK_OFFSET: usize = 8;
    Ok(response.text()?.as_bytes()[8..].iter().copied().collect())
}
