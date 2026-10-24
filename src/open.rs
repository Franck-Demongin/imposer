use std::io;
use std::path::Path;
use std::process::Command;

/// Opens `path` with the platform's default handler (file or directory).
///
/// On Linux this relies on `xdg-open`; on Windows it uses the shell's
/// `start` command, which opens files with their associated application and
/// folders in Explorer. On macOS it uses `open`.
pub fn open_path(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &path.display().to_string()])
            .spawn()
            .map(|_| ())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn().map(|_| ())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "opening local files is not supported on this platform",
        ))
    }
}
