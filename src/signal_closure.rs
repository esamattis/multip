use lazy_static::lazy_static;
use libc::c_int;
use nix::sys::signal::Signal;
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet};
use std::convert::TryFrom;
use std::marker::Send;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::log;

// Store last received signal here
lazy_static! {
    static ref SIGNAL: Arc<Mutex<c_int>> = Arc::new(Mutex::new(0));
    static ref CONDVAR: Arc<Condvar> = Arc::new(Condvar::new());
}

extern "C" fn handle_os_signal(s: c_int) {
    let mut current_signal = SIGNAL.lock().expect("signal fail");
    *current_signal = s;
    CONDVAR.notify_one();
}

pub fn trap_signal(s: Signal) {
    let handler = SigHandler::Handler(handle_os_signal);

    // https://www.gnu.org/software/libc/manual/html_node/Flags-for-Sigaction.html
    let sa_flags = SaFlags::SA_RESTART;

    // Block all other signals while the signal handler is executing
    let sig_set = SigSet::all();

    unsafe { sigaction(s, &SigAction::new(handler, sa_flags, sig_set)) }
        .expect("Failed to set signal handler");
}

// Poll for the sotred signal and send it back via the channel
pub fn poll_signals<F>(cb: F)
where
    F: 'static + Send + Fn(Signal) -> (),
{
    thread::spawn(move || loop {
        let current_signal = SIGNAL.lock().expect("signal fail");
        let sig = *CONDVAR.wait(current_signal).expect("wait fail");

        if sig == 0 {
            log!("Got weird signal 0");
            continue;
        }

        let try_sig = Signal::try_from(sig);
        let sig = match try_sig {
            Ok(sig) => sig,
            _ => {
                log!("Signal parsing failed");
                continue;
            }
        };

        cb(sig);
    });
}
