use libc::c_int;
use nix::sys::signal;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::sys::signal::{signal as trap_os, SigHandler};
use nix::unistd::Pid;
use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{id, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::{thread, time};

// Store last received signal here
static SIGNAL: AtomicI32 = AtomicI32::new(0);

fn int_to_sig(i: i32) -> Signal {
    match i {
        2 => Signal::SIGINT,
        3 => Signal::SIGQUIT,
        15 => Signal::SIGTERM,
        sig => {
            panic!("Parent received unkown signal {}", sig);
        }
    }
}

struct Line {
    name: String,
    line: Result<String, std::io::Error>,
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.name, self.line.as_ref().unwrap().trim())
    }
}

enum Message {
    Line(Line),
    Exit(String, std::process::ExitStatus),
    ParentSignal(Signal),
}

type Channel = std::sync::mpsc::Sender<Message>;

struct MultipChild<'a> {
    name: &'a str,
    pid: Pid,
    kill_sent: Option<Signal>,
    exit_status: Option<ExitStatus>,
    tx: &'a Channel,
}

impl fmt::Display for MultipChild<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name, self.pid)
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

        let pid = cmd.id() as i32;
        println!("Started {} as {}", name, pid);

        let pid = Pid::from_raw(pid);

        let child = MultipChild {
            name,
            tx,
            pid,
            exit_status: None,
            kill_sent: None,
        };

        child.monitor_ouput(stdout);
        child.monitor_ouput(stderr);
        child.monitor_for_exit(cmd);

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
        println!("Sending {} to {}({})", sig, self.name, self.pid);
        if kill(self.pid, sig).is_err() {
            println!("kill failed for {}", self.name);
        }
    }

    fn monitor_for_exit(&self, mut cmd: std::process::Child) {
        let tx = mpsc::Sender::clone(self.tx);
        let name = self.name.to_string();
        thread::spawn(move || {
            let res = cmd.wait().expect("exit failed");
            println!("{} exited {}", name, res);
            tx.send(Message::Exit(name, res)).unwrap();
        });
    }

    fn monitor_ouput(&self, stream: impl Read + Send + 'static) -> std::thread::JoinHandle<()> {
        let name = self.name.to_string();
        let tx = mpsc::Sender::clone(self.tx);
        thread::spawn(move || {
            let buf = BufReader::new(stream);
            for line in buf.lines() {
                let name = name.to_string();
                tx.send(Message::Line(Line { name, line })).unwrap();
            }
        })
    }

    fn is_alive(&self) -> bool {
        self.exit_status.is_none()
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
    // Just store the signal to avoid unsafety issues
    SIGNAL.store(s as i32, Ordering::SeqCst);
}

fn trap_signal(s: Signal) {
    let handler = SigHandler::Handler(handle_os_signal);
    unsafe { trap_os(s, handler) }.unwrap();
}

// Poll for the sotred signal and send it back via the channel
fn poll_signals(tx: &Channel) {
    let tx = mpsc::Sender::clone(tx);

    thread::spawn(move || {
        let mut sigint_count = 0;
        loop {
            let sig = SIGNAL.swap(0, Ordering::SeqCst);

            if sig != 0 {
                let sig = int_to_sig(sig);

                if sig == Signal::SIGINT {
                    sigint_count += 1;
                }

                if sigint_count == 2 {
                    println!("Got second SIGINT, converting it SIGKILL...");
                    tx.send(Message::ParentSignal(Signal::SIGKILL)).unwrap();
                } else {
                    tx.send(Message::ParentSignal(sig)).unwrap();
                }
            }

            thread::sleep(time::Duration::from_millis(100));
        }
    });
}

fn main() {
    println!("multip pid {}", id());
    let (tx, rx) = mpsc::channel::<Message>();

    trap_signal(signal::SIGINT);
    trap_signal(signal::SIGTERM);
    trap_signal(signal::SIGQUIT);
    poll_signals(&tx);

    let args: Vec<String> = env::args().collect();
    let mut children: Vec<MultipChild> = Vec::new();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);
        let child = MultipChild::spawn(name, command, &tx);
        children.push(child)
    }

    let mut killall: Option<Signal> = None;

    for msg in rx {
        match msg {
            Message::Exit(name, exit_status) => {
                println!("{} exited with {}", name, exit_status);
                for ding in children.iter_mut() {
                    if ding.name == name {
                        ding.exit_status = Some(exit_status);
                    }
                }
                if killall.is_none() {
                    println!("First child died. Bringing all down with SIGTERM.");
                    killall = Some(Signal::SIGTERM);
                }
            }
            Message::ParentSignal(parent_signal) => {
                if killall.is_none() {
                    println!("Parent got signal {}", parent_signal);
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
            println!("All processes died. Exiting...");
            return;
        }
    }
}
