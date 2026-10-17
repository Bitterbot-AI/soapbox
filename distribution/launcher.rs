#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(not(windows))]
use std::path::PathBuf;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};
#[cfg(windows)]
fn hidden(c: &mut Command) {
    c.creation_flags(0x08000000);
}
#[cfg(not(windows))]
fn hidden(_: &mut Command) {}
#[cfg(windows)]
fn message(text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(w: *mut std::ffi::c_void, t: *const u16, c: *const u16, f: u32) -> i32;
    }
    let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let c: Vec<u16> = "Soapbox Setup".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10);
    }
}
#[cfg(not(windows))]
fn message(text: &str) {
    eprintln!("{text}");
    let _ = Command::new("zenity")
        .args(["--error", "--title=Soapbox Setup", "--text", text])
        .status();
}
#[cfg(windows)]
fn browse(url: &str) -> Result<(), String> {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            w: *mut std::ffi::c_void,
            o: *const u16,
            f: *const u16,
            p: *const u16,
            d: *const u16,
            s: i32,
        ) -> isize;
    }
    let u: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let o: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            o.as_ptr(),
            u.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if r <= 32 {
        Err("Could not open your default browser".into())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn browse(url: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| "Could not open your default browser".into())
}
fn run() -> Result<(), String> {
    let root = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Release folder missing")?
        .to_path_buf();
    let script = root.join("setup/installer-server.mjs");
    if !script.is_file() {
        return Err("Extract the entire Soapbox download before opening Setup.".into());
    }
    let mut command;
    #[cfg(windows)]
    {
        let mut convert = Command::new("wsl.exe");
        convert.args(["--exec", "wslpath", "-u"]).arg(&script);
        hidden(&mut convert);
        let output=convert.output().map_err(|_|"WSL2 Ubuntu is required. Install Bitterbot 1.8.0 in your default Ubuntu distribution first.")?;
        if !output.status.success() {
            return Err("Could not access the release from WSL. Start your default Ubuntu distribution and try again.".into());
        }
        let linux = String::from_utf8(output.stdout).map_err(|_| "Invalid WSL path")?;
        command = Command::new("wsl.exe");
        command.args([
            "--exec",
            "sh",
            "-c",
            "exec \"$HOME/.bitterbot/app/current/runtime/node/bin/node\" \"$@\"",
            "soapbox-installer",
            linux.trim(),
        ]);
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
        command =
            Command::new(PathBuf::from(home).join(".bitterbot/app/current/runtime/node/bin/node"));
        command.arg(script);
    }
    // Optional state override is for testing another local profile; it is never supplied by a web page.
    let extra: Vec<String> = std::env::args().skip(1).collect();
    if !extra.is_empty() {
        if extra.len() != 2 || extra[0] != "--state" || !extra[1].starts_with('/') {
            return Err("Only --state /absolute/linux/profile is supported".into());
        }
        command.args(extra);
    }
    hidden(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| {
        "Install and open Bitterbot 1.8.0 first. Its bundled Node runtime is required."
    })?;
    let mut line = String::new();
    BufReader::new(child.stdout.take().ok_or("Installer did not start")?)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let url = line.trim();
    if !url.starts_with("http://127.0.0.1:") || !url.contains("/#") || url.contains(['\r', '\n']) {
        let _ = child.kill();
        return Err("Setup could not start. Check that Bitterbot 1.8.0 is installed in Ubuntu 24.04 (your default WSL distribution on Windows), then extract the full download and retry.".into());
    }
    if let Err(e) = browse(url) {
        let _ = child.kill();
        return Err(e);
    }
    let _ = child.wait();
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        message(&e);
        std::process::exit(1);
    }
}
