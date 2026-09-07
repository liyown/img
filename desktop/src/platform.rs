use std::{path::Path, process::Command};
pub fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}
pub fn capture_command(binary: &Path, path: &Path) -> Command {
    let mut command = Command::new(binary);
    command.arg("screenshot").arg("--output").arg(path);
    command.arg("--region");
    command
}
pub fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let program = "/usr/bin/open";
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    Command::new(program).arg(path).spawn()?;
    Ok(())
}
