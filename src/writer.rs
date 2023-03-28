//! TODO: The implementation here is extremely primitive and unoptimized.

use crate::Pathbox;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub(crate) fn stdout(pathbox: &Pathbox) -> Writer<'_> {
    Writer::new(pathbox, Box::new(std::io::stdout()))
}

pub(crate) fn stderr(pathbox: &Pathbox) -> Writer<'_> {
    Writer::new(pathbox, Box::new(std::io::stderr()))
}

/// A standard-output stream that's linked to a [`Pathbox`] and translates
/// guest paths back into their external presentation.
pub struct Writer<'a> {
    pathbox: &'a Pathbox,
    inner: Box<dyn io::Write>,
    buf: Vec<u8>,
}

impl<'a> Writer<'a> {
    fn new(pathbox: &'a Pathbox, inner: Box<dyn io::Write>) -> Self {
        Self {
            pathbox,
            inner,
            buf: Vec::new(),
        }
    }

    fn replace_guest_paths(&mut self) {
        if let Some((before, _after_prefix)) = is_subsequence(b"guest-path.", &self.buf) {
            for grant in self.pathbox.as_slice() {
                let after_match = before + grant.guest.len();
                if self.buf.get(before..after_match) == Some(grant.guest.as_bytes()) {
                    let after = self.buf[after_match..].to_vec();
                    self.buf.resize(before, 0);

                    #[cfg(unix)]
                    self.buf.extend_from_slice(grant.original.as_bytes());
                    #[cfg(not(unix))]
                    self.buf
                        .extend_from_slice(grant.original.as_os_str().to_str().unwrap().as_bytes());

                    self.buf.extend_from_slice(&after);
                }
            }
        }
    }
}

fn is_subsequence(needle: &[u8], haystack: &[u8]) -> Option<(usize, usize)> {
    if needle.len() <= haystack.len() {
        for i in 0..haystack.len() - needle.len() {
            if &haystack[i..i + needle.len()] == needle {
                return Some((i, i + needle.len()));
            }
        }
    }
    None
}

impl<'a> io::Write for Writer<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut work = buf;
        while let Some(line) = work.iter().position(|b| *b == b'\n') {
            self.buf.extend_from_slice(&work[..=line]);
            self.replace_guest_paths();
            self.inner.write_all(&self.buf)?;
            work = &work[line + 1..];
            self.buf.clear();
        }
        self.buf.extend_from_slice(work);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
