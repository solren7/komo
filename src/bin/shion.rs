//! Deprecated compatibility entrypoint for source-installed upgrades.
//!
//! `cargo install` installs every binary target. Keeping this small forwarder
//! means an existing `shion upgrade` can pull the renamed package, replace its
//! old executable, and restart into the new `komo` implementation. Release
//! archives intentionally contain only `komo`.

use std::path::PathBuf;
use std::process::Command;

fn komo_executable() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!("komo{}", std::env::consts::EXE_SUFFIX)))
}

#[cfg(unix)]
fn main() {
    use std::os::unix::process::CommandExt;

    eprintln!("`shion` was renamed to `komo`; forwarding this command");
    let error = match komo_executable() {
        Ok(exe) => Command::new(exe).args(std::env::args_os().skip(1)).exec(),
        Err(error) => error,
    };
    eprintln!("could not start `komo`: {error}");
    std::process::exit(1);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("`shion` was renamed to `komo`; forwarding this command");
    let status = komo_executable()
        .and_then(|exe| Command::new(exe).args(std::env::args_os().skip(1)).status());
    match status {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("could not start `komo`: {error}");
            std::process::exit(1);
        }
    }
}
