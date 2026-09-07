#[cfg(target_os = "macos")]
#[path = "installer_macos.rs"]
mod implementation;
#[cfg(not(target_os = "macos"))]
#[path = "installer_other.rs"]
mod implementation;
pub use implementation::*;
