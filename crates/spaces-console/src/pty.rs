//! PTY allocation helper for real-time output in Passthrough mode.

use std::fs::File;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::process::Stdio;

pub struct PtyPair {
    pub master: File,
    pub slave: Stdio,
}

pub fn open_pty() -> anyhow::Result<PtyPair> {
    let mut master_fd: libc::c_int = 0;
    let mut slave_fd: libc::c_int = 0;

    let ret = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    anyhow::ensure!(
        ret == 0,
        "openpty() failed: {}",
        std::io::Error::last_os_error()
    );

    let master = unsafe { File::from(OwnedFd::from_raw_fd(master_fd)) };
    let slave_stdio: Stdio = unsafe { OwnedFd::from_raw_fd(slave_fd) }.into();

    Ok(PtyPair {
        master,
        slave: slave_stdio,
    })
}
