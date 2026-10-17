//! Windows console handling for the GUI. The executable is a console-subsystem
//! program so the CLI and the MCP server get real standard handles; a GUI
//! started by double-click would then show an empty console window, which
//! [`detach_if_alone`] closes before the main window opens.
//!
//! This is the only module of the crate that uses `unsafe` (two calls into
//! kernel32), the same arrangement as `ananasar-dde`.
#![allow(unsafe_code, reason = "two kernel32 calls with no pointer invariants beyond a local buffer")]

/// True when this process is the only one attached to its console, which is
/// the case when Windows created the console for it (a double-click in
/// Explorer). Started from a terminal there are at least two: the shell and us.
#[cfg(windows)]
pub fn owns_console_alone() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    let mut ids = [0u32; 2];
    // SAFETY: the pointer is to a live local array and the count is its length.
    let attached = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), 2) };
    attached == 1
}

/// Elsewhere a double-click does not run the program through a console, so
/// there is no such case to detect.
#[cfg(not(windows))]
pub fn owns_console_alone() -> bool {
    false
}

/// Release the console if this process is its only user, so that no console
/// window stays open behind the GUI. A terminal that launched us keeps its
/// console.
pub fn detach_if_alone() {
    #[cfg(windows)]
    if owns_console_alone() {
        use windows_sys::Win32::System::Console::FreeConsole;
        // SAFETY: no arguments; failure just leaves the console attached.
        let _ = unsafe { FreeConsole() };
    }
}
