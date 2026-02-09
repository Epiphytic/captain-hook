pub mod policy;
pub mod roles;

pub use policy::*;
pub use roles::*;

use std::path::PathBuf;

/// Returns the global config directory path: `~/.config/captain-hook/`
pub fn dirs_global() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config").join("captain-hook")
}
