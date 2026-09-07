#[cfg(target_os = "macos")]
#[path = "native_macos.rs"]
mod platform;
#[cfg(not(target_os = "macos"))]
#[path = "native_other.rs"]
mod platform;
pub use platform::*;
