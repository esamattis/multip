use nix::errno::Errno;
use nix::sys::signal;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::sys::wait::WaitStatus::{Exited, StillAlive};
use nix::sys::wait::{waitpid, WaitPidFlag};
use nix::unistd::Pid;
use nix::Error::Sys;
use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{id, Command, Stdio};
use std::sync::mpsc;
use std::thread;

mod log;
mod signal_closure;

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
    is_dead: bool,
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
            is_dead: false,
            kill_sent: None,
        };

        child.monitor_ouput(stdout);
        child.monitor_ouput(stderr);

        child
    }

    fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.cmd.id() as i32)
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

        let pid = self.pid();

        log!("Sending {} to {}({})", sig, self.name, pid);
        if kill(pid, sig).is_err() {
            log!("kill failed for {}", self.name);
        }
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

    fn is_alive(&self) -> bool {
        !self.is_dead
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

struct ProcessWaiter {}

impl ProcessWaiter {
    fn iter() -> ProcessWaiter {
        ProcessWaiter {}
    }
}

impl Iterator for ProcessWaiter {
    type Item = (Pid, i32);

    fn next(&mut self) -> Option<Self::Item> {
        // WNOHANG     return immediately if no child has exited.
        let status = waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG));

        match status {
            Ok(Exited(pid, exit_code)) => Some((pid, exit_code)),

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

fn main() {
    log!("Started multip with pid {}", id());
    let (tx, rx) = mpsc::channel::<Message>();

    signal_closure::trap_signal(signal::SIGINT);
    signal_closure::trap_signal(signal::SIGTERM);
    signal_closure::trap_signal(signal::SIGQUIT);
    signal_closure::trap_signal(signal::SIGCHLD);

    let t = mpsc::Sender::clone(&tx);
    signal_closure::poll_signals(move |sig| {
        t.send(Message::ParentSignal(sig)).unwrap();
    });

    let args: Vec<String> = env::args().collect();
    let mut children: Vec<MultipChild> = Vec::new();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);
        let child = MultipChild::spawn(name, command, &tx);
        children.push(child)
    }

    let mut killall: Option<Signal> = None;
    let mut sigint_count = 0;

    for msg in &rx {
        // Look for dead chilren on every event
        for (pid, exit_code) in ProcessWaiter::iter() {
            let child = children.iter_mut().find(|child| child.pid() == pid);

            match child {
                Some(child) => {
                    log!("Child {} died with exit code {}", child, exit_code);
                    child.is_dead = true;
                    killall = Some(Signal::SIGTERM);
                }
                None => {
                    log!("Unknown process({}) died with exit code {}", pid, exit_code);
                }
            }
        }

        match msg {
            Message::ParentSignal(Signal::SIGCHLD) => {
                // no-op signal just for looking dead children
            }

            Message::ParentSignal(Signal::SIGINT) => {
                killall = Some(Signal::SIGINT);
                sigint_count += 1;

                if sigint_count == 2 {
                    log!("Got second SIGINT, converting it to SIGKILL");
                    killall = Some(Signal::SIGTERM);
                } else if sigint_count > 2 {
                    log!("Got third SIGINT, converting it to SIGKILL");
                    killall = Some(Signal::SIGKILL);
                }
            }

            Message::ParentSignal(parent_signal) => {
                if killall.is_none() {
                    log!("Forwarding parent signal {} to children", parent_signal);
                }
                killall = Some(parent_signal);
            }

            Message::Line(line) => {
                println!("{}", line);
            }
        }

        let mut somebody_is_alive = false;

        for child in children.iter_mut() {
            if let Some(sig) = killall {
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
