use core::str::from_utf8;
use std::convert::TryFrom;
use std::io::{BufRead, BufReader, Read};
use std::io::{Error, ErrorKind};

struct Guard<'a> {
    buf: &'a mut Vec<u8>,
    len: usize,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        unsafe {
            self.buf.set_len(self.len);
        }
    }
}

fn to_usize(i: isize) -> usize {
    usize::try_from(i).unwrap_or(0)
}

fn to_isize(i: usize) -> isize {
    isize::try_from(i).unwrap_or(0)
}

// https://github.com/rust-lang/rust/blob/b69f6e65c081f9a628ef5db83ba77e3861e60e60/src/libstd/io/mod.rs#L333-L349
fn append_to_string<F>(buf: &mut String, f: F) -> Result<usize, Error>
where
    F: FnOnce(&mut Vec<u8>) -> Result<usize, Error>,
{
    unsafe {
        let mut g = Guard {
            len: buf.len(),
            buf: buf.as_mut_vec(),
        };
        let ret = f(g.buf);
        if from_utf8(&g.buf[g.len..]).is_err() {
            ret.and_then(|_| {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                ))
            })
        } else {
            g.len = g.buf.len();
            ret
        }
    }
}

pub enum Line {
    FullLine(String),
    PartialLine(String),
}

pub struct SafeLineReader<R> {
    inner: BufReader<R>,
    max_line_size: isize,
}

impl<R: Read> SafeLineReader<R> {
    pub fn new(inner: BufReader<R>, max_line_size: isize) -> SafeLineReader<R> {
        SafeLineReader {
            inner,
            max_line_size,
        }
    }

    fn read_line(&mut self) -> Result<Line, Error> {
        // b'\n'
        let mut buf = String::new();

        loop {
            let (done, used) = {
                let available = match self.inner.fill_buf() {
                    Ok(n) => n,
                    Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };

                let overflow =
                    self.max_line_size - (to_isize(buf.len()) + to_isize(available.len()));
                let space_available = to_usize(self.max_line_size - to_isize(buf.len()));

                match memchr::memchr(b'\n', available) {
                    Some(i) => {
                        if overflow >= 0 {
                            println!("################ IF {}", overflow);
                            append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..=i]);
                                Ok(available[..=i].len())
                            })?;

                            (true, i + 1)
                        } else {
                            println!("################ ELSE {}", overflow);

                            append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..space_available]);
                                Ok(available[..space_available].len())
                            })?;

                            (true, space_available)
                        }
                    }
                    None => {
                        println!("################ None {}", overflow);

                        if overflow < 0 {
                            println!("overflow > 0 TRUE");
                            append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..space_available]);
                                Ok(available[..space_available].len())
                            })?;
                            (false, space_available)
                        } else {
                            println!("overflow > 0 FALSE");
                            append_to_string(&mut buf, |b| {
                                b.extend_from_slice(available);
                                Ok(available.len())
                            })?;
                            (false, available.len())
                        }
                    }
                }
            };
            self.inner.consume(used);

            if done || used == 0 {
                return Ok(Line::FullLine(buf));
            }
        }

        // self.inner.read_line(&mut buf)?;
    }
}

#[cfg(test)]
fn get_full_line(s: Line) -> String {
    match s {
        Line::FullLine(s) => s,
        _ => String::from("err"),
    }
}

#[test]
fn can_read_multiple_lines() {
    let in_buf: &[u8] = b"a\nb\nc";

    let mut reader = SafeLineReader::new(BufReader::with_capacity(2, in_buf), 100);

    let s = reader.read_line().unwrap();

    assert_eq!(get_full_line(s), "a\n");
}

#[test]
fn can_read_multiple_lines_with_words() {
    let in_buf: &[u8] = b"first\nsecond\nthird\n";
    let mut reader = SafeLineReader::new(BufReader::with_capacity(2, in_buf), 100);

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "first\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "second\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "third\n");
}

#[test]
fn can_split_too_long_lines_large_buffer() {
    let in_buf: &[u8] = b"too long line\nsecond line\n";

    let mut reader = SafeLineReader::new(BufReader::with_capacity(100, in_buf), 7);

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "too lon");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "g line\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "second ");
}

#[test]
fn can_split_too_long_lines_small_buffer() {
    let in_buf: &[u8] = b"too long line\nsecond line\n";

    let mut reader = SafeLineReader::new(BufReader::with_capacity(3, in_buf), 7);

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "too lon");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "g line\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "second ");
}
