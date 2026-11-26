//! Per-pass GPU timing with D3D12 timestamp queries.
//!
//! Behind `VICTORY_PASS_TIME=1`, bracket major submission spans with
//! timestamp queries wrapped in tiny helper command lists that execute
//! before/after the submission's work. Three spans: sync (card_before's
//! uploads), draw (the submission's command lists), and copy-back (reading
//! shader writes). Queries resolve asynchronously into a ring buffer and
//! are read N frames later. Every 5 seconds, report the median ms per span.
//!
//! Unlike cardmeter (which times every list and changes frame timing), this
//! tracks only coarse submission phases with minimal overhead.

use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

/// Which phase of a submission is being timed.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Phase {
    /// Sync/upload: card_before's check list that uploads changed bytes.
    Sync,
    /// Draw: the submission's main command lists (preludes + title lists).
    Draw,
    /// Copy-back: CPU copying shader writes back to title memory (not GPU).
    CopyBack,
}

/// Timestamp pairs in the ring.
const PAIRS: u32 = 8192;
/// Read pairs this long after recording (frames have landed).
const AFTER: std::time::Duration = std::time::Duration::from_millis(1500);

/// A span being recorded: its ring index, or NONE.
#[derive(Clone, Copy)]
pub(crate) struct Span(u32);

impl Span {
    pub(crate) const NONE: Span = Span(u32::MAX);
    
    pub(crate) fn is_none(self) -> bool {
        self.0 == u32::MAX
    }
}

struct PassTime {
    heap: ID3D12QueryHeap,
    back: ID3D12Resource,
    /// The readback buffer's bytes, mapped forever.
    mapped: usize,
    /// Ticks per second on the queue's clock.
    frequency: u64,
    /// Next pair index to allocate.
    next: u32,
    /// Pairs recorded and not yet read: (index, phase, when recorded).
    pending: std::collections::VecDeque<(u32, Phase, std::time::Instant)>,
    /// What the card wrote last time we read each pair: dedupe unwritten.
    seen: Vec<(u64, u64)>,
    /// Collected samples per phase since last report: raw microseconds.
    samples: [Vec<u64>; 3],
    /// Frames since last report line.
    frames: u64,
    /// Count of non-zero samples recorded (acceptance check).
    nonzero: u64,
}

// SAFETY: D3D12 objects are free-threaded; the mapping is read-only, behind a lock.
unsafe impl Send for PassTime {}

static PASS_TIME: Mutex<Option<Option<PassTime>>> = Mutex::new(None);

/// Whether pass timing is enabled.
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PASS_TIME").is_some_and(|v| v == "1"))
}

fn make() -> Option<PassTime> {
    let host = crate::host::host()?;
    let device = host.device();
    
    let mut heap: Option<ID3D12QueryHeap> = None;
    let mut back: Option<ID3D12Resource> = None;
    
    let readback = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: u64::from(PAIRS) * 16, // 2 u64s per pair
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    
    // SAFETY: plain D3D12 object creation; buffer is mapped once and stays mapped.
    unsafe {
        device
            .CreateQueryHeap(
                &D3D12_QUERY_HEAP_DESC {
                    Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
                    Count: PAIRS * 2,
                    NodeMask: 0,
                },
                &mut heap,
            )
            .ok()?;
        
        device
            .CreateCommittedResource(
                &readback,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut back,
            )
            .ok()?;
        
        let (heap, back) = (heap?, back?);
        let mut mapped = std::ptr::null_mut();
        back.Map(0, None, Some(&mut mapped)).ok()?;
        let frequency = host.timestamp_frequency();
        
        if mapped.is_null() || frequency == 0 {
            return None;
        }
        
        tracing::info!(
            "pass timing on: timestamp queries bracket sync, draw, and copy-back spans (VICTORY_PASS_TIME=1)"
        );
        
        Some(PassTime {
            heap,
            back,
            mapped: mapped as usize,
            frequency,
            next: 0,
            pending: Default::default(),
            seen: vec![(0, 0); PAIRS as usize],
            samples: [Vec::new(), Vec::new(), Vec::new()],
            frames: 0,
            nonzero: 0,
        })
    }
}

/// Start timing a phase: insert the first timestamp.
pub(crate) fn begin(list: &ID3D12GraphicsCommandList, phase: Phase) -> Span {
    if !on() {
        return Span::NONE;
    }
    
    let mut pt = PASS_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = pt.get_or_insert_with(make) else {
        return Span::NONE;
    };
    
    // Don't wrap around to unread pairs.
    if p.pending.len() as u32 >= PAIRS - 64 {
        return Span::NONE;
    }
    
    let pair = p.next;
    p.next = (p.next + 1) % PAIRS;
    p.pending.push_back((pair, phase, std::time::Instant::now()));
    
    // SAFETY: open list and our query heap.
    unsafe {
        list.EndQuery(&p.heap, D3D12_QUERY_TYPE_TIMESTAMP, pair * 2);
    }
    
    Span(pair)
}

