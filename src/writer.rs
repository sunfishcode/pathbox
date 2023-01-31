//! TODO: The implementation here is extremely primitive and unoptimized.

use crate::Preopener;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub(crate) fn stdout(preopener: &Preopener) -> Writer<'_> {
    Writer::new(preopener, Box::new(std::io::stdout()))
}

pub(crate) fn stderr(preopener: &Preopener) -> Writer<'_> {
    Writer::new(preopener, Box::new(std::io::stderr()))
}

/// A standard-output stream that's linked to a [`Preopener`] and translates
/// preopens back into their external presentation.
pub struct Writer<'a> {
    preopener: &'a Preopener,
    inner: Box<dyn io::Write>,
    buf: Vec<u8>,
}

impl<'a> Writer<'a> {
    fn new(preopener: &'a Preopener, inner: Box<dyn io::Write>) -> Self {
        Self {
            preopener,
            inner,
            buf: Vec::new(),
        }
    }

    fn replace_preopens(&mut self) {
        for preopen in self.preopener.as_slice() {
            while let Some((before, after)) = is_subsequence(preopen.uuid.as_bytes(), &self.buf) {
                let after = self.buf[after..].to_vec();
                self.buf.resize(before, 0);

                #[cfg(unix)]
                self.buf.extend_from_slice(preopen.original.as_bytes());
                #[cfg(not(unix))]
                self.buf
                    .extend_from_slice(preopen.original.as_os_str().to_str().unwrap().as_bytes());

                self.buf.extend_from_slice(&after);
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
            self.replace_preopens();
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
