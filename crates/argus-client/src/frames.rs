//! Frames shared between the daemon and a client (§5: "the hot paths
//! (frame streaming, snapshot transfer) use out-of-band shared memory or a
//! sidecar binary channel that the JSON message merely points at").
//!
//! A stream is one file mapped by both processes: a header, then a few
//! slots, each a small header and a frame's RGBA8 pixels, rows top first.
//! The daemon writes the slots in turn and sends a `render.frame`
//! notification naming the slot and its sequence number; the client copies
//! that slot. Each slot's sequence number is odd while it is written and
//! even once done, so a copy that raced a write is seen and dropped.
//!
//! ```text
//! 0     magic "ARGUSFR1", version u32, slots u32, width u32, height u32,
//!       slot bytes u64                                  (64 bytes)
//! 64    slot 0: seq u64, tick u64, frame u64, width u32, height u32 (64)
//!       then width * height * 4 bytes of pixels, padded to 64
//! ...   slot 1, slot 2
//! ```
//!
//! Every slot starts on a multiple of 64, so its sequence number is an
//! aligned u64 whatever the frame's size (a 641 x 361 frame is not a
//! multiple of 8 bytes; found by the editor's first resize).

use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{fence, AtomicU64, Ordering};

/// The file's first bytes.
pub const MAGIC: &[u8; 8] = b"ARGUSFR1";
/// The layout's version.
pub const VERSION: u32 = 1;
/// Bytes before the first slot.
pub const HEADER: usize = 64;
/// Bytes of a slot before its pixels.
pub const SLOT_HEADER: usize = 64;
/// Slots in a stream: one being written, one being read, one spare.
pub const SLOTS: u32 = 3;

/// A frame as a slot's header describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    /// The sim tick drawn.
    pub tick: u64,
    /// The world frame drawn.
    pub frame: u64,
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

fn pixel_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

fn slot_bytes(width: u32, height: u32) -> usize {
    (SLOT_HEADER + pixel_bytes(width, height)).div_ceil(64) * 64
}

/// The daemon's end: writes frames into the slots in turn.
pub struct FrameWriter {
    map: MmapMut,
    path: PathBuf,
    width: u32,
    height: u32,
    next: u32,
    seq: u64,
}

impl FrameWriter {
    /// Make the file at `path` for frames of this size, and map it.
    pub fn create(path: &Path, width: u32, height: u32) -> io::Result<FrameWriter> {
        let sb = slot_bytes(width, height);
        let len = HEADER + SLOTS as usize * sb;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(len as u64)?;
        // SAFETY: the file is ours, just made at this length; the client
        // maps it read-only and takes only what the sequence numbers say
        // is whole.
        let mut map = unsafe { MmapMut::map_mut(&file)? };
        map[0..8].copy_from_slice(MAGIC);
        map[8..12].copy_from_slice(&VERSION.to_le_bytes());
        map[12..16].copy_from_slice(&SLOTS.to_le_bytes());
        map[16..20].copy_from_slice(&width.to_le_bytes());
        map[20..24].copy_from_slice(&height.to_le_bytes());
        map[24..32].copy_from_slice(&(sb as u64).to_le_bytes());
        Ok(FrameWriter {
            map,
            path: path.to_path_buf(),
            width,
            height,
            next: 0,
            seq: 0,
        })
    }

    /// The file's path, for the notification.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The frames' size.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Write the next slot: `fill` puts the pixels in (exactly `width *
    /// height * 4` bytes). Returns the slot and its sequence number.
    pub fn write<E>(
        &mut self,
        tick: u64,
        frame: u64,
        fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
    ) -> Result<(u32, u64), E> {
        let slot = self.next;
        self.next = (self.next + 1) % SLOTS;
        let sb = slot_bytes(self.width, self.height);
        let at = HEADER + slot as usize * sb;
        self.seq += 2;
        let done = self.seq;
        let seq = self.seq_at(at);
        seq.store(done - 1, Ordering::Relaxed);
        fence(Ordering::Release);
        let h = &mut self.map[at..at + SLOT_HEADER];
        h[8..16].copy_from_slice(&tick.to_le_bytes());
        h[16..24].copy_from_slice(&frame.to_le_bytes());
        h[24..28].copy_from_slice(&self.width.to_le_bytes());
        h[28..32].copy_from_slice(&self.height.to_le_bytes());
        let px = pixel_bytes(self.width, self.height);
        let r = fill(&mut self.map[at + SLOT_HEADER..at + SLOT_HEADER + px]);
        // A slot left half written stays odd: a reader never takes it.
        r?;
        self.seq_at(at).store(done, Ordering::Release);
        Ok((slot, done))
    }

    fn seq_at(&self, at: usize) -> &AtomicU64 {
        // SAFETY: `at` is a multiple of 64 from a page-aligned mapping, so
        // the u64 is aligned; the mapping lives as long as `self`, and the
        // other process touches it only through atomic loads.
        unsafe { &*(self.map.as_ptr().add(at) as *const AtomicU64) }
    }
}

