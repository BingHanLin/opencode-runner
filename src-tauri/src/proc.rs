//! Subprocess spawn helpers.
//!
//! The release build sets `windows_subsystem = "windows"` (see `main.rs`), so
//! the GUI process has no console attached. Spawning a console subprocess
//! (`opencode`, `git`) from that build makes Windows allocate a fresh console
//! window for the child — it flashes open for short-lived commands and stays
//! visible for the duration of long-running ones. Applying `CREATE_NO_WINDOW`
//! suppresses that window. On non-Windows targets this is a no-op.

/// `CREATE_NO_WINDOW` — see the Win32 process creation flags. Spawning a
/// console child with this flag set gives it no console, so no window appears.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Apply the no-console-window creation flag on Windows; no-op elsewhere.
/// Call this on every `tokio::process::Command` before `.spawn()`/`.output()`/
/// `.status()` so the GUI build never flashes a console window.
pub fn no_window(cmd: &mut tokio::process::Command) {
    // `tokio::process::Command` exposes `creation_flags` as an inherent method
    // on Windows (mirroring `std`), so no extra trait import is needed.
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Touch `cmd` so the parameter isn't flagged unused on non-Windows targets.
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}
