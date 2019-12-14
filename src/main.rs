use std::env;
use std::fmt;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::marker::Send;
use std::process::{Command, Stdio};

use std::thread;
use std::time::Duration;

fn cap(prefix: String, stream: impl Read + Send + 'static) -> std::thread::JoinHandle<()> {
    thread::spawn(move || {
        let buf = BufReader::new(stream);
        for line in buf.lines() {
            println!("{}{}", prefix, line.unwrap().trim());
        }
    })
}

fn run(prefix: &str, command: &str) -> std::process::Child {
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

    cap(format!("{}{}", prefix, String::from("(stdout)> ")), stdout);
    cap(format!("{}{}", prefix, String::from("(stderr)> ")), stderr);

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

    for command in args[1..].iter() {
        let (name, command) = command_with_name(command);

        let child = run(&name[..], command);

        dings.push(Ding {
            name,
            child,
            kill_sent: false,
            died: false,
        })
    }

    let mut killall = false;

    loop {
        let mut somebody_is_alive = false;

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
                    if killall && !ding.kill_sent {
                        ding.kill_sent = true;
                        println!("Killing {}", ding);
                        ding.child.kill().expect("kill failed");
                    }
                }
                Err(e) => println!("error attempting to wait: {}", e),
            }
        }

        if !somebody_is_alive {
            println!("All processes died. Existing...");
            return;
        }

        thread::sleep(Duration::from_millis(1000));
    }
}
