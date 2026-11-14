//! Runs yt-dlp with command-line arguments and returns its output.
//!
//! - Desktop: a process. On macOS that's Catlify's bundled `ytdlp-apple` (github.com/Catlyn-Lab/ytdlp-apple,
//!   yt-dlp on an embedded Python: nothing to install), otherwise the user's own `yt-dlp`.
//! - iOS: in-process, through the same ytdlp-apple Swift package; the app registers it at launch
//!   (`stafy_set_ytdlp`, see media/YTDLPBridge.swift) because iOS can't start programs.
//!
//! Android doesn't use this: its bundled yt-dlp is driven from Kotlin (StafyYouTube.kt).

use anyhow::anyhow;

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Exit code of a run stopped with `cancel`.
pub const CANCELLED: i32 = 130;

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// yt-dlp's last "ERROR:" line, or `fallback`.
    pub fn error(&self, fallback: &str) -> anyhow::Error {
        let line = self.stderr.lines().rfind(|l| l.contains("ERROR")).map(|l| l.replace("ERROR: ", ""));
        anyhow!("{}", line.as_deref().unwrap_or(fallback).trim())
    }
}

pub use imp::{available, cancel, run};

/// yt-dlp's version, if it can run here.
pub fn version() -> Option<String> {
    let out = run(&["--version".into()], None, &mut |_, _| {}).ok()?;
    out.ok().then(|| out.stdout.trim().to_string())
}

#[cfg(desktop)]
mod imp {
    use super::{Output, CANCELLED};
    use anyhow::{Context, Result};
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    const INSTALL_HINT: &str = if cfg!(windows) {
        "yt-dlp is missing. Reinstall Catlify, or install it with: winget install yt-dlp.yt-dlp"
    } else {
        "yt-dlp is not installed. Install it with: brew install yt-dlp ffmpeg"
    };

    /// Runs started with an id: id → process id (for cancel).
    static JOBS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

    /// GUI apps on macOS don't get the shell's PATH, so also look where Homebrew/pipx install.
    pub fn find_tool(name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(Path::new(&home).join(".local/bin"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(Path::new(&local).join("Microsoft/WinGet/Links")); // winget
        }
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        dirs.into_iter().map(|d| d.join(&file)).find(|p| p.is_file())
    }

    /// A tool shipped next to Catlify's executable (release builds), e.g. `bundled("ffmpeg")`.
    pub fn bundled(name: &str) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        Some(exe.parent()?.join(file)).filter(|p| p.is_file())
    }

    /// yt-dlp shipped with Catlify: `ytdlp-apple` inside Catlify.app (scripts/ytdlp-apple.sh) or
    /// `yt-dlp.exe` next to Catlify.exe (release workflow).
    fn bundled_ytdlp() -> Option<PathBuf> {
        if cfg!(target_os = "macos") {
            let tool = bundled("ytdlp-apple")?;
            tool.parent()?.join("../Frameworks/Python.framework").exists().then_some(tool)
        } else {
            bundled("yt-dlp")
        }
    }

    pub fn tool() -> Option<PathBuf> {
        bundled_ytdlp().or_else(|| find_tool("yt-dlp"))
    }

    /// yt-dlp needs a JavaScript runtime for YouTube's challenges. ytdlp-apple has JavaScriptCore;
    /// Windows builds ship QuickJS (`qjs.exe`) for the standalone yt-dlp.exe.
    fn runtime_args(tool: &Path) -> Vec<String> {
        match bundled("qjs") {
            Some(qjs) if Some(tool) == bundled_ytdlp().as_deref() => {
                vec!["--js-runtimes".into(), format!("quickjs:{}", qjs.display())]
            }
            _ => vec![],
        }
    }

    pub fn available() -> bool {
        tool().is_some()
    }

