use core::str::from_utf8;
use std::cmp;
use std::convert::TryFrom;
use std::fmt;
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

impl Line {
    pub fn as_line<'a>(&'a self) -> &'a str {
        match self {
            Line::PartialLine(s) => &s,
            Line::FullLine(s) => &s,
            Line::EOF(s) => &s,
        }
    }
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
    EOF(String),
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Line::PartialLine(s) => write!(f, "PartialLine({})", s),
            Line::FullLine(s) => write!(f, "FullLine({})", s),
            Line::EOF(s) => write!(f, "EOF({})", s),
        }
    }
}

enum Status {
    Full(usize),
    Partial(usize),
    Missing(usize),
    Error(usize, Error),
}

pub struct SafeLineReader<R> {
    inner: BufReader<R>,
    max_line_size: usize,
    sent_partial: bool,
}

impl<R: Read> SafeLineReader<R> {
    pub fn new(inner: BufReader<R>, max_line_size: usize) -> SafeLineReader<R> {
        SafeLineReader {
            inner,
            max_line_size,
            sent_partial: false,
        }
    }

    pub fn read_line(&mut self) -> Result<Line, Error> {
        // b'\n'
        let mut buf = String::new();

        loop {
            let status = {
                let available = match self.inner.fill_buf() {
                    Ok(n) => n,
                    Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };

                let space_available = to_usize(to_isize(self.max_line_size) - to_isize(buf.len()));

                match memchr::memchr(b'\n', available) {
                    Some(i) => {
                        let overflow = to_isize(self.max_line_size)
                            - (to_isize(buf.len()) + to_isize(i));

                        if overflow >= 0 {
                            let res = append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..=i]);
                                Ok(available[..=i].len())
                            });

                            if let Err(err) = res {
                                Status::Error(i + 1, err)
                            } else if self.sent_partial {
                                self.sent_partial = false;
                                Status::Partial(i + 1)
                            } else {
                                Status::Full(i + 1)
                            }
                        } else {
                            let remaining = cmp::min(space_available, i + 1);
                            let res = append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..remaining]);
                                Ok(available[..remaining].len())
                            });

                            if let Err(err) = res {
                                Status::Error(i + 1, err)
                            } else {
                                self.sent_partial = true;
                                Status::Partial(remaining)
                            }
                        }
                    }
                    None => {
                        let overflow = to_isize(self.max_line_size)
                            - (to_isize(buf.len()) + to_isize(available.len()));
                        if overflow < 0 {
                            let res = append_to_string(&mut buf, |b| {
                                b.extend_from_slice(&available[..space_available]);
                                Ok(available[..space_available].len())
                            });

                            if let Err(err) = res {
                                Status::Error(space_available, err)
                            } else {
                                self.sent_partial = true;
                                Status::Partial(space_available)
                            }
                        } else {
                            let res = append_to_string(&mut buf, |b| {
                                b.extend_from_slice(available);
                                Ok(available.len())
                            });

                            if let Err(err) = res {
                                Status::Error(available.len(), err)
                            } else {
                                Status::Missing(available.len())
                            }
                        }
                    }
                }
            };

            match status {
                Status::Full(used) => {
                    self.inner.consume(used);
                    return Ok(Line::FullLine(buf));
                }
                Status::Partial(used) => {
                    self.inner.consume(used);
                    return Ok(Line::PartialLine(buf));
                }
                Status::Missing(used) => {
                    if used == 0 {
                        return Ok(Line::EOF(buf));
                    }
                    self.inner.consume(used);
                }
                Status::Error(used, err) => {
                    self.inner.consume(used);
                    return Err(err);
                }
            }
        }
    }
}

#[cfg(test)]
fn get_full_line(s: Line) -> String {
    match s {
        Line::FullLine(s) => s,
        Line::PartialLine(s) => panic!("Expected full line but got partial with: `{}`", s),
        Line::EOF(s) => panic!("Expected full line but got EOF with: `{}`", s),
    }
}

#[cfg(test)]
fn get_partial_line(s: Line) -> String {
    match s {
        Line::PartialLine(s) => s,
        Line::FullLine(s) => panic!("Expected partial line but got full with: `{}`", s),
        Line::EOF(s) => panic!("Expected partial line but got EOF with: `{}`", s),
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

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "too lon");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "g line\n");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "second ");
}

#[test]
fn can_split_too_long_lines_small_buffer() {
    let in_buf: &[u8] = b"too long line\nsecond line\n";

    let mut reader = SafeLineReader::new(BufReader::with_capacity(3, in_buf), 7);

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "too lon");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "g line\n");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "second ");
}

#[test]
fn really_long_line() {
    let in_buf: &[u8] = b"too long line hubba bubba dubba\n";

    let mut reader = SafeLineReader::new(BufReader::with_capacity(3, in_buf), 5);

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "too l");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "ong l");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "ine h");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "ubba ");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "bubba");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, " dubb");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "a\n");
}

#[test]
fn empty_lines() {
    let in_buf: &[u8] = b"\n\n\n\n";
    let mut reader = SafeLineReader::new(BufReader::with_capacity(2, in_buf), 5);

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "\n");
}

#[test]
fn invalid_unicode() {
    let in_buf: &[u8] = &[32, 255, 6, 2, 3];
    let mut reader = SafeLineReader::new(BufReader::with_capacity(2, in_buf), 5);

    let err = match reader.read_line() {
        Err(err) => format!("{}", err),
        _ => String::from("no error"),
    };

    assert_eq!(err, "stream did not contain valid UTF-8");
}

#[test]
fn test_eof() {
    let in_buf: &[u8] = b"12345678";
    let mut reader = SafeLineReader::new(BufReader::with_capacity(2, in_buf), 5);

    let line = reader.read_line().unwrap();
    assert_eq!(format!("{}", line), "PartialLine(12345)");

    let line = reader.read_line().unwrap();
    assert_eq!(format!("{}", line), "EOF(678)");
}

#[test]
fn long_line_with_large_buffer() {
    let in_buf: &[u8] = b"abcdefghikjlmnopqrstxyz\nabcdefghikjlmnopqrstxyz\n";
    let mut reader = SafeLineReader::new(BufReader::with_capacity(1000, in_buf), 5);

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "abcde");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "fghik");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "jlmno");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "pqrst");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "xyz\n");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "abcde");
}

#[test]
fn long_line_in_the_middle() {
    let in_buf: &[u8] = b"foo\nlong stuff\nbar\n";
    let mut reader = SafeLineReader::new(BufReader::with_capacity(1000, in_buf), 5);

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "foo\n");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "long ");

    let s = get_partial_line(reader.read_line().unwrap());
    assert_eq!(s, "stuff\n");

    let s = get_full_line(reader.read_line().unwrap());
    assert_eq!(s, "bar\n");

}