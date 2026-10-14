//! `data.acd`: Assetto Corsa's packed, lightly encrypted car data archive.
//!
//! Layout (little-endian):
//! - optional header: `i32 -1111`, `i32 <unknown>`
//! - repeated entries: `i32 name_len`, `name_len` name bytes,
//!   `i32 data_len`, then `data_len * 4` bytes where only every fourth byte
//!   carries data (the other three are zero).
//!
//! Each data byte is shifted by a repeating key derived from the car folder
//! name (see [`derive_key`]).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

const HEADER_MARKER: i32 = -1111;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcdEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// The string that seeds the key: the parent folder (car id) for files named
/// `data*.acd`, otherwise the file name itself.
pub fn key_source_for(acd_path: &Path) -> String {
    let file_name = acd_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if file_name.to_ascii_lowercase().starts_with("data") {
        acd_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        file_name
    }
}

fn int_to_byte(v: i32) -> u8 {
    (v.wrapping_rem(256).wrapping_add(256)).wrapping_rem(256) as u8
}

/// Derives the key exactly like `AcdFactory.CreateKey`: eight byte values
/// computed from the lowercased id, joined with `-`. The ASCII bytes of that
/// string are the repeating key.
pub fn derive_key(key_source: &str) -> Vec<u8> {
    // C# works on UTF-16 code units with wrapping 32-bit arithmetic.
    let s: Vec<i32> = key_source
        .to_lowercase()
        .encode_utf16()
        .map(i32::from)
        .collect();
    let len = s.len() as i32;
    let at = |i: i32| s[i as usize];

    let b1 = int_to_byte(s.iter().fold(0i32, |a, &c| a.wrapping_add(c)));

    let mut n = 0i32;
    let mut i = 0;
    while i < len - 1 {
        n = n.wrapping_mul(at(i)).wrapping_sub(at(i + 1));
        i += 2;
    }
    let b2 = int_to_byte(n);

    let mut n = 0i32;
    let mut i = 1;
    while i < len - 3 {
        n = n.wrapping_mul(at(i));
        n = n.wrapping_div(at(i + 1) + 27);
        n = n.wrapping_add(-27 - at(i - 1));
        i += 3;
    }
    let b3 = int_to_byte(n);

    let mut n = 5763i32;
    for i in 1..len {
        n = n.wrapping_sub(at(i));
    }
    let b4 = int_to_byte(n);

    let mut n = 66i32;
    let mut i = 1;
    while i < len - 4 {
        n = (at(i) + 15)
            .wrapping_mul(n)
            .wrapping_mul(at(i - 1) + 15)
            .wrapping_add(22);
        i += 4;
    }
    let b5 = int_to_byte(n);

    let mut n = 101i32;
    let mut i = 0;
    while i < len - 2 {
        n = n.wrapping_sub(at(i));
        i += 2;
    }
    let b6 = int_to_byte(n);

    let mut n = 171i32;
    let mut i = 0;
    while i < len - 2 {
        n = n.wrapping_rem(at(i));
        i += 2;
    }
    let b7 = int_to_byte(n);

    let mut n = 171i32;
    let mut i = 0;
    while i < len - 1 {
        n = n.wrapping_div(at(i)).wrapping_add(at(i + 1));
        i += 1;
    }
    let b8 = int_to_byte(n);

    [b1, b2, b3, b4, b5, b6, b7, b8]
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join("-")
        .into_bytes()
}

fn decrypt(data: &mut [u8], key: &[u8]) {
    if key.is_empty() {
        return;
    }
    for (b, k) in data.iter_mut().zip(key.iter().cycle()) {
        *b = b.wrapping_sub(*k);
    }
}

fn encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; data.len() * 4];
    if key.is_empty() {
        return out;
    }
    for (i, (b, k)) in data.iter().zip(key.iter().cycle()).enumerate() {
        out[i * 4] = b.wrapping_add(*k);
    }
    out
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn i32(&mut self) -> Result<i32> {
        let end = self.pos + 4;
        ensure!(end <= self.bytes.len(), "unexpected end of data.acd at {}", self.pos);
        let v = i32::from_le_bytes(self.bytes[self.pos..end].try_into().unwrap());
        self.pos = end;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("data.acd length overflow")?;
        ensure!(end <= self.bytes.len(), "unexpected end of data.acd at {}", self.pos);
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }
}

/// Decodes all entries of a packed archive. `key_source` is usually the car id.
pub fn read_bytes(bytes: &[u8], key_source: &str) -> Result<Vec<AcdEntry>> {
    let key = derive_key(key_source);
    let mut c = Cursor { bytes, pos: 0 };
    if bytes.len() >= 8 && c.i32()? == HEADER_MARKER {
        c.i32()?;
    } else {
        c.pos = 0;
    }

    let mut entries = Vec::new();
    while c.pos < bytes.len() {
        let name_len = c.i32()?;
        ensure!(name_len >= 0, "negative entry name length in data.acd");
        let name = String::from_utf8_lossy(c.take(name_len as usize)?).into_owned();
        let data_len = c.i32()?;
        ensure!(data_len >= 0, "negative entry length in data.acd for {name}");
        let raw = c.take(data_len as usize * 4)?;
        let mut data: Vec<u8> = raw.iter().step_by(4).copied().collect();
        decrypt(&mut data, &key);
        entries.push(AcdEntry { name, data });
    }
    Ok(entries)
}

pub fn read(acd_path: &Path) -> Result<Vec<AcdEntry>> {
    let bytes = fs::read(acd_path).with_context(|| format!("reading {}", acd_path.display()))?;
    read_bytes(&bytes, &key_source_for(acd_path))
}

/// Encodes entries in the headerless layout Content Manager writes.
pub fn write_bytes(entries: &[AcdEntry], key_source: &str) -> Result<Vec<u8>> {
    let key = derive_key(key_source);
    let mut out = Vec::new();
    for e in entries {
        ensure!(e.name.is_ascii(), "entry name must be ASCII: {}", e.name);
        out.extend_from_slice(&(e.name.len() as i32).to_le_bytes());
        out.extend_from_slice(e.name.as_bytes());
        let len = i32::try_from(e.data.len()).context("entry too large for data.acd")?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&encrypt(&e.data, &key));
    }
    Ok(out)
}

pub fn write(acd_path: &Path, entries: &[AcdEntry]) -> Result<()> {
    let bytes = write_bytes(entries, &key_source_for(acd_path))?;
    fs::write(acd_path, bytes).with_context(|| format!("writing {}", acd_path.display()))
}

/// Unpacks `data.acd` into `out_dir` (normally the car's `data` folder).
pub fn unpack(acd_path: &Path, out_dir: &Path) -> Result<usize> {
    let entries = read(acd_path)?;
    fs::create_dir_all(out_dir)?;
    for e in &entries {
        let name = Path::new(&e.name);
        if name.is_absolute() || name.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            bail!("refusing unsafe entry name {:?}", e.name);
        }
        let target = out_dir.join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &e.data)?;
    }
    Ok(entries.len())
}

/// Packs every file directly inside `data_dir` into `acd_path`.
pub fn pack(data_dir: &Path, acd_path: &Path) -> Result<usize> {
    let mut files: Vec<PathBuf> = fs::read_dir(data_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let entries = files
        .iter()
        .map(|p| {
            Ok(AcdEntry {
                name: p.file_name().unwrap().to_string_lossy().into_owned(),
                data: fs::read(p)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    write(acd_path, &entries)?;
    Ok(entries.len())
}
