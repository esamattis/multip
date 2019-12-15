use nix::sys::signal::kill;
use nix::sys::signal::Signal::SIGTERM;
use nix::unistd::Pid;
use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;

use std::thread;

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
}

type Channel = std::sync::mpsc::Sender<Message>;

struct MultipChild<'a> {
    name: &'a str,
    pid: Pid,
    kill_sent: bool,
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
            .arg(command)
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
            kill_sent: false,
        };

        child.monitor_ouput(stdout);
        child.monitor_ouput(stderr);
        child.monitor_for_exit(cmd);

        child
    }

    fn kill(&mut self) {
        if self.kill_sent || !self.is_alive() {
            return;
        }

        self.kill_sent = true;
        println!("Killing {}", self);
        if kill(self.pid, SIGTERM).is_err() {
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

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut children: Vec<MultipChild> = Vec::new();
    let (tx, rx) = mpsc::channel::<Message>();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);
        let child = MultipChild::spawn(name, command, &tx);
        children.push(child)
    }

    let mut killall = false;

    for msg in rx {
        match msg {
            Message::Exit(name, exit_status) => {
                println!("{} exited with {}", name, exit_status);
                for ding in children.iter_mut() {
                    if ding.name == name {
                        ding.exit_status = Some(exit_status);
                    }
                }
                if !killall {
                    println!("Killing others");
                    killall = true;
                }
            }
            Message::Line(line) => {
                println!("line: {}", line);
            }
        }

        let mut somebody_is_alive = false;

        for child in children.iter_mut() {
            if killall {
                child.kill();
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
