//! What the title and the host were doing, most recent last, kept per thread
//! so the record costs no locking on the hot path. The crash handler prints
//! the faulting thread's record, which is the one that matters.

use std::sync::Mutex;

const KEPT: usize = 256;

#[derive(Clone, Copy)]
pub struct Entry {
    pub frame: u64,
    pub kind: &'static str,
    pub hash: u64,
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            frame: 0,
            kind: "",
            hash: 0,
            a: 0,
            b: 0,
            c: 0,
            d: 0,
        }
    }
}

struct Ring {
    at: usize,
    filled: usize,
    entries: [Entry; KEPT],
}

static RING: Mutex<Option<Ring>> = Mutex::new(None);

static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where the rolling record is written, from `VICTORY_RECENT_FILE`.
fn rolling_file() -> Option<&'static str> {
    static PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var("VICTORY_RECENT_FILE").ok())
        .as_deref()
}

/// Records one operation. Cheap: a lock that never waits, no allocation.
pub fn note(frame: u64, kind: &'static str, hash: u64, a: u32, b: u32, c: u32, d: u32) {
    // Never waits: an operation that cannot record is better lost than late.
    let Ok(mut held) = RING.try_lock() else {
        return;
    };
    let r = held.get_or_insert_with(|| Ring {
        at: 0,
        filled: 0,
        entries: [Entry::default(); KEPT],
    });
    {
        let at = r.at;
        r.entries[at] = Entry {
            frame,
            kind,
            hash,
            a,
            b,
            c,
            d,
        };
        r.at = (at + 1) % KEPT;
        if r.filled < KEPT {
            r.filled += 1;
        }
    }
    drop(held);
    // Every so often the record goes to disk, so a run that dies without an
    // exception (the title's own exit path) still says what it was doing.
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if n % 1024 == 0 {
        if let Some(path) = rolling_file() {
            let _ = std::fs::write(path, dump());
        }
    }
}

/// The record of this thread, oldest first. Empty when nothing was noted.
pub fn dump() -> String {
    let Ok(held) = RING.lock() else {
        return String::new();
    };
    let Some(r) = held.as_ref() else {
        return String::new();
    };
    {
        if r.filled == 0 {
            return String::new();
        }
        let mut out = String::with_capacity(r.filled * 72);
        out.push_str("\n  the last operations on this thread, oldest first:\n");
        let start = if r.filled < KEPT { 0 } else { r.at };
        for i in 0..r.filled {
            let e = &r.entries[(start + i) % KEPT];
            if e.kind.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "    frame {:<6} {:<9} hash {:016x} {} {} {} {}\n",
                e.frame, e.kind, e.hash, e.a, e.b, e.c, e.d
            ));
        }
        out
    }
}
