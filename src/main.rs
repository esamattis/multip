#[macro_use]
extern crate lazy_static;

use libc::c_int;
use nix::sys::signal;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::sys::signal::{signal as trap_os, SigHandler};
use nix::unistd::Pid;
use std::convert::TryFrom;
use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{id, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::{thread, time};

macro_rules! log {
    () => {
        println!();
    };
    ($($arg:tt)+) => {
        print!("[multip] ");
        println!($($arg)*);
    }
}

// Store last received signal here

lazy_static! {
    static ref SIGNAL: Arc<Mutex<c_int>> = Arc::new(Mutex::new(0));
    static ref CONDVAR: Arc<Condvar> = Arc::new(Condvar::new());
}

struct Line {
    name: String,
    line: String,
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.name, self.line.trim())
    }
}

enum Message {
    Line(Line),
    ParentSignal(Signal),
}

type Channel = std::sync::mpsc::Sender<Message>;

struct MultipChild<'a> {
    name: &'a str,
    kill_sent: Option<Signal>,
    death_handled: bool,
    tx: &'a Channel,
    cmd: std::process::Child,
}

impl fmt::Display for MultipChild<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name, self.cmd.id())
    }
}

impl MultipChild<'_> {
    fn spawn<'a>(name: &'a str, command: &str, tx: &'a Channel) -> MultipChild<'a> {
        let mut cmd = Command::new("/bin/sh")
            .arg("-c")
            // Add implicit exec to avoid extra process
            .arg(format!("exec {}", command))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()
            .expect("failed to spawn command");

        let stdout = cmd.stdout.take().expect("failed to take stdout");
        let stderr = cmd.stderr.take().expect("failed to take stderr");

        let pid = cmd.id();
        log!("Started [{}] with pid {}", name, pid);

        let child = MultipChild {
            name,
            tx,
            cmd,
            death_handled: false,
            kill_sent: None,
        };

        child.monitor_ouput(stdout);
        child.monitor_ouput(stderr);

        child
    }

    fn kill(&mut self, sig: Signal) {
        if !self.is_alive() {
            return;
        }

        if let Some(prev) = self.kill_sent {
            if prev == sig {
                return;
            }
        }

        self.kill_sent = Some(sig);

        let pid = nix::unistd::Pid::from_raw(self.cmd.id() as i32);

        log!("Sending {} to {}({})", sig, self.name, pid);
        if kill(pid, sig).is_err() {
            log!("kill failed for {}", self.name);
        }
    }

    fn handle_death(&mut self) -> bool {
        let is_new = !self.death_handled && !self.is_alive();
        self.death_handled = true;
        is_new
    }

    fn monitor_ouput(&self, stream: impl Read + Send + 'static) -> std::thread::JoinHandle<()> {
        let name = self.name.to_string();
        let tx = mpsc::Sender::clone(self.tx);
        thread::spawn(move || {
            let mut buf = BufReader::new(stream);

            loop {
                let name = name.to_string();
                let mut line = String::new();
                let res = buf.read_line(&mut line);
                match res {
                    Ok(0) => {
                        // EOF
                        return;
                    }
                    Ok(_) => {
                        tx.send(Message::Line(Line { name, line })).unwrap();
                    }
                    Err(msg) => {
                        tx.send(Message::Line(Line {
                            name,
                            line: format!("Failed to parse line. Error {}", msg),
                        }))
                        .unwrap();
                    }
                }
            }
        })
    }

    fn is_alive(&mut self) -> bool {
        match self.cmd.try_wait() {
            Ok(Some(_)) => return false,
            Ok(None) => return true,
            _ => {
                log!("Failed to read exit status");
                false
            }
        }
    }
}

fn command_with_name(s: &String) -> (&str, &str) {
    let bytes = s.as_bytes();

    for (i, &item) in bytes.iter().enumerate() {
        if item == b':' {
            return (&s[0..i], (&s[i + 1..]).trim());
        }
    }

    panic!("cannot parse name from> {}", s);
}

extern "C" fn handle_os_signal(s: c_int) {
    let mut current_signal = SIGNAL.lock().expect("signal fail");
    *current_signal = s;
    CONDVAR.notify_one();
}

fn trap_signal(s: Signal) {
    let handler = SigHandler::Handler(handle_os_signal);
    unsafe { trap_os(s, handler) }.unwrap();
}

// Poll for the sotred signal and send it back via the channel
fn poll_signals<F>(cb: F)
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

fn main() {
    log!("Started multip with pid {}", id());
    let (tx, rx) = mpsc::channel::<Message>();

    trap_signal(signal::SIGINT);
    trap_signal(signal::SIGTERM);
    trap_signal(signal::SIGQUIT);
    trap_signal(signal::SIGCHLD);

    let t = mpsc::Sender::clone(&tx);
    poll_signals(move |sig| {
        t.send(Message::ParentSignal(sig)).unwrap();
    });

    let args: Vec<String> = env::args().collect();
    let mut children: Vec<MultipChild> = Vec::new();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);
        let child = MultipChild::spawn(name, command, &tx);
        children.push(child)
    }

    for msg in &rx {
        let mut forward_signal: Option<Signal> = None;

        match msg {
            Message::ParentSignal(Signal::SIGCHLD) => {
                log!("Got SIGCHLD. Looking for dead child");
                for child in children.iter_mut() {
                    if child.handle_death() {
                        log!("{} has died", child);
                        forward_signal = Some(Signal::SIGTERM);
                    }
                }
            }
            Message::ParentSignal(parent_signal) => {
                if forward_signal.is_none() {
                    log!("Forwarding parent signal {} to children", parent_signal);
                }
                forward_signal = Some(parent_signal);
            }
            Message::Line(line) => {
                println!("{}", line);
            }
        }

        let mut somebody_is_alive = false;

        for child in children.iter_mut() {
            if let Some(sig) = forward_signal {
                child.kill(sig);
            }

            if child.is_alive() {
                somebody_is_alive = true;
            }
        }

        if !somebody_is_alive {
            log!("All processes died. Exiting...");
            break;
        }
    }

    // Print all pending message from the buffers
    for msg in rx.try_iter() {
        match msg {
            Message::Line(line) => {
                println!("{}", line);
            }
            _ => {
                println!("Unhandled remaining message...");
            }
        }
    }
}
