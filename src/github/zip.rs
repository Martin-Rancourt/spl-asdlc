//! One file out of a zip archive, the form GitHub serves an artifact in:
//! the central directory read from the end, the entry stored or deflated.
//! Nothing else of the format: no ZIP64, no encryption, no multi-disk; an
//! archive that needs them is refused, never half-read.

use std::io::Read;

const EOCD: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;

/// Why a file could not be taken out of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipError {
    /// The archive is readable and holds no such file.
    Absent,
    /// The archive is not one this reader can read whole.
    Unreadable(String),
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 2)
        .map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn bad(what: &str) -> ZipError {
    ZipError::Unreadable(format!("not a readable zip: {what}"))
}

/// The bytes of the file named `name` (no leading `/`), at most `limit` of
/// them once inflated: a larger file is refused.
pub fn extract(archive: &[u8], name: &str, limit: u64) -> Result<Vec<u8>, ZipError> {
    // The end-of-central-directory record: 22 bytes plus a comment of at
    // most 65535, searched backwards.
    let floor = archive.len().saturating_sub(22 + 65_535);
    let eocd = (floor..=archive.len().saturating_sub(22))
        .rev()
        .find(|&i| u32_at(archive, i) == Some(EOCD))
        .ok_or_else(|| bad("no end of central directory"))?;
    let entries = u16_at(archive, eocd + 10).ok_or_else(|| bad("truncated"))?;
    let offset = u32_at(archive, eocd + 16).ok_or_else(|| bad("truncated"))?;
    if offset == u32::MAX || entries == usize::from(u16::MAX) {
        return Err(bad("ZIP64 archives are not read"));
    }
    let mut at = offset as usize;
    for _ in 0..entries {
        if u32_at(archive, at) != Some(CENTRAL) {
            return Err(bad("a central directory entry is broken"));
        }
        let field = |o: usize| u16_at(archive, at + o).ok_or_else(|| bad("truncated"));
        let flags = field(8)?;
        let method = field(10)?;
        let compressed = u32_at(archive, at + 20).ok_or_else(|| bad("truncated"))?;
        let size = u32_at(archive, at + 24).ok_or_else(|| bad("truncated"))?;
        let (name_len, extra_len, comment_len) = (field(28)?, field(30)?, field(32)?);
        let local = u32_at(archive, at + 42).ok_or_else(|| bad("truncated"))? as usize;
        let entry = archive
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| bad("truncated"))?;
        at += 46 + name_len + extra_len + comment_len;
        if entry != name.as_bytes() {
            continue;
        }
        if flags & 1 != 0 {
            return Err(bad("the file is encrypted"));
        }
        if compressed == u32::MAX || size == u32::MAX {
            return Err(bad("ZIP64 entries are not read"));
        }
        if u64::from(size) > limit {
            return Err(ZipError::Unreadable(format!(
                "{name} is {size} bytes, more than the {limit} read"
            )));
        }
        if u32_at(archive, local) != Some(LOCAL) {
            return Err(bad("a local header is broken"));
        }
        let skip = |o: usize| u16_at(archive, local + o).ok_or_else(|| bad("truncated"));
        let start = local + 30 + skip(26)? + skip(28)?;
        let data = archive
            .get(start..start + compressed as usize)
            .ok_or_else(|| bad("truncated"))?;
        let mut out = Vec::new();
        match method {
            0 => out.extend_from_slice(data),
            8 => {
                flate2::read::DeflateDecoder::new(data)
                    .take(limit + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| bad(&format!("the file does not inflate: {e}")))?;
            }
            m => return Err(bad(&format!("compression method {m} is not read"))),
        }
        if out.len() as u64 != u64::from(size) {
            return Err(bad("the file's size does not match its entry"));
        }
        return Ok(out);
    }
    Err(ZipError::Absent)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of `files`, each stored (`deflate = false`) or deflated.
    pub(crate) fn archive(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, content) in files {
            let data = if deflate {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(content).unwrap();
                e.finish().unwrap()
            } else {
                content.to_vec()
            };
            let method: u16 = if deflate { 8 } else { 0 };
            let local = out.len() as u32;
            out.extend_from_slice(&LOCAL.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&data);
            central.extend_from_slice(&CENTRAL.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&local.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn a_stored_or_deflated_file_is_taken_out_whole() {
        let files: &[(&str, &[u8])] = &[
            ("qa-report.md", b"# report"),
            ("results/suites.json", br#"{"failed":["tests"]}"#),
        ];
        for deflate in [false, true] {
            let zip = archive(files, deflate);
            assert_eq!(
                extract(&zip, "results/suites.json", 1024).unwrap(),
                br#"{"failed":["tests"]}"#.to_vec()
            );
            assert_eq!(
                extract(&zip, "results/other.json", 1024),
                Err(ZipError::Absent)
            );
        }
    }

    #[test]
    fn what_is_not_a_whole_zip_is_unreadable_never_absent() {
        assert!(matches!(
            extract(b"not a zip", "a", 10),
            Err(ZipError::Unreadable(_))
        ));
        let zip = archive(&[("a", b"abcdefghij")], false);
        assert!(matches!(
            extract(&zip, "a", 4),
            Err(ZipError::Unreadable(_))
        ));
        let cut = &zip[..zip.len() - 30];
        assert!(matches!(
            extract(cut, "a", 64),
            Err(ZipError::Unreadable(_))
        ));
    }
}
