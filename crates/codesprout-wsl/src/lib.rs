//! Protocol between CodeSprout on Windows and its agent inside a WSL distro.
//!
//! The app starts the agent with `wsl.exe -d <distro> --cd ~ --exec <agent>` and talks to it over
//! its stdin/stdout. Each frame is a JSON header followed by a binary payload, both prefixed with
//! their length (u32, little endian), so git's output travels byte for byte.
//! Requests carry an id; the agent answers them in any order, so many can be in flight.

use std::io::{self, Read, Write};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    /// Runs git in `cwd`. The payload is its stdin.
    Run { id: u64, cwd: String, args: Vec<String>, env: Vec<(String, String)>, max_buffer: usize, progress: bool },
    /// Cancels a `Run` (kills git and everything it started).
    Kill { id: u64 },
    /// Watches a folder recursively; changes arrive as `Changed { id }`.
    Watch { id: u64, path: String },
    Unwatch { id: u64 },
    /// Moves files or folders to the distro's trash.
    Trash { id: u64, paths: Vec<String> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    /// First frame after starting.
    Hello(Hello),
    /// A line git wrote to stderr, while `progress` was requested.
    Progress { id: u64, line: String },
    /// Git finished. The payload is its stdout.
    Done { id: u64, code: Option<i32>, stderr: String, truncated: bool },
    /// The request could not run at all.
    Failed { id: u64, message: String },
    Changed { id: u64 },
    Trashed { id: u64, error: Option<String> },
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Hello {
    pub version: String,
    /// `$HOME` of the default user.
    pub home: String,
    /// Where Windows drives are mounted, with a trailing slash (`/mnt/`).
    pub mount: String,
    /// `git --version`, or `None` if git is not installed.
    pub git: Option<String>,
    /// Whether git has a `credential.helper` it can actually run (see [`usable_helper`]).
    pub credential_helper: bool,
}

const MAX_HEADER: usize = 16 * 1024 * 1024;

pub fn write_frame<T: Serialize>(w: &mut impl Write, header: &T, payload: &[u8]) -> io::Result<()> {
    let h = serde_json::to_vec(header)?;
    let mut buf = Vec::with_capacity(8 + h.len() + payload.len());
    buf.extend_from_slice(&(h.len() as u32).to_le_bytes());
    buf.extend_from_slice(&h);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    w.write_all(&buf)?;
    w.flush()
}

/// Reads one frame; `Ok(None)` at the end of the stream.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<Option<(T, Vec<u8>)>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_HEADER {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "cabecera demasiado grande"));
    }
    let mut h = vec![0u8; n];
    r.read_exact(&mut h)?;
    r.read_exact(&mut len)?;
    let mut payload = vec![0u8; u32::from_le_bytes(len) as usize];
    r.read_exact(&mut payload)?;
    let header = serde_json::from_slice(&h).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some((header, payload)))
}

/// Whether any of the configured `credential.helper` values (in order, as `git config --get-all`
/// prints them) can run. A helper that is configured but not installed, like the common
/// `libsecret` without its binary, only makes git print an error and fall back to a password
/// prompt, so it does not count. `program_exists` answers for `git-credential-<name>` or a path.
pub fn usable_helper<'a>(values: impl IntoIterator<Item = &'a str>, program_exists: impl Fn(&str) -> bool) -> bool {
    let mut usable = false;
    for v in values {
        let v = v.trim();
        let program = v.split_whitespace().next().unwrap_or("");
        usable = match v {
            // an empty value resets the list
            "" => false,
            _ if v.starts_with('!') => true,
            _ if program.starts_with('/') => usable || program_exists(program),
            _ => usable || program_exists(&format!("git-credential-{program}")),
        };
    }
    usable
}

/// Changes under these paths don't affect what the app shows (git's own bookkeeping).
pub fn is_relevant_change(path: &str) -> bool {
    let s = path.replace('\\', "/");
    !(s.contains("/.git/objects") || s.contains("/.git/logs") || s.ends_with(".lock") || s.contains("/.git/codesprout"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_with_binary_payload() {
        let mut buf = Vec::new();
        let req = Request::Run { id: 7, cwd: "/home/a b".into(), args: vec!["log".into(), "ñ".into()], env: vec![], max_buffer: 0, progress: true };
        write_frame(&mut buf, &req, &[0, 255, 10, 13]).unwrap();
        write_frame(&mut buf, &Event::Changed { id: 3 }, &[]).unwrap();
        let mut r = buf.as_slice();
        let (back, payload): (Request, _) = read_frame(&mut r).unwrap().unwrap();
        assert!(matches!(back, Request::Run { id: 7, ref cwd, .. } if cwd == "/home/a b"));
        assert_eq!(payload, [0, 255, 10, 13]);
        let (ev, _): (Event, _) = read_frame(&mut r).unwrap().unwrap();
        assert!(matches!(ev, Event::Changed { id: 3 }));
        assert!(read_frame::<Event>(&mut r).unwrap().is_none());
    }

    #[test]
    fn only_helpers_that_run_count() {
        let installed = |p: &str| ["git-credential-store", "git-credential-cache", "/usr/bin/pass-helper"].contains(&p);
        assert!(!usable_helper([], installed));
        assert!(!usable_helper(["libsecret"], installed));
        assert!(usable_helper(["libsecret", "store"], installed));
        assert!(usable_helper(["cache --timeout=3600"], installed));
        assert!(usable_helper(["!f() { echo password=x; }; f"], installed));
        assert!(usable_helper(["/usr/bin/pass-helper"], installed));
        assert!(!usable_helper(["/mnt/c/missing.exe"], installed));
        // an empty value drops the helpers before it
        assert!(!usable_helper(["store", ""], installed));
        assert!(usable_helper(["", "store"], installed));
    }

    #[test]
    fn relevant_changes() {
        assert!(is_relevant_change("/home/u/repo/src/main.rs"));
        assert!(is_relevant_change("/home/u/repo/.git/index"));
        assert!(!is_relevant_change("/home/u/repo/.git/objects/ab/cdef"));
        assert!(!is_relevant_change("/home/u/repo/.git/index.lock"));
    }
}
