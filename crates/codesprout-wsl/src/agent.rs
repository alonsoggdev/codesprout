//! Reads requests from stdin and runs each one on its own thread; answers share stdout.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use codesprout_wsl::{is_relevant_change, read_frame, write_frame, Event, Hello, Request};

type Out = Arc<Mutex<io::Stdout>>;

fn send(out: &Out, ev: &Event, payload: &[u8]) {
    // the app went away if this fails; stdin's EOF ends the agent
    let _ = write_frame(&mut *out.lock().unwrap(), ev, payload);
}

pub fn run() {
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    send(&out, &Event::Hello(hello()), &[]);

    // process group of each running git, to cancel it with everything it started
    let running: Arc<Mutex<HashMap<u64, i32>>> = Arc::default();
    let mut watchers: HashMap<u64, notify::RecommendedWatcher> = HashMap::new();
    let mut stdin = io::stdin().lock();
    loop {
        let (req, payload) = match read_frame::<Request>(&mut stdin) {
            Ok(Some(f)) => f,
            Ok(None) | Err(_) => break,
        };
        match req {
            Request::Run { id, cwd, args, env, max_buffer, progress } => {
                let (out, running) = (out.clone(), running.clone());
                thread::spawn(move || run_git(&out, &running, id, &cwd, &args, &env, payload, max_buffer, progress));
            }
            Request::Kill { id } => {
                if let Some(pgid) = running.lock().unwrap().get(&id) {
                    unsafe { libc::kill(-pgid, libc::SIGTERM) };
                }
            }
            Request::Watch { id, path } => match watch(&out, id, &path) {
                Ok(w) => {
                    watchers.insert(id, w);
                }
                Err(e) => send(&out, &Event::Failed { id, message: e.to_string() }, &[]),
            },
            Request::Unwatch { id } => {
                watchers.remove(&id);
            }
            Request::Trash { id, paths } => {
                let out = out.clone();
                thread::spawn(move || {
                    let error = trash::delete_all(&paths).err().map(|e| e.to_string());
                    send(&out, &Event::Trashed { id, error }, &[]);
                });
            }
        }
    }
    for pgid in running.lock().unwrap().values() {
        unsafe { libc::kill(-pgid, libc::SIGTERM) };
    }
}

fn hello() -> Hello {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let output = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .current_dir(&home)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    // "/mnt/c/" → "/mnt/"; it changes when wsl.conf sets automount.root
    let mount = output("wslpath", &["-u", "C:\\"])
        .and_then(|p| p.strip_suffix("c/").or_else(|| p.strip_suffix('c')).map(str::to_string))
        .filter(|p| p.starts_with('/'))
        .unwrap_or_else(|| "/mnt/".into());
    Hello {
        version: env!("CARGO_PKG_VERSION").into(),
        git: output("git", &["--version"]),
        credential_helper: output("git", &["config", "--get-all", "credential.helper"]).is_some_and(|s| !s.is_empty()),
        mount: if mount.ends_with('/') { mount } else { format!("{mount}/") },
        home,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_git(out: &Out, running: &Mutex<HashMap<u64, i32>>, id: u64, cwd: &str, args: &[String], env: &[(String, String)], input: Vec<u8>, max: usize, progress: bool) {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let message = if e.kind() == io::ErrorKind::NotFound && !std::path::Path::new(cwd).is_dir() {
                format!("La carpeta no existe: {cwd}")
            } else if e.kind() == io::ErrorKind::NotFound {
                "Git no está instalado en esta distribución de WSL.".into()
            } else {
                e.to_string()
            };
            send(out, &Event::Failed { id, message }, &[]);
            return;
        }
    };
    running.lock().unwrap().insert(id, child.id() as i32);

    let mut stdin = child.stdin.take().expect("stdin");
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stderr = child.stderr.take().expect("stderr");
    let progress_out = progress.then(|| out.clone());
    let err_reader = thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    all.extend_from_slice(&buf[..n]);
                    if let Some(o) = &progress_out {
                        for line in String::from_utf8_lossy(&buf[..n]).split(['\r', '\n']) {
                            let t = line.trim();
                            if !t.is_empty() {
                                send(o, &Event::Progress { id, line: t.to_string() }, &[]);
                            }
                        }
                    }
                }
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    });

    let mut stdout = child.stdout.take().expect("stdout");
    let mut data = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 65536];
    loop {
        match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if max > 0 && data.len() + n > max {
                    let room = max.saturating_sub(data.len());
                    data.extend_from_slice(&buf[..room]);
                    truncated = true;
                } else if !truncated {
                    data.extend_from_slice(&buf[..n]);
                }
            }
        }
    }
    let code = child.wait().ok().and_then(|s| s.code());
    running.lock().unwrap().remove(&id);
    let _ = writer.join();
    let stderr = err_reader.join().unwrap_or_default();
    send(out, &Event::Done { id, code, stderr, truncated }, &data);
}

/// Recursive watcher that reports at most one change every 250 ms.
fn watch(out: &Out, id: u64, path: &str) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let out = out.clone();
    let last = Mutex::new(Instant::now() - Duration::from_secs(1));
    let pending = Arc::new(Mutex::new(false));
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if !ev.paths.iter().any(|p| is_relevant_change(&p.to_string_lossy())) {
            return;
        }
        let mut last = last.lock().unwrap();
        if last.elapsed() >= Duration::from_millis(250) {
            *last = Instant::now();
            send(&out, &Event::Changed { id }, &[]);
        } else if !std::mem::replace(&mut *pending.lock().unwrap(), true) {
            // a burst: one more notice once it settles, so the last change is not missed
            let (out, pending) = (out.clone(), pending.clone());
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(250));
                *pending.lock().unwrap() = false;
                send(&out, &Event::Changed { id }, &[]);
            });
        }
    })?;
    w.watch(std::path::Path::new(path), notify::RecursiveMode::Recursive)?;
    Ok(w)
}