impl Drop for FrameWriter {
    fn drop(&mut self) {
        // A client still mapping it keeps it until it lets go.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The client's end: reads the slot a notification names.
pub struct FrameReader {
    map: Mmap,
    path: PathBuf,
    width: u32,
    height: u32,
    slots: u32,
    slot_bytes: usize,
}

impl FrameReader {
    /// Map a stream's file.
    pub fn open(path: &Path) -> io::Result<FrameReader> {
        let file = File::open(path)?;
        // SAFETY: mapped read-only; the daemon writes it, and `read` takes
        // a slot only between two equal even sequence numbers.
        let map = unsafe { Mmap::map(&file)? };
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
        if map.len() < HEADER || &map[0..8] != MAGIC {
            return Err(bad("not a frame stream"));
        }
        let u32_at = |i: usize| u32::from_le_bytes(map[i..i + 4].try_into().expect("4 bytes"));
        if u32_at(8) != VERSION {
            return Err(bad("a frame stream of another version"));
        }
        let (slots, width, height) = (u32_at(12), u32_at(16), u32_at(20));
        let sb = u64::from_le_bytes(map[24..32].try_into().expect("8 bytes")) as usize;
        if !sb.is_multiple_of(64) || sb < SLOT_HEADER + pixel_bytes(width, height) {
            return Err(bad("a frame stream whose slots do not fit its frames"));
        }
        if map.len() < HEADER + slots as usize * sb {
            return Err(bad("a frame stream shorter than its header says"));
        }
        Ok(FrameReader {
            map,
            path: path.to_path_buf(),
            width,
            height,
            slots,
            slot_bytes: sb,
        })
    }

    /// The file this reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The frames' size.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Copy slot `slot` into `out` if it still holds frame `seq`, whole.
    pub fn read(&self, slot: u32, seq: u64, out: &mut Vec<u8>) -> Option<FrameInfo> {
        if slot >= self.slots || seq % 2 == 1 {
            return None;
        }
        let at = HEADER + slot as usize * self.slot_bytes;
        // SAFETY: as in `FrameWriter::seq_at`.
        let s = unsafe { &*(self.map.as_ptr().add(at) as *const AtomicU64) };
        if s.load(Ordering::Acquire) != seq {
            return None;
        }
        let h = &self.map[at..at + SLOT_HEADER];
        let u64_at = |i: usize| u64::from_le_bytes(h[i..i + 8].try_into().expect("8 bytes"));
        let u32_at = |i: usize| u32::from_le_bytes(h[i..i + 4].try_into().expect("4 bytes"));
        let info = FrameInfo {
            tick: u64_at(8),
            frame: u64_at(16),
            width: u32_at(24),
            height: u32_at(28),
        };
        out.clear();
        let px = pixel_bytes(self.width, self.height);
        out.extend_from_slice(&self.map[at + SLOT_HEADER..at + SLOT_HEADER + px]);
        fence(Ordering::Acquire);
        (s.load(Ordering::Relaxed) == seq).then_some(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_stay_aligned_for_any_size() {
        let p = std::env::temp_dir().join(format!("argus-frames-odd-{}.bin", std::process::id()));
        let mut w = FrameWriter::create(&p, 641, 361).unwrap();
        let r = FrameReader::open(&p).unwrap();
        let mut out = Vec::new();
        for i in 0..4u64 {
            let (slot, seq) = w
                .write(i, i, |px| {
                    px.fill(i as u8);
                    Ok::<(), ()>(())
                })
                .unwrap();
            let info = r.read(slot, seq, &mut out).unwrap();
            assert_eq!((info.tick, out.len()), (i, 641 * 361 * 4));
            assert!(out.iter().all(|b| *b == i as u8));
        }
    }

    #[test]
    fn a_frame_written_is_read_whole_and_an_old_one_is_not() {
        let p = std::env::temp_dir().join(format!("argus-frames-test-{}.bin", std::process::id()));
        let mut w = FrameWriter::create(&p, 4, 2).unwrap();
        let r = FrameReader::open(&p).unwrap();
        assert_eq!(r.size(), (4, 2));
        let (slot, seq) = w
            .write(7, 9, |px| {
                px.iter_mut().enumerate().for_each(|(i, b)| *b = i as u8);
                Ok::<(), ()>(())
            })
            .unwrap();
        let mut out = Vec::new();
        let info = r.read(slot, seq, &mut out).unwrap();
        assert_eq!(
            info,
            FrameInfo {
                tick: 7,
                frame: 9,
                width: 4,
                height: 2
            }
        );
        assert_eq!(out, (0..32).map(|i| i as u8).collect::<Vec<_>>());
        // Three more frames: the first slot now holds a newer one.
        for t in 0..3 {
            w.write(t, t, |px| {
                px.fill(1);
                Ok::<(), ()>(())
            })
            .unwrap();
        }
        assert_eq!(r.read(slot, seq, &mut out), None, "overwritten");
        // A frame whose drawing failed is never taken.
        let failed = w.write(1, 1, |_| Err::<(), &str>("no GPU"));
        assert_eq!(failed, Err("no GPU"));
        drop(r);
        drop(w);
        assert!(!p.exists(), "the file goes with the stream");
    }
}
