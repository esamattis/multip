use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use std::thread;
use std::time::Duration;

fn cap(
    name: &str,
    stream: impl Read + Send + 'static,
    tx: std::sync::mpsc::Sender<Line>,
) -> std::thread::JoinHandle<()> {
    let name = name.to_string();
    thread::spawn(move || {
        let buf = BufReader::new(stream);
        for line in buf.lines() {
            let name = name.to_string();
            tx.send(Line { name, line }).unwrap();
        }
    })
}

fn run(name: &str, command: &str, tx: &std::sync::mpsc::Sender<Line>) -> std::process::Child {
    let mut cmd = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()
        .expect("failed to spwan");

    let stdout = cmd.stdout.take().expect("lol");
    let stderr = cmd.stderr.take().expect("lol");

    let tx1 = mpsc::Sender::clone(&tx);
    let tx2 = mpsc::Sender::clone(&tx);
    cap(name, stdout, tx1);
    cap(name, stderr, tx2);

    cmd
}

struct Ding<'a> {
    name: &'a str,
    child: std::process::Child,
    kill_sent: bool,
    died: bool,
}

impl fmt::Display for Ding<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name, self.child.id())
    }
}

struct Line {
    name: String,
    line: std::result::Result<std::string::String, std::io::Error>,
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.name, self.line.as_ref().unwrap().trim())
    }
}

fn command_with_name(s: &String) -> (&str, &str) {
    let bytes = s.as_bytes();

    for (i, &item) in bytes.iter().enumerate() {
        if item == b':' {
            return (&s[0..i], (&s[i + 1..]).trim());
        }
    }

    panic!("parse error: {}", s);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut dings: Vec<Ding> = Vec::new();
    let (tx, rx) = mpsc::channel::<Line>();

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);

        let child = run(name, command, &tx);

        dings.push(Ding {
            name,
            child,
            kill_sent: false,
            died: false,
        })
    }

    for received in rx {
        println!("{}", received);
        let mut somebody_is_alive = false;
        let mut killall = false;

        for ding in dings.iter_mut() {
            match ding.child.try_wait() {
                Ok(Some(status)) => {
                    if !ding.died {
                        println!("{} died with: {}", ding, status);
                        ding.died = true;
                    }

                    if !killall {
                        println!("Killing others!");
                        killall = true;
                    }
                }
                Ok(None) => {
                    somebody_is_alive = true;
                }
                Err(e) => println!("error attempting to wait: {}", e),
            }
        }

        if killall {
            for ding in dings.iter_mut() {
                if ding.died {
                    continue;
                }
                ding.kill_sent = true;
                println!("Killing {}", ding);
                ding.child.kill().expect("kill failed");
            }
        }

        if !somebody_is_alive {
            println!("All processes died. Existing...");
            return;
        }
    }
}
