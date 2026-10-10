//! Order-preserving INI files in the dialect Assetto Corsa uses.
//!
//! Section and key names are matched case-sensitively (AC writes them in
//! upper case). Comments after `;` or `//` are dropped on read.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    entries: Vec<(String, String)>,
}

impl Section {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), entries: Vec::new() }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, key: &str, value: impl ToString) -> &mut Self {
        let value = value.to_string();
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key.to_owned(), value)),
        }
        self
    }

    pub fn set_bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.set(key, if value { 1 } else { 0 })
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct IniFile {
    sections: Vec<Section>,
}

fn strip_comment(line: &str) -> &str {
    let semi = line.find(';');
    let slashes = line.find("//");
    match (semi, slashes) {
        (Some(a), Some(b)) => &line[..a.min(b)],
        (Some(a), None) | (None, Some(a)) => &line[..a],
        (None, None) => line,
    }
}

impl IniFile {
    pub fn parse(text: &str) -> Self {
        let mut file = IniFile::default();
        let mut current: Option<usize> = None;
        for raw in text.trim_start_matches('\u{feff}').lines() {
            let line = raw.trim();
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    let name = line[1..end].trim().to_owned();
                    current = Some(file.section_index(&name));
                    continue;
                }
            }
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            if let (Some(idx), Some(eq)) = (current, line.find('=')) {
                let key = line[..eq].trim();
                let value = line[eq + 1..].trim();
                file.sections[idx].set(key, value);
            }
        }
        file
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Loads the file, or returns an empty one if it does not exist.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() { Self::load(path) } else { Ok(Self::default()) }
    }

    fn section_index(&mut self, name: &str) -> usize {
        match self.sections.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => {
                self.sections.push(Section::new(name));
                self.sections.len() - 1
            }
        }
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Returns the named section, creating it at the end if missing.
    pub fn section_mut(&mut self, name: &str) -> &mut Section {
        let i = self.section_index(name);
        &mut self.sections[i]
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.section(section)?.get(key)
    }

    pub fn remove_section(&mut self, name: &str) {
        self.sections.retain(|s| s.name != name);
    }

    /// Removes `PREFIX_n` sections for every `n >= from`.
    pub fn remove_numbered(&mut self, prefix: &str, from: u32) {
        self.sections.retain(|s| {
            s.name
                .strip_prefix(prefix)
                .and_then(|r| r.strip_prefix('_'))
                .and_then(|n| n.parse::<u32>().ok())
                .is_none_or(|n| n < from)
        });
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        for (i, s) in self.sections.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let _ = writeln!(out, "[{}]", s.name);
            for (k, v) in &s.entries {
                let _ = writeln!(out, "{k}={v}");
            }
        }
        out
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_string()).with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ac_style() {
        let ini = IniFile::parse(
            "[LIGHTING] \nSUN_ANGLE=-48\n[CAR_0]\nNATIONALITY=ITA\t\t\t; ISO Country code\n[CAR_1]\nMODEL=x\n",
        );
        assert_eq!(ini.get("LIGHTING", "SUN_ANGLE"), Some("-48"));
        assert_eq!(ini.get("CAR_0", "NATIONALITY"), Some("ITA"));
        let mut ini = ini;
        ini.remove_numbered("CAR", 1);
        assert!(ini.section("CAR_1").is_none());
        assert!(ini.section("CAR_0").is_some());
    }
}
