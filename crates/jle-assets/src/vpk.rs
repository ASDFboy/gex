//! Valve Pak (.vpk) archives, versions 1 and 2: the `*_dir.vpk` file holds the
//! directory (and sometimes small files); the data lives in `*_000.vpk`, `*_001.vpk`...
//! Files are read on demand; the archive is never unpacked.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SIGNATURE: u32 = 0x55aa_1234;
const IN_DIR_FILE: u16 = 0x7fff;

#[derive(Clone, Debug)]
pub struct VpkEntry {
    pub crc: u32,
    pub preload: Vec<u8>,
    pub archive: u16,
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug)]
pub struct Vpk {
    /// The `_dir.vpk` path.
    pub dir_path: PathBuf,
    /// Where data stored in the dir file itself starts.
    data_start: u64,
    /// Lowercase "folder/name.ext" → entry.
    pub entries: BTreeMap<String, VpkEntry>,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Vpk {
    pub fn open(dir_path: &Path) -> Result<Vpk, String> {
        let mut f = File::open(dir_path).map_err(|e| format!("{}: {}", dir_path.display(), e))?;
        let mut head = [0u8; 28];
        f.read_exact(&mut head[..12]).map_err(|e| e.to_string())?;
        if u32le(&head, 0) != SIGNATURE {
            return Err(format!("{} isn't a VPK directory file", dir_path.display()));
        }
        let version = u32le(&head, 4);
        let tree_size = u32le(&head, 8) as usize;
        let header_len = match version {
            1 => 12,
            2 => {
                f.read_exact(&mut head[12..28]).map_err(|e| e.to_string())?;
                28
            }
            v => return Err(format!("VPK version {} isn't supported", v)),
        };
        let mut tree = vec![0u8; tree_size];
        f.read_exact(&mut tree).map_err(|e| format!("VPK directory is cut short: {}", e))?;
        let entries = parse_tree(&tree)?;
        Ok(Vpk { dir_path: dir_path.to_path_buf(), data_start: (header_len + tree_size) as u64, entries })
    }

    fn archive_path(&self, index: u16) -> PathBuf {
        let name = self.dir_path.file_name().unwrap().to_string_lossy();
        let stem = name.strip_suffix("_dir.vpk").unwrap_or(&name);
        self.dir_path.with_file_name(format!("{}_{:03}.vpk", stem, index))
    }

    pub fn read(&self, path: &str) -> Option<Result<Vec<u8>, String>> {
        let e = self.entries.get(&path.to_ascii_lowercase().replace('\\', "/"))?;
        Some(self.read_entry(e))
    }

    fn read_entry(&self, e: &VpkEntry) -> Result<Vec<u8>, String> {
        let mut out = e.preload.clone();
        if e.length > 0 {
            let (file, at) = if e.archive == IN_DIR_FILE {
                (self.dir_path.clone(), self.data_start + e.offset as u64)
            } else {
                (self.archive_path(e.archive), e.offset as u64)
            };
            let mut f = File::open(&file).map_err(|err| format!("{}: {}", file.display(), err))?;
            f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
            let start = out.len();
            out.resize(start + e.length as usize, 0);
            f.read_exact(&mut out[start..]).map_err(|err| format!("{}: {}", file.display(), err))?;
        }
        Ok(out)
    }
}

fn parse_tree(b: &[u8]) -> Result<BTreeMap<String, VpkEntry>, String> {
    let mut i = 0;
    let cstr = |i: &mut usize| -> Result<String, String> {
        let start = *i;
        while *i < b.len() && b[*i] != 0 {
            *i += 1;
        }
        if *i >= b.len() {
            return Err("VPK directory is damaged".into());
        }
        let s = String::from_utf8_lossy(&b[start..*i]).into_owned();
        *i += 1;
        Ok(s)
    };
    let mut out = BTreeMap::new();
    loop {
        let ext = cstr(&mut i)?;
        if ext.is_empty() {
            break;
        }
        loop {
            let dir = cstr(&mut i)?;
            if dir.is_empty() {
                break;
            }
            loop {
                let name = cstr(&mut i)?;
                if name.is_empty() {
                    break;
                }
                if i + 18 > b.len() {
                    return Err("VPK directory is damaged".into());
                }
                let crc = u32le(b, i);
                let preload_len = u16le(b, i + 4) as usize;
                let archive = u16le(b, i + 6);
                let offset = u32le(b, i + 8);
                let length = u32le(b, i + 12);
                i += 18; // includes the 0xffff terminator
                let preload = b.get(i..i + preload_len).ok_or("VPK directory is damaged")?.to_vec();
                i += preload_len;
                let mut full = String::new();
                if dir != " " {
                    full.push_str(&dir);
                    full.push('/');
                }
                full.push_str(&name);
                if ext != " " {
                    full.push('.');
                    full.push_str(&ext);
                }
                out.insert(full.to_ascii_lowercase(), VpkEntry { crc, preload, archive, offset, length });
            }
        }
    }
    Ok(out)
}

/// Writes a small single-file VPK (version 1, data in the dir file). For tests and tools.
pub fn write_simple(path: &Path, files: &[(&str, &[u8])]) -> std::io::Result<()> {
    let mut by_ext: BTreeMap<String, BTreeMap<String, Vec<(String, &[u8])>>> = BTreeMap::new();
    for (p, data) in files {
        let (dir, file) = p.rsplit_once('/').unwrap_or((" ", p));
        let (name, ext) = file.rsplit_once('.').unwrap_or((file, " "));
        by_ext.entry(ext.into()).or_default().entry(dir.into()).or_default().push((name.into(), data));
    }
    let mut tree = Vec::new();
    let mut data = Vec::new();
    for (ext, dirs) in &by_ext {
        tree.extend_from_slice(ext.as_bytes());
        tree.push(0);
        for (dir, names) in dirs {
            tree.extend_from_slice(dir.as_bytes());
            tree.push(0);
            for (name, bytes) in names {
                tree.extend_from_slice(name.as_bytes());
                tree.push(0);
                tree.extend_from_slice(&0u32.to_le_bytes());
                tree.extend_from_slice(&0u16.to_le_bytes());
                tree.extend_from_slice(&IN_DIR_FILE.to_le_bytes());
                tree.extend_from_slice(&(data.len() as u32).to_le_bytes());
                tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                tree.extend_from_slice(&0xffffu16.to_le_bytes());
                data.extend_from_slice(bytes);
            }
            tree.push(0);
        }
        tree.push(0);
    }
    tree.push(0);
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    out.extend_from_slice(&tree);
    out.extend_from_slice(&data);
    std::fs::write(path, out)
}
