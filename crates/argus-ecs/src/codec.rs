//! A small binary codec for state that goes to disk: the replay files
//! behind marks (phase 2 item 2), which hold a keyframe of the whole sim.
//! Little-endian; counts as u32, byte blocks with a u64 length, strings as
//! UTF-8 with a u32 length, floats by their bits. Every crate with state a
//! keyframe holds writes it with these.

use crate::error::{codes, ArgusError, Result};

/// Appends values to a byte buffer.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// An empty buffer.
    pub fn new() -> Writer {
        Writer::default()
    }
    /// One byte.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    /// A bool as one byte.
    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    /// Little-endian u32.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend(v.to_le_bytes());
    }
    /// Little-endian u64.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend(v.to_le_bytes());
    }
    /// Little-endian i64.
    pub fn i64(&mut self, v: i64) {
        self.buf.extend(v.to_le_bytes());
    }
    /// An f32 by its bits.
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    /// An f64 by its bits.
    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    /// A count or a length.
    pub fn count(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("more than 4 billion items"));
    }
    /// A string.
    pub fn str(&mut self, s: &str) {
        self.count(s.len());
        self.buf.extend(s.as_bytes());
    }
    /// A block of bytes.
    pub fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.buf.extend(b);
    }
    /// An optional value: a presence byte, then the value.
    pub fn opt<T>(&mut self, v: Option<T>, f: impl FnOnce(&mut Writer, T)) {
        self.bool(v.is_some());
        if let Some(v) = v {
            f(self, v);
        }
    }
    /// Bytes written so far.
    pub fn written(&self) -> usize {
        self.buf.len()
    }
    /// The buffer.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// Reads values back, failing with a structured error (not a panic) on a
/// short or corrupt buffer.
pub struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    /// Read `buf`; `what` names it in errors ("world state").
    pub fn new(buf: &'a [u8], what: &'static str) -> Reader<'a> {
        Reader { buf, at: 0, what }
    }

    /// An error at the current position.
    pub fn bad(&self, why: impl std::fmt::Display) -> ArgusError {
        ArgusError::new(
            codes::BAD_STATE,
            format!("{}: {why} (at byte {})", self.what, self.at),
        )
        .hint("The file is damaged or was written by another version of Argus.")
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.buf.len() - self.at < n {
            return Err(self.bad(format!(
                "ends {} bytes early",
                n - (self.buf.len() - self.at)
            )));
        }
        let s = &self.buf[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    /// One byte.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    /// A bool.
    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(self.bad(format!("{b} is not a bool"))),
        }
    }
    /// A u32.
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// A u64.
    pub fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().expect("8 bytes")))
    }
    /// An i64.
    pub fn i64(&mut self) -> Result<i64> {
        Ok(self.u64()? as i64)
    }
    /// An f32.
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    /// An f64.
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    /// A count or a length, no larger than the bytes left could hold
    /// (every item takes at least one), so a corrupt count cannot ask for
    /// a huge allocation.
    pub fn count(&mut self) -> Result<usize> {
        let n = self.u32()? as usize;
        if n > self.remaining() {
            return Err(self.bad(format!(
                "a count of {n} with {} bytes left",
                self.remaining()
            )));
        }
        Ok(n)
    }
    /// A count of items that may take no bytes each (the rows of tags).
    pub fn rows(&mut self) -> Result<usize> {
        Ok(self.u32()? as usize)
    }
    /// A string.
    pub fn str(&mut self) -> Result<String> {
        let n = self.count()?;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| self.bad("a string is not UTF-8"))
    }
    /// A block of bytes.
    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.u64()?;
        if n > self.remaining() as u64 {
            return Err(self.bad(format!(
                "a block of {n} bytes with {} left",
                self.remaining()
            )));
        }
        self.take(n as usize)
    }
    /// An optional value written by [`Writer::opt`].
    pub fn opt<T>(&mut self, f: impl FnOnce(&mut Reader<'a>) -> Result<T>) -> Result<Option<T>> {
        if self.bool()? {
            Ok(Some(f(self)?))
        } else {
            Ok(None)
        }
    }
    /// Check a four-byte tag and a version no newer than `version`.
    pub fn magic(&mut self, tag: &[u8; 4], version: u32) -> Result<u32> {
        let t = self.take(4)?;
        if t != tag {
            return Err(self.bad(format!("expected {:?}", String::from_utf8_lossy(tag))));
        }
        let v = self.u32()?;
        if v == 0 || v > version {
            return Err(self.bad(format!("version {v}; this build reads up to {version}")));
        }
        Ok(v)
    }
    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.at
    }
}

impl Writer {
    /// A four-byte tag and a version, which [`Reader::magic`] checks.
    pub fn magic(&mut self, tag: &[u8; 4], version: u32) {
        self.buf.extend(tag);
        self.u32(version);
    }
}
