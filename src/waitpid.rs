use nix::errno::Errno;
use nix::sys::wait::WaitStatus::{Exited, Signaled, StillAlive};
use nix::sys::wait::{waitpid, WaitPidFlag};
use nix::unistd::Pid;
use nix::Error::Sys;

use crate::*;

pub struct ProcessWaiter {}

impl Iterator for ProcessWaiter {
    type Item = (Pid, i32);

    fn next(&mut self) -> Option<Self::Item> {
        // -1     meaning wait for any child process.
        // WNOHANG     return immediately if no child has exited.
        let status = waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG));

        match status {
            Ok(Exited(pid, exit_code)) => Some((pid, exit_code)),

            Ok(Signaled(pid, signal, _core_dumped)) => {
                debug!("waitpid(): {} killed with signal {}", pid, signal);
                Some((pid, 0))
            }

            Ok(StillAlive) => None,

            Ok(status) => {
                log!("Unknown status from waitpid() {:#?}", status);
                None
            }

            Err(Sys(Errno::ECHILD)) => {
                // log!("No child processess");
                None
            }

            Err(err) => {
                log!("Failed to waitpid() {}", err);
                None
            }
        }
    }
}

pub fn iter_dead_children() -> ProcessWaiter {
    ProcessWaiter {}
}