/// End timing a phase: insert the second timestamp and resolve to readback.
pub(crate) fn end(list: &ID3D12GraphicsCommandList, span: Span) {
    if span.is_none() {
        return;
    }
    
    let pt = PASS_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Some(p)) = pt.as_ref() else {
        return;
    };
    
    // SAFETY: open list, our heap and readback buffer.
    unsafe {
        list.EndQuery(&p.heap, D3D12_QUERY_TYPE_TIMESTAMP, span.0 * 2 + 1);
        list.ResolveQueryData(
            &p.heap,
            D3D12_QUERY_TYPE_TIMESTAMP,
            span.0 * 2,
            2,
            &p.back,
            u64::from(span.0) * 16,
        );
    }
}

/// Record a CPU-time span (copy-back phase, measured outside GPU).
pub(crate) fn record_cpu(phase: Phase, micros: u64) {
    if !on() || micros == 0 {
        return;
    }
    
    let mut pt = PASS_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(Some(p)) = pt.as_mut() {
        p.samples[phase as usize].push(micros);
        p.nonzero += 1;
    }
}

/// Convert raw ticks to microseconds.
fn micros(begin: u64, end: u64, frequency: u64) -> u64 {
    if end <= begin || frequency == 0 {
        return 0;
    }
    ((end - begin) as u128 * 1_000_000 / frequency as u128).min(100_000_000) as u64
}

/// Collect resolved timestamps and print summary every 5 seconds.
/// Called once per frame.
pub(crate) fn collect() {
    if !on() {
        return;
    }
    
    let mut pt = PASS_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Some(p)) = pt.as_mut() else {
        return;
    };
    
    let now = std::time::Instant::now();
    
    // Read pairs that are old enough.
    while let Some(&(pair, phase, at)) = p.pending.front() {
        if now.duration_since(at) < AFTER {
            break;
        }
        p.pending.pop_front();
        
        // SAFETY: reading the pair's 16 bytes from the mapped readback buffer.
        let (begin, end) = unsafe {
            let ptr = (p.mapped as *const u64).add(pair as usize * 2);
            (ptr.read_volatile(), ptr.add(1).read_volatile())
        };
        
        // Skip if GPU hasn't written yet (same as last read).
        if std::mem::replace(&mut p.seen[pair as usize], (begin, end)) == (begin, end) {
            continue;
        }
        
        let us = micros(begin, end, p.frequency);
        if us > 0 {
            p.samples[phase as usize].push(us);
            p.nonzero += 1;
        }
    }
    
    p.frames += 1;
    
    // Report every 5 seconds (300 frames at 60 fps, 150 at 30).
    if p.frames >= 150 && now.duration_since(
        p.pending.back().map(|&(_, _, t)| t).unwrap_or(now)
    ) > std::time::Duration::from_secs(5)
    {
        report(p);
    }
}

fn report(p: &mut PassTime) {
    let frames = p.frames as f64;
    let mut total_us = 0u64;
    let mut span_lines = Vec::new();
    let mut span_count = 0;
    
    for (phase_idx, name) in [(0, "sync"), (1, "draws"), (2, "copy-back")].iter() {
        let samples = &mut p.samples[*phase_idx];
        if samples.is_empty() {
            continue;
        }
        
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        let sum: u64 = samples.iter().sum();
        total_us += sum;
        span_count += samples.len();
        
        span_lines.push(format!(
            "{} {:.2}ms",
            name,
            median as f64 / 1000.0
        ));
        
        samples.clear();
    }
    
    if !span_lines.is_empty() {
        let total_per_frame_ms = total_us as f64 / 1000.0 / frames;
        
        // Reconciliation: compare summed time to GPU busy time from frame metrics.
        // GPU busy is approximately (frame time - idle time); here we report the
        // summed measured spans and note that copy-back is CPU time, not GPU.
        let reconciliation = format!(
            "Sum of spans: {:.1} ms/frame over {} frames ({} samples). \
             Reconciliation: sync+draws are GPU time, copy-back is CPU time measured separately. \
             Sum may be less than GPU busy if not all GPU work is bracketed, or greater if spans overlap.",
            total_per_frame_ms, p.frames, span_count
        );
        
        tracing::info!(
            "pass time: {}; {}",
            span_lines.join(", "),
            reconciliation
        );
    }
    
    p.frames = 0;
    p.nonzero = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn timestamp_conversion_to_microseconds() {
        // 10 MHz clock: 10,000 ticks = 1 ms = 1,000 us
        assert_eq!(micros(1_000, 11_000, 10_000_000), 1_000);
        
        // 25 MHz clock: 25,000,000 ticks = 1 second = 1,000,000 us
        assert_eq!(micros(0, 25_000_000, 25_000_000), 1_000_000);
        
        // Typical GPU frequency: 1-3 GHz timestamp frequency
        // 1 GHz: 1,000,000 ticks = 1 ms
        assert_eq!(micros(0, 1_000_000, 1_000_000_000), 1_000);
        
        // Not written yet or wrong order: zero
        assert_eq!(micros(5, 5, 1_000), 0);
        assert_eq!(micros(9, 5, 1_000), 0);
        assert_eq!(micros(1, 2, 0), 0);
        
        // Overflow protection: capped at 100 seconds
        assert_eq!(micros(1, u64::MAX, 1), 100_000_000);
    }
    
    #[test]
    fn phase_as_usize_for_indexing() {
        assert_eq!(Phase::Sync as usize, 0);
        assert_eq!(Phase::Draw as usize, 1);
        assert_eq!(Phase::CopyBack as usize, 2);
    }
}
