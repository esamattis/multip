use nix::sys::signal;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{id, Command, Stdio};
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

mod line_reader;
mod log;
mod signal_closure;
mod waitpid;

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

        // Don't send the same signal twice
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

fn main() {
    let args: Vec<String> = env::args().collect();

    for command in args[1..].iter() {
        if command == "--version" {
            println!("version {}", option_env!("MULTIP_VERSION").unwrap_or("DEV"));
            println!("git rev {}", option_env!("GITHUB_SHA").unwrap_or("DEV"));
            return;
        }
    }

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

    let mut children: Vec<MultipChild> = Vec::new();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);
        let child = MultipChild::spawn(name, command, &tx);
        children.push(child)
    }

    let mut killall: Option<Signal> = None;

    let mut sigint_count = 0;

    loop {
        // Manually check for dead children with the given timeout
        let msg = rx.recv_timeout(Duration::from_millis(100));

        let mut forward: Option<Signal> = None;

        // Look for dead chilren on every event
        for (pid, exit_code) in waitpid::iter_dead_children() {
            let child = children.iter_mut().find(|child| child.pid() == pid);

            match child {
                Some(child) => {
                    log!("Child {} died with exit code {}", child, exit_code);
                    child.is_dead = true;
                    if killall.is_none() {
                        log!("Killing all other children too");
                        killall = Some(Signal::SIGTERM);
                    }
                }
                None => {
                    log!("Unknown process({}) died with exit code {}", pid, exit_code);
                }
            }
        }

        match msg {
            Err(RecvTimeoutError::Timeout) => {
                // loop tick
            }

            Ok(Message::ParentSignal(Signal::SIGCHLD)) => {
                // no-op signal just for looking dead children
            }

            Ok(Message::ParentSignal(Signal::SIGINT)) => {
                forward = Some(Signal::SIGINT);
                sigint_count += 1;

                if sigint_count == 2 {
                    log!("Got second SIGINT, converting it to SIGKILL");
                    forward = Some(Signal::SIGTERM);
                } else if sigint_count > 2 {
                    log!("Got third SIGINT, converting it to SIGKILL");
                    forward = Some(Signal::SIGKILL);
                }
            }

            Ok(Message::ParentSignal(parent_signal)) => {
                log!("Forwarding parent signal {} to children", parent_signal);
                forward = Some(parent_signal);
            }

            Ok(Message::Line(line)) => {
                println!("{}", line);
            }

            Err(RecvTimeoutError::Disconnected) => {
                println!("Channel disconnected");
                break;
            }
        }

        let mut somebody_is_alive = false;

        for child in children.iter_mut() {
            if let Some(sig) = forward {
                child.kill(sig);
            }

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
