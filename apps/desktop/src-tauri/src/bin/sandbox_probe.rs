use std::env;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().collect();
    match arguments.get(1).and_then(|value| value.to_str()) {
        Some("filesystem") if arguments.len() == 4 => {
            filesystem_probe(Path::new(&arguments[2]), Path::new(&arguments[3]))
        }
        Some("network") if arguments.len() == 3 => network_probe(&arguments[2]),
        Some("spawn-child") if arguments.len() == 2 => spawn_child_probe(),
        Some("wait") if arguments.len() == 2 => {
            std::thread::sleep(Duration::from_secs(60));
            ExitCode::from(90)
        }
        Some("flood") if arguments.len() == 2 => flood_probe(),
        _ => ExitCode::from(99),
    }
}

fn flood_probe() -> ExitCode {
    let chunk = vec![b'x'; 8192];
    for _ in 0..32 {
        if std::io::stdout().write_all(&chunk).is_err()
            || std::io::stderr().write_all(&chunk).is_err()
        {
            return ExitCode::from(88);
        }
    }
    ExitCode::SUCCESS
}

fn filesystem_probe(allowed: &Path, denied: &Path) -> ExitCode {
    if fs::write(allowed, b"sandbox write proof").is_err() {
        return ExitCode::from(81);
    }
    if fs::read(denied).is_ok() {
        return ExitCode::from(82);
    }
    ExitCode::SUCCESS
}

fn network_probe(address: &std::ffi::OsStr) -> ExitCode {
    let Some(address) = address.to_str() else {
        return ExitCode::from(83);
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        return ExitCode::from(84);
    };
    match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
        Ok(_) => ExitCode::from(85),
        Err(_) => ExitCode::SUCCESS,
    }
}

fn spawn_child_probe() -> ExitCode {
    let Ok(executable) = env::current_exe() else {
        return ExitCode::from(86);
    };
    match std::process::Command::new(executable).arg("wait").spawn() {
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            ExitCode::from(87)
        }
        Err(_) => ExitCode::SUCCESS,
    }
}
