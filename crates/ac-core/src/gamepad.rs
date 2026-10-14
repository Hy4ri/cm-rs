//! Reads gamepad button presses from Linux evdev, to bind them in AC.
//!
//! Wine/Proton exposes pads to AC as XInput, so buttons are reported by
//! their XInput names (Cross = `A`, Circle = `B`, ...), which is what
//! `controls.ini` stores in `XBOXBUTTON`.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Result, bail};

const BTN_SOUTH: usize = 0x130;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;
const O_NONBLOCK: i32 = 0o4000;
/// `struct input_event`: a `timeval` (two longs), then u16 type, u16 code, i32 value.
const EVENT_SIZE: usize = 2 * size_of::<usize>() + 8;

/// Whether a `capabilities/key` bitmap (hex longs, most significant first) has `bit`.
fn has_key(bitmap: &str, bit: usize) -> bool {
    let bits = usize::BITS as usize;
    let words: Vec<&str> = bitmap.split_whitespace().rev().collect();
    words
        .get(bit / bits)
        .and_then(|w| usize::from_str_radix(w, 16).ok())
        .is_some_and(|w| w & (1 << (bit % bits)) != 0)
}

/// `/dev/input/event*` devices that have gamepad face buttons.
pub fn devices() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir("/sys/class/input").into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            continue;
        }
        let dev = entry.path().join("device");
        let keys = fs::read_to_string(dev.join("capabilities/key")).unwrap_or_default();
        if has_key(&keys, BTN_SOUTH) {
            let label = fs::read_to_string(dev.join("name")).unwrap_or_default().trim().to_owned();
            out.push((PathBuf::from("/dev/input").join(name), label));
        }
    }
    out
}

fn button_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x130 => "A",
        0x131 => "B",
        0x133 => "Y",
        0x134 => "X",
        0x136 => "LSHOULDER",
        0x137 => "RSHOULDER",
        0x13a => "BACK",
        0x13b => "START",
        0x13d => "LEFT_THUMB",
        0x13e => "RIGHT_THUMB",
        // AC's dpad codes when a driver reports it as buttons.
        0x220 => "DPAD_UP",
        0x221 => "DPAD_DOWN",
        0x222 => "DPAD_LEFT",
        0x223 => "DPAD_RIGHT",
        _ => return None,
    })
}

/// What a press turned into.
pub enum Press {
    Button(&'static str),
    /// A button AC can't bind (triggers are throttle/brake, PS is Steam's).
    Unbindable(u16),
}

pub struct Reader {
    files: Vec<File>,
    pub names: Vec<String>,
}

impl Reader {
    pub fn open() -> Result<Self> {
        let mut files = Vec::new();
        let mut names = Vec::new();
        let mut denied = None;
        for (path, name) in devices() {
            match OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(&path) {
                Ok(f) => {
                    files.push(f);
                    names.push(name);
                }
                Err(e) => denied = Some(format!("{}: {e}", path.display())),
            }
        }
        if files.is_empty() {
            match denied {
                Some(e) => bail!("can't read the controller ({e}); add yourself to the input group"),
                None => bail!("no controller found; connect it and try again"),
            }
        }
        Ok(Self { files, names })
    }

    /// Returns the first new press since the last call, without blocking.
    pub fn poll(&mut self) -> Option<Press> {
        let mut buf = [0u8; EVENT_SIZE * 64];
        for f in &mut self.files {
            while let Ok(n) = f.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for ev in buf[..n].as_chunks::<EVENT_SIZE>().0 {
                    let t = &ev[EVENT_SIZE - 8..];
                    let kind = u16::from_ne_bytes([t[0], t[1]]);
                    let code = u16::from_ne_bytes([t[2], t[3]]);
                    let value = i32::from_ne_bytes([t[4], t[5], t[6], t[7]]);
                    let press = match (kind, code, value) {
                        (EV_KEY, c, 1) => Some(button_name(c).map(Press::Button).unwrap_or(Press::Unbindable(c))),
                        (EV_ABS, ABS_HAT0X, -1) => Some(Press::Button("DPAD_LEFT")),
                        (EV_ABS, ABS_HAT0X, 1) => Some(Press::Button("DPAD_RIGHT")),
                        (EV_ABS, ABS_HAT0Y, -1) => Some(Press::Button("DPAD_UP")),
                        (EV_ABS, ABS_HAT0Y, 1) => Some(Press::Button("DPAD_DOWN")),
                        _ => None,
                    };
                    if press.is_some() {
                        return press;
                    }
                }
            }
        }
        None
    }
}
