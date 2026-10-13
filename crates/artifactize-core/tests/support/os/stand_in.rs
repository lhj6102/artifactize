//! Dependency-free console fixtures, also compiled by the standalone utility stand-in.
/// Windows console Ctrl-Break is the termination request corresponding to Unix TERM.
pub fn ignore_interrupts() {
    #[cfg(windows)]
    {
        unsafe extern "system" fn ignore(_event: u32) -> i32 {
            1
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(u32) -> i32>,
                add: i32,
            ) -> i32;
        }
        // SAFETY: registers a handler that only returns TRUE and retains no data.
        unsafe { SetConsoleCtrlHandler(Some(ignore), 1) };
    }
}
