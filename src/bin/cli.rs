//! Tiny CLI parser for the installer.
//!
//! We deliberately do not pull `clap` / `argh` into the installer — it
//! should be a single-binary airlifted into a clean environment.

use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
enum Subcommand {
    Install {
        dir: PathBuf,
        auto_start: bool,
    },
    Uninstall {
        dir: PathBuf,
    },
    Portable {
        dir: PathBuf,
    },
    Help,
}

pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("jacquewm-installer: {msg}\n");
            print_help();
            return ExitCode::from(2);
        }
    };
    match cmd {
        Subcommand::Help => {
            print_help();
            ExitCode::SUCCESS
        }
        Subcommand::Install { dir, auto_start } => match crate::install(dir, auto_start) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("jacquewm-installer: install failed: {e}");
                ExitCode::from(1)
            }
        },
        Subcommand::Portable { dir } => match crate::install_portable(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("jacquewm-installer: portable install failed: {e}");
                ExitCode::from(1)
            }
        },
        Subcommand::Uninstall { dir } => match crate::uninstall(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("jacquewm-installer: uninstall failed: {e}");
                ExitCode::from(1)
            }
        },
    }
}

fn parse(args: &[String]) -> Result<Subcommand, String> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Subcommand::Help);
    }

    // Default install root under %APPDATA%\JacqueWM.
    let default_dir = default_install_dir();

    let (cmd, rest) = match args.first().map(|s| s.as_str()) {
        Some("install") => ("install", &args[1..]),
        Some("uninstall") => ("uninstall", &args[1..]),
        Some("portable") => ("portable", &args[1..]),
        _ => return Err("first argument must be install|uninstall|portable".into()),
    };

    let mut dir: Option<PathBuf> = None;
    let mut auto_start: bool = false;

    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        match a.as_str() {
            "--dir" => {
                let v = rest.get(i + 1).ok_or("missing value after --dir")?;
                dir = Some(PathBuf::from(v));
                i += 2;
            }
            "--auto-start" => {
                auto_start = true;
                i += 1;
            }
            "--no-auto-start" => {
                auto_start = false;
                i += 1;
            }
            _ => return Err(format!("unknown flag: {a}")),
        }
    }

    let dir = dir.unwrap_or(default_dir);

    match cmd {
        "install" => Ok(Subcommand::Install { dir, auto_start }),
        "portable" => Ok(Subcommand::Portable { dir }),
        "uninstall" => Ok(Subcommand::Uninstall { dir }),
        _ => unreachable!(),
    }
}

fn default_install_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("JacqueWM")
}

fn print_help() {
    println!("jacquewm-installer — {0} {1}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_NAME"));
    println!();
    println!("USAGE:");
    println!("  jacquewm-installer install [--dir DIR] [--auto-start]");
    println!("  jacquewm-installer portable [--dir DIR]");
    println!("  jacquewm-installer uninstall [--dir DIR]");
    println!();
    println!("Subcommands:");
    println!("  install   Copy jacquewm.exe into --dir and (optionally) register HKCU auto-start.");
    println!("  portable  Copy jacquewm.exe into --dir. NO registry changes. Default mode.");
    println!("  uninstall Remove install dir + the auto-start HKCU value (if we wrote it).");
    println!();
    println!("Flags:");
    println!("  --dir DIR      Install/uninstall location. Defaults to %APPDATA%\\JacqueWM");
    println!("  --auto-start   Register HKCU\\...\\Run on install (default off; never required).");
    println!("  --help, -h     Print this message.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_minimal() {
        let r = parse(&["install".into()]);
        assert!(matches!(r, Ok(Subcommand::Install { auto_start: false, .. })));
    }

    #[test]
    fn parses_install_with_auto_start() {
        let r = parse(&["install".into(), "--auto-start".into()]);
        assert!(matches!(r, Ok(Subcommand::Install { auto_start: true, .. })));
    }

    #[test]
    fn parses_install_with_dir() {
        let r = parse(&["install".into(), "--dir".into(), "C:\\JacqueWM".into()]);
        if let Ok(Subcommand::Install { dir, auto_start }) = r {
            assert_eq!(dir.to_string_lossy(), "C:\\JacqueWM");
            assert!(!auto_start);
        } else {
            panic!();
        }
    }
}
