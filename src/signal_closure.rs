use lazy_static::lazy_static;
use libc::c_int;
use nix::sys::signal::Signal;
use nix::sys::signal::{signal as trap_os, SigHandler};
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
    unsafe { trap_os(s, handler) }.unwrap();
}

// Poll for the sotred signal and send it back via the channel
pub fn poll_signals<F>(cb: F)
where
    F: 'static + Send + Fn(Signal) -> (),
{
    thread::spawn(move || {
        let mut sigint_count = 0;
        loop {
            log!("waiting lock lock");
            let current_signal = SIGNAL.lock().expect("signal fail");
            log!("waiting condvar");
            let sig = *CONDVAR.wait(current_signal).expect("wait fail");
            log!("got lock {}", sig);

            if sig == 0 {
                log!("weird signal");
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

            if sig == Signal::SIGINT {
                sigint_count += 1;
            }

            match (sig, sigint_count) {
                (Signal::SIGINT, 2) => {
                    log!("Got second SIGINT, converting it to SIGTERM...");
                    cb(Signal::SIGTERM);
                    // tx.send(Message::ParentSignal(Signal::SIGTERM)).unwrap();
                }
                (Signal::SIGINT, 3) => {
                    log!("Got third SIGINT, converting it to SIGKILL...");
                    cb(Signal::SIGKILL);
                    // tx.send(Message::ParentSignal(Signal::SIGKILL)).unwrap();
                }
                _ => {
                    cb(sig);
                    // tx.send(Message::ParentSignal(sig)).unwrap();
                }
            }
        }
    });
}