    pub fn run(args: &[String], id: Option<&str>, on_line: &mut dyn FnMut(&str, bool)) -> Result<Output> {
        let tool = tool().ok_or_else(|| anyhow::anyhow!("{}", INSTALL_HINT))?;
        let mut command = Command::new(&tool);
        command.args(runtime_args(&tool)).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            // No console window flashing up for each yt-dlp run.
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("starting {}", tool.display()))?;
        if let Some(id) = id {
            JOBS.lock().get_or_insert_with(HashMap::new).insert(id.to_string(), child.id());
        }
        let mut err_pipe = child.stderr.take();
        let err_reader = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = err_pipe.as_mut() {
                let _ = e.read_to_string(&mut s);
            }
            s
        });
        let mut stdout = String::new();
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                on_line(&line, false);
                stdout.push_str(&line);
                stdout.push('\n');
            }
        }
        let status = child.wait().context("waiting for yt-dlp")?;
        let stderr = err_reader.join().unwrap_or_default();
        // Removed by cancel() = cancelled.
        let cancelled = id.is_some_and(|id| JOBS.lock().as_mut().and_then(|j| j.remove(id)).is_none());
        let code = if cancelled { CANCELLED } else { status.code().unwrap_or(1) };
        Ok(Output { code, stdout, stderr })
    }

    pub fn cancel(id: &str) {
        if let Some(pid) = JOBS.lock().as_mut().and_then(|j| j.remove(id)) {
            #[cfg(windows)]
            let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status();
            #[cfg(not(windows))]
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::Output;
    use anyhow::{anyhow, Result};
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::sync::OnceLock;

    /// `void line(void *context, const char *line, int is_stderr)`
    pub type LineFn = extern "C" fn(*mut c_void, *const c_char, c_int);
    /// `char *run(const char *request_json, line, context)`; request: `{"args": [...], "id"?}` or
    /// `{"cancel": id}`. Returns `{"code", "stdout", "stderr"}` (or `{"error"}`), freed with FreeFn.
    pub type RunFn = extern "C" fn(*const c_char, Option<LineFn>, *mut c_void) -> *mut c_char;
    pub type FreeFn = extern "C" fn(*mut c_char);

    static RUNNER: OnceLock<(RunFn, FreeFn)> = OnceLock::new();

    pub fn register(run: RunFn, free: FreeFn) {
        let _ = RUNNER.set((run, free));
    }

    pub fn available() -> bool {
        RUNNER.get().is_some()
    }

    extern "C" fn line_trampoline(context: *mut c_void, line: *const c_char, is_stderr: c_int) {
        if context.is_null() || line.is_null() {
            return;
        }
        let f = unsafe { &mut *(context as *mut &mut dyn FnMut(&str, bool)) };
        let text = unsafe { CStr::from_ptr(line) }.to_string_lossy();
        f(&text, is_stderr != 0);
    }

    fn call(request: &serde_json::Value, on_line: Option<&mut dyn FnMut(&str, bool)>) -> Result<serde_json::Value> {
        let (run, free) = RUNNER.get().ok_or_else(|| anyhow!("yt-dlp isn't available in this build"))?;
        let req = CString::new(request.to_string())?;
        let raw = match on_line {
            Some(f) => {
                let mut f: &mut dyn FnMut(&str, bool) = f;
                run(req.as_ptr(), Some(line_trampoline), &mut f as *mut _ as *mut c_void)
            }
            None => run(req.as_ptr(), None, std::ptr::null_mut()),
        };
        if raw.is_null() {
            return Err(anyhow!("yt-dlp returned nothing"));
        }
        let text = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        free(raw);
        let v: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            return Err(anyhow!("{e}"));
        }
        Ok(v)
    }

    pub fn run(args: &[String], id: Option<&str>, on_line: &mut dyn FnMut(&str, bool)) -> Result<Output> {
        let v = call(&serde_json::json!({ "args": args, "id": id }), Some(on_line))?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Ok(Output { code: v.get("code").and_then(|c| c.as_i64()).unwrap_or(1) as i32, stdout: s("stdout"), stderr: s("stderr") })
    }

    pub fn cancel(id: &str) {
        let _ = call(&serde_json::json!({ "cancel": id }), None);
    }
}

#[cfg(desktop)]
pub use imp::{bundled, find_tool};

#[cfg(target_os = "ios")]
pub use imp::{register, FreeFn, LineFn, RunFn};
