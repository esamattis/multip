use nix::sys::signal::{kill, Signal};
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

fn run_multip(args: Vec<&str>) -> Command {
    let mut cmd = Command::new("target/debug/multip");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.args(args);

    cmd
}

fn get_lines(out: Option<ChildStdout>) -> Vec<String> {
    let out = out.expect("get stdout/stderr");
    let mut lines: Vec<String> = Vec::new();
    let buf = BufReader::new(out);
    for line in buf.lines() {
        let line = line.unwrap_or("".to_string());
        lines.push(line);
    }
    lines
}

fn assert_has_line(lines: &Vec<String>, needle_line: &str) {
    let found = lines.iter().find(|&line| line.trim() == needle_line);

    if found.is_some() {
        return;
    }

    for line in lines {
        eprintln!("LINE> {}", line);
    }

    panic!("Failed to find line: {}", needle_line);
}

fn assert_line_matches(lines: &Vec<String>, pat: &str, count: i32) {
    let re = Regex::new(pat).unwrap();

    let mut match_count = 0;

    for line in lines {
        if re.is_match(line.trim()) {
            match_count = match_count + 1;
        }
    }

    if match_count == count {
        return;
    }

    for line in lines {
        eprintln!("LINE> {}", line);
    }

    panic!(
        "Failed to get {} (got {}) matches with RegExp: {}",
        count, match_count, pat
    );
}

#[test]
fn run_single_command() {
    let mut cmd = run_multip(vec!["foo: sh -c 'echo hello'"]).spawn().unwrap();

    let lines = get_lines(cmd.stdout.take());

    assert_has_line(&lines, "[foo] hello");

    cmd.wait().unwrap();
}

#[test]
fn run_multiple_commands() {
    let mut cmd = run_multip(vec![
        "foo: sh -c 'echo hello foo && sleep 0.1'",
        "bar: sh -c 'echo hello bar && sleep 0.1'",
    ])
    .spawn()
    .unwrap();

    let lines = get_lines(cmd.stdout.take());

    assert_has_line(&lines, "[foo] hello foo");
    assert_has_line(&lines, "[bar] hello bar");

    cmd.wait().unwrap();
}

#[test]
fn uses_exit_code_of_first_dead_child() {
    let mut cmd = run_multip(vec![
        "foo: sh -c 'sleep 0.1 && exit 11'",
        "bar: sh -c 'sleep 0.2 && exit 22'",
    ])
    .spawn()
    .unwrap();

    let status_code = cmd.wait().unwrap().code().unwrap();
    assert_eq!(status_code, 11);
}

#[test]
fn uses_exit_code_of_first_dead_child_with_zore() {
    let mut cmd = run_multip(vec![
        "foo: sh -c 'sleep 0.1 && exit 0'",
        "bar: sh -c 'sleep 0.2 && exit 22'",
    ])
    .spawn()
    .unwrap();

    let status_code = cmd.wait().unwrap().code().unwrap();
    assert_eq!(status_code, 0);
}

#[test]
#[cfg(target_os = "linux")]
fn reaps_zombies() {
    let mut cmd = run_multip(vec!["test: ./tests/zombie_creator.py"])
        .spawn()
        .unwrap();

    let lines = get_lines(cmd.stdout.take());
    cmd.wait().unwrap();

    assert_line_matches(&lines, r"Reaped zombie process(.*) with exit code 12", 1);
}

#[test]
fn wraps_long_lines() {
    let mut cmd = run_multip(vec!["foo: sh -c 'echo 1234567890'"])
        .env("MULTIP_MAX_LINE_LENGTH", "5")
        .spawn()
        .unwrap();

    let lines = get_lines(cmd.stdout.take());
    cmd.wait().unwrap();

    assert_has_line(&lines, "[foo...] 12345");
    assert_has_line(&lines, "[foo...] 67890");
}

#[test]
fn signal_handling() {
    let mut cmd = run_multip(vec!["test: ./tests/signals.py"])
        .spawn()
        .unwrap();

    let pid = nix::unistd::Pid::from_raw(cmd.id() as i32);

    thread::sleep(Duration::from_millis(100));
    kill(pid, Signal::SIGINT).unwrap();
    thread::sleep(Duration::from_millis(50));
    kill(pid, Signal::SIGINT).unwrap();
    thread::sleep(Duration::from_millis(50));
    kill(pid, Signal::SIGINT).unwrap();

    let lines = get_lines(cmd.stdout.take());
    cmd.wait().unwrap();
    assert_has_line(&lines, "[test] got signal 2");
    assert_has_line(&lines, "[test] got signal 15");
}
