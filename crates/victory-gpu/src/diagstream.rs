//! Diagnostics: a live stats stream for the PalmVI dashboard (`tools/dash`).
//!
//! With `VICTORY_DIAG_PORT=<port>` a small HTTP server on 127.0.0.1 (never
//! another interface) serves:
//!
//! * `GET /events`: server-sent events, a `snapshot` about every 250 ms and
//!   an `ev` for each event (stall begun or ended, shader that does not
//!   translate, scripted press, device removed, DRED, error);
//! * `GET /snapshot`: one snapshot as JSON.
//!
//! The schema is versioned in `docs/dashboard.md` ([`SCHEMA`]).
//!
//! The numbers come from the log (a tracing layer, [`layer`], sees every
//! event the filter lets through) and from counters this module reads on its
//! own threads, so the stream keeps going while the render thread is stuck:
//! that is when it says how long no frame has come out and why (the stall
//! reporter's events, target `victory_gpu::stall`). Nothing is written to
//! disk. Without the variable no layer, thread or socket exists.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// The version of the JSON this serves; bump it on any incompatible change
/// and say what changed in `docs/dashboard.md`.
pub const SCHEMA: u32 = 1;

/// How often a client gets a snapshot.
const PERIOD: Duration = Duration::from_millis(250);
/// No new frame for this long (once frames have come out) is a stall.
const STALL: Duration = Duration::from_millis(1000);
/// At most this many clients at once.
const CLIENTS: usize = 8;
/// Kept for late joiners and the timeline.
const EVENTS: usize = 256;
const STALLS: usize = 64;
const RECENT_SHADERS: usize = 64;
const WARN_KINDS: usize = 500;

/// The tracing layer that feeds the stream, with its server started, or
/// `None` when `VICTORY_DIAG_PORT` is unset or the port cannot be had.
pub fn layer() -> Option<DiagLayer> {
    let port: u16 = std::env::var("VICTORY_DIAG_PORT")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    // Our own audio recorder child inherits the variable; the parent has
    // the port, so the child simply fails to bind and serves nothing.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).ok()?;
    lock().started = Some(Instant::now());
    let _ = std::thread::Builder::new()
        .name("palmvi-diag-http".into())
        .spawn(move || serve(listener));
    let _ = std::thread::Builder::new()
        .name("palmvi-diag-probe".into())
        .spawn(probe);
    Some(DiagLayer)
}

/// Sets a named gauge (e.g. `descriptor_heap_used`), shown as is; for hooks
/// elsewhere that have a number with no log line of its own. Cheap; a no-op
/// unless the stream is on.
pub fn gauge(name: &'static str, value: f64) {
    let mut s = lock();
    if s.started.is_some() {
        s.gauges.insert(name, value);
    }
}

// ---- state ----

#[derive(Default)]
struct Timing {
    fps: f64,
    ms: f64,
    prepare_ms: f64,
    copies_ms: f64,
    copies_mb: f64,
    textures_ms: f64,
    textures_mb: f64,
    gpu_ms: f64,
    copy_back_ms: f64,
    copy_back_mb: f64,
    draws: f64,
    dropped: f64,
    making: f64,
    indirect: f64,
    at: f64,
}

struct Shader {
    hash: String,
    stage: String,
    reason: String,
    at: f64,
}

struct StallSpan {
    start: f64,
    end: Option<f64>,
    frame: u64,
    cause: String,
}

#[derive(Default)]
struct State {
    started: Option<Instant>,
    seq: u64,
    events: VecDeque<(u64, String)>,
    timing: Option<Timing>,
    /// The latest drop table (per frame, by reason) and the one being read.
    drops: Vec<(String, f64)>,
    drops_reading: bool,
    copies: Vec<(String, f64)>,
    shadow: Vec<(String, f64)>,
    direct: Vec<(String, f64)>,
    shaders: VecDeque<Shader>,
    shaders_seen: std::collections::HashSet<String>,
    /// Shaders that did translate ("shader translated").
    translated: u64,
    shader_reasons: HashMap<String, u64>,
    warns: HashMap<String, u64>,
    warn_total: u64,
    errors: u64,
    presses: u64,
    last_press: Option<(String, f64)>,
    pad_script: Option<String>,
    device_removed: u64,
    dred_lines: u64,
    heap_overflow: Option<(u64, u64, bool)>,
    compute_waits: u64,
    last_wait_ms: Option<f64>,
    /// The stall reporter's latest word on what the frame waits for.
    stall_cause: Option<String>,
    stall_cause_ms: Option<f64>,
    stalls: VecDeque<StallSpan>,
    // Sampled by the probe.
    frame: u64,
    drawn: u64,
    making: u64,
    fps_live: f64,
    since_progress_ms: u64,
    stalled: bool,
    vram_used: Option<u64>,
    vram_budget: Option<u64>,
    working_set: Option<u64>,
    gauges: BTreeMap<&'static str, f64>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

struct Guard(std::sync::MutexGuard<'static, Option<State>>);

impl std::ops::Deref for Guard {
    type Target = State;
    fn deref(&self) -> &State {
        self.0.as_ref().expect("initialised in lock()")
    }
}

impl std::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut State {
        self.0.as_mut().expect("initialised in lock()")
    }
}

/// Never log while holding this: the layer takes it too.
fn lock() -> Guard {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(State::default);
    Guard(g)
}

impl State {
    fn now(&self) -> f64 {
        self.started.map_or(0.0, |s| s.elapsed().as_secs_f64())
    }

    fn event(&mut self, kind: &str, text: &str, extra: &[(&str, Num)]) {
        self.seq += 1;
        let mut j = Json::object();
        j.num("seq", self.seq as f64);
        j.num("t", round(self.now(), 3));
        j.str("kind", kind);
        j.str("text", text);
        for (k, v) in extra {
            match v {
                Num::F(x) => j.num(k, *x),
                Num::S(s) => j.str(k, s),
            }
        }
        if self.events.len() == EVENTS {
            self.events.pop_front();
        }
        self.events.push_back((self.seq, j.end()));
    }
}

enum Num {
    F(f64),
    S(String),
}

// ---- the layer ----

/// Feeds the stream from the log; see [`layer`].
pub struct DiagLayer;

thread_local! {
    static INSIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl<S: Subscriber> Layer<S> for DiagLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        let stall = meta.target().starts_with("victory_gpu::stall");
        if *meta.level() > Level::INFO && !stall {
            return;
        }
        // A log line from inside our own code must not take the lock twice.
        if INSIDE.with(|i| i.replace(true)) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        note(&mut lock(), *meta.level(), meta.target(), &fields);
        INSIDE.with(|i| i.set(false));
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    values: Vec<(&'static str, String)>,
}

impl Fields {
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    fn num(&self, name: &str) -> Option<f64> {
        self.get(name)?.parse().ok()
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.values.push((field.name(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_owned();
        } else {
            self.values.push((field.name(), value.to_owned()));
        }
    }
}

/// Takes in one log event.
fn note(s: &mut State, level: Level, target: &str, f: &Fields) {
    let m = f.message.as_str();
    if target.starts_with("victory_gpu::stall") {
        // #64's stall reporter: "stalled frame=N for X.Xs: <cause>" each second
        // while stuck, "stall ended frame=N after X.Xs: <cause> ..." after.
        if m.starts_with("stall ended") || m.contains("resumed") {
            s.stall_cause = None;
            s.stall_cause_ms = None;
        } else {
            let cause = m.split_once(": ").map_or(m, |(_, c)| c).to_owned();
            s.stall_cause_ms = after(m, " for ").map(|secs| secs * 1000.0).or(f.num("ms"));
            if let Some(span) = s.stalls.back_mut().filter(|x| x.end.is_none()) {
                span.cause.clone_from(&cause);
            }
            s.stall_cause = Some(cause);
        }
        // Once a second while stuck: kept out of the WARN tally.
        return;
    }
    if level == Level::WARN || level == Level::ERROR {
        s.warn_total += u64::from(level == Level::WARN);
        s.errors += u64::from(level == Level::ERROR);
        let key = mask(&with_fields_except(f, &["hash"]));
        if s.warns.len() < WARN_KINDS || s.warns.contains_key(&key) {
            *s.warns.entry(key).or_default() += 1;
        }
        if level == Level::ERROR {
            let t = with_fields(f);
            s.event("error", &t, &[]);
        }
    }
    if m.contains(" fps, ") && m.contains("ms/frame") {
        s.timing = Some(parse_timing(m, s.now()));
        s.drops_reading = true;
        s.drops.clear();
    } else if let Some(rest) = m.strip_prefix("dropped ")
        && let Some((n, why)) = rest.split_once("/frame without a host pipeline: ")
    {
        if !s.drops_reading {
            s.drops.clear();
            s.drops_reading = true;
        }
        if let Ok(n) = n.parse() {
            s.drops.push((why.to_owned(), n));
        }
    } else if m.starts_with("copies per frame:") {
        s.drops_reading = false;
        s.copies = parse_copies(m);
    } else if m.starts_with("direct SRVs per frame:") {
        s.direct = [
            ("slots", before(m, " slots")),
            ("ranges", before(m, " merged ranges")),
            ("requested_mb", before(m, " MB requested")),
            ("filled_mb", before(m, " MB filled")),
            ("ms", before(m, " ms preparing")),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k.to_owned(), v?)))
        .collect();
    } else if m == "shadow traffic per frame" {
        s.shadow = f
            .values
            .iter()
            .filter_map(|(k, v)| Some(((*k).to_owned(), v.parse().ok()?)))
            .collect();
    } else if m == "shader translated" {
        s.translated += 1;
    } else if m.starts_with("shader does not translate") {
        let hash = f.get("hash").unwrap_or("?").to_owned();
        let reason = mask(f.get("error").unwrap_or("unknown"));
        let reason: String = reason.chars().take(160).collect();
        if s.shaders_seen.insert(hash.clone()) {
            *s.shader_reasons.entry(reason.clone()).or_default() += 1;
            let stage = f.get("stage").unwrap_or("?").to_owned();
            let at = s.now();
            s.event(
                "untranslated",
                &reason,
                &[
                    ("hash", Num::S(hash.clone())),
                    ("stage", Num::S(stage.clone())),
                ],
            );
            if s.shaders.len() == RECENT_SHADERS {
                s.shaders.pop_front();
            }
            s.shaders.push_back(Shader {
                hash,
                stage,
                reason,
                at,
            });
        }
    } else if m == "scripted press" {
        s.presses += 1;
        let key = f.get("key").unwrap_or("?").to_owned();
        let at = f.num("at").unwrap_or(0.0);
        s.event("press", &key, &[("at", Num::F(at))]);
        s.last_press = Some((key, at));
    } else if m == "title slot past the host heap" {
        let n = |k| f.num(k).unwrap_or(0.0) as u64;
        s.heap_overflow = Some((n("index"), n("count"), f.get("sampler") == Some("true")));
    }
    let lower = m.to_ascii_lowercase();
    if lower.contains("device removed") || lower.contains("device lost") {
        s.device_removed += 1;
        let t = with_fields(f);
        s.event("device_removed", &t, &[]);
    } else if lower.contains("dred")
        || lower.contains("breadcrumb")
        || lower.contains("page fault at")
    {
        s.dred_lines += 1;
        let t = with_fields(f);
        s.event("dred", &t, &[]);
    }
    if lower.contains("waited for")
        && let Some(ms) = f.num("ms")
    {
        s.compute_waits += 1;
        s.last_wait_ms = Some(ms);
        s.event("wait", m, &[("ms", Num::F(ms))]);
    }
}

fn with_fields(f: &Fields) -> String {
    with_fields_except(f, &[])
}

fn with_fields_except(f: &Fields, skip: &[&str]) -> String {
    let mut t = f.message.clone();
    for (k, v) in &f.values {
        if !skip.contains(k) {
            let _ = write!(t, " {k}={v}");
        }
    }
    t
}

/// Numbers, `0x` values and 16-digit hashes as `#` (as tools/world-run.ps1
/// does for its WARN tally).
pub(crate) fn mask(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let word_start = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if word_start && b[i] == b'0' && b.get(i + 1) == Some(&b'x') {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j > i + 2 {
                out.push('#');
                i = j;
                continue;
            }
        }
        if word_start {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j - i == 16 && (j == b.len() || !b[j].is_ascii_alphanumeric()) {
                out.push('#');
                i = j;
                continue;
            }
        }
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            out.push('#');
            i = j;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The number written just before `suffix` (its first occurrence).
fn before(text: &str, suffix: &str) -> Option<f64> {
    let end = text.find(suffix)?;
    let head = &text[..end];
    let start = head
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .map_or(0, |i| i + 1);
    head[start..].parse().ok()
}

/// The number written just after `prefix` (its first occurrence).
fn after(text: &str, prefix: &str) -> Option<f64> {
    let start = text.find(prefix)? + prefix.len();
    let tail = &text[start..];
    let end = tail
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(tail.len());
    tail[..end].parse().ok()
}

/// The host's frame-rate line (host.rs `Meter::frame`), by its labels so a
/// field added to it does not shift the others.
fn parse_timing(m: &str, at: f64) -> Timing {
    let g = |v: Option<f64>| v.unwrap_or(0.0);
    Timing {
        fps: g(before(m, " fps")),
        ms: g(before(m, " ms/frame")),
        prepare_ms: g(before(m, " preparing")),
        copies_ms: g(after(m, "copies ")),
        copies_mb: g(before(m, " MB, textures")),
        textures_ms: g(after(m, "textures ")),
        textures_mb: g(before(m, " MB; ")),
        gpu_ms: g(before(m, " on the GPU")),
        copy_back_ms: g(before(m, " copying back")),
        copy_back_mb: g(after(m, "copying back ")),
        draws: g(before(m, " draws/frame")),
        dropped: g(after(m, "dropped ")),
        making: g(before(m, " being made")),
        indirect: g(before(m, " indirect")),
        at,
    }
}

/// The copy statistics line (copystat.rs `log`).
fn parse_copies(m: &str) -> Vec<(String, f64)> {
    [
        ("submissions", before(m, " submissions")),
        ("title_copies", before(m, " title copies")),
        ("title_copies_mb", after(m, " title copies ")),
        ("refreshes", before(m, " refreshes")),
        ("refreshes_mb", after(m, " refreshes ")),
        ("skipped", before(m, " skipped")),
        ("runs", before(m, " runs")),
        ("page_faults", before(m, " page faults")),
        ("watches", before(m, " watches")),
        ("watches_ms", after(m, " watches ")),
        ("fallbacks", before(m, " fallbacks")),
        ("guard_faults", before(m, " guard faults")),
    ]
    .into_iter()
    .filter_map(|(k, v)| Some((k.to_owned(), v?)))
    .collect()
}

// ---- the probe: counters read off the render thread ----

fn probe() {
    let mut last_frame = (0u64, 0u64);
    let mut progress = Instant::now();
    let mut window: VecDeque<(Instant, u64)> = VecDeque::new();
    let mut memory_at: Option<Instant> = None;
    let mut adapters = Adapters::new();
    loop {
        let frame = crate::frame::current();
        let drawn = crate::draw::drawn();
        let making = crate::pipeline::making();
        let now = Instant::now();
        if (frame, drawn) != last_frame {
            last_frame = (frame, drawn);
            progress = now;
        }
        window.push_back((now, frame));
        while window.len() > 2 && now - window[0].0 > Duration::from_secs(2) {
            window.pop_front();
        }
        let fps_live = match (window.front(), window.back()) {
            (Some(a), Some(b)) if b.0 > a.0 => (b.1 - a.1) as f64 / (b.0 - a.0).as_secs_f64(),
            _ => 0.0,
        };
        let memory = if memory_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            memory_at = Some(now);
            Some((adapters.vram(), working_set()))
        } else {
            None
        };
        {
            let mut s = lock();
            s.frame = frame;
            s.drawn = drawn;
            s.making = making;
            s.fps_live = fps_live;
            let quiet = now - progress;
            s.since_progress_ms = quiet.as_millis() as u64;
            let stalled = frame > 0 && quiet >= STALL;
            if stalled && !s.stalled {
                let start = s.now() - quiet.as_secs_f64();
                let cause = s.stall_cause.clone().unwrap_or_else(|| guess(&s));
                if s.stalls.len() == STALLS {
                    s.stalls.pop_front();
                }
                s.stalls.push_back(StallSpan {
                    start,
                    end: None,
                    frame,
                    cause: cause.clone(),
                });
                s.event("stall_begin", &cause, &[("frame", Num::F(frame as f64))]);
            } else if !stalled && s.stalled {
                let end = s.now();
                let mut lasted = 0.0;
                if let Some(span) = s.stalls.back_mut() {
                    span.end = Some(end);
                    lasted = end - span.start;
                }
                s.event(
                    "stall_end",
                    "frames again",
                    &[("seconds", Num::F(round(lasted, 2)))],
                );
            }
            s.stalled = stalled;
            if let Some((vram, ws)) = memory {
                if let Some((used, budget)) = vram {
                    s.vram_used = Some(used);
                    s.vram_budget = Some(budget);
                }
                s.working_set = ws;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A cause when the stall reporter has said nothing.
fn guess(s: &State) -> String {
    if s.making > 0 {
        format!("unknown; {} pipelines being made", s.making)
    } else {
        "unknown".to_owned()
    }
}

/// The adapters' local memory as used by this process (the largest).
struct Adapters(Vec<windows::Win32::Graphics::Dxgi::IDXGIAdapter3>);

impl Adapters {
    fn new() -> Self {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1};
        use windows::core::Interface;
        let mut found = Vec::new();
        // SAFETY: plain DXGI enumeration on our own thread.
        unsafe {
            if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
                let mut i = 0;
                while let Ok(a) = factory.EnumAdapters1(i) {
                    if let Ok(a3) = a.cast::<IDXGIAdapter3>() {
                        found.push(a3);
                    }
                    i += 1;
                }
            }
        }
        Adapters(found)
    }

    /// (used, budget) bytes.
    fn vram(&mut self) -> Option<(u64, u64)> {
        use windows::Win32::Graphics::Dxgi::{
            DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
        };
        self.0
            .iter()
            .filter_map(|a| {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                // SAFETY: a live adapter and a sized structure.
                unsafe { a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }
                    .ok()?;
                Some((info.CurrentUsage, info.Budget))
            })
            .max_by_key(|&(used, _)| used)
    }
}

fn working_set() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: our own process and a sized structure.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some(c.WorkingSetSize as u64)
}

// ---- JSON ----

/// A JSON object written by hand (no serde in this crate).
struct Json {
    out: String,
    first: Vec<bool>,
}

impl Json {
    fn object() -> Self {
        Json {
            out: "{".into(),
            first: vec![true],
        }
    }

    fn key(&mut self, k: &str) {
        if let Some(first) = self.first.last_mut() {
            if !*first {
                self.out.push(',');
            }
            *first = false;
        }
        if !k.is_empty() {
            escape(&mut self.out, k);
            self.out.push(':');
        }
    }

    fn num(&mut self, k: &str, v: f64) {
        self.key(k);
        if v.is_finite() {
            let _ = write!(self.out, "{}", round(v, 3));
        } else {
            self.out.push_str("null");
        }
    }

    fn opt(&mut self, k: &str, v: Option<f64>) {
        match v {
            Some(v) => self.num(k, v),
            None => self.null(k),
        }
    }

    fn null(&mut self, k: &str) {
        self.key(k);
        self.out.push_str("null");
    }

    fn bool(&mut self, k: &str, v: bool) {
        self.key(k);
        self.out.push_str(if v { "true" } else { "false" });
    }

    fn str(&mut self, k: &str, v: &str) {
        self.key(k);
        escape(&mut self.out, v);
    }

    fn open(&mut self, k: &str, bracket: char) {
        self.key(k);
        self.out.push(bracket);
        self.first.push(true);
    }

    fn close(&mut self, bracket: char) {
        self.first.pop();
        self.out.push(bracket);
    }

    /// `{"k": v, ...}` from pairs.
    fn pairs(&mut self, k: &str, pairs: &[(String, f64)]) {
        self.open(k, '{');
        for (name, v) in pairs {
            self.num(name, *v);
        }
        self.close('}');
    }

    fn end(mut self) -> String {
        self.out.push('}');
        self.out
    }
}

fn escape(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn round(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (v * k).round() / k
}

/// The snapshot (schema [`SCHEMA`], see docs/dashboard.md).
fn snapshot(s: &State) -> String {
    let mut j = Json::object();
    j.num("schema", f64::from(SCHEMA));
    j.num("t", round(s.now(), 3));
    j.num("seq", s.seq as f64);
    j.open("frame", '{');
    j.num("count", s.frame as f64);
    j.num("fps_live", round(s.fps_live, 2));
    j.num("draws_total", s.drawn as f64);
    j.num("since_progress_ms", s.since_progress_ms as f64);
    j.close('}');
    match &s.timing {
        Some(t) => {
            j.open("timing", '{');
            for (k, v) in [
                ("fps", t.fps),
                ("ms", t.ms),
                ("prepare_ms", t.prepare_ms),
                ("copies_ms", t.copies_ms),
                ("copies_mb", t.copies_mb),
                ("textures_ms", t.textures_ms),
                ("textures_mb", t.textures_mb),
                ("gpu_wait_ms", t.gpu_ms),
                ("copy_back_ms", t.copy_back_ms),
                ("copy_back_mb", t.copy_back_mb),
                ("draws", t.draws),
                ("dropped", t.dropped),
                ("being_made", t.making),
                ("indirect", t.indirect),
                ("at", t.at),
            ] {
                j.num(k, v);
            }
            j.null("present_ms");
            j.close('}');
        }
        None => j.null("timing"),
    }
    j.open("pipelines", '{');
    j.num("queued", s.making as f64);
    j.num("compute_waits", s.compute_waits as f64);
    j.opt("last_wait_ms", s.last_wait_ms);
    j.close('}');
    j.open("stall", '{');
    j.bool("active", s.stalled);
    j.num("since_ms", s.since_progress_ms as f64);
    match (&s.stall_cause, s.stalled) {
        (Some(c), _) => j.str("cause", c),
        (None, true) => j.str("cause", &guess(s)),
        (None, false) => j.null("cause"),
    }
    j.opt("cause_ms", s.stall_cause_ms);
    j.close('}');
    j.open("stalls", '[');
    for span in &s.stalls {
        j.open("", '{');
        j.num("start", round(span.start, 2));
        j.opt("end", span.end.map(|e| round(e, 2)));
        j.num("frame", span.frame as f64);
        j.str("cause", &span.cause);
        j.close('}');
    }
    j.close(']');
    j.open("drops", '[');
    let mut drops = s.drops.clone();
    drops.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (why, n) in &drops {
        j.open("", '{');
        j.str("why", why);
        j.num("per_frame", *n);
        j.close('}');
    }
    j.close(']');
    j.open("untranslated", '{');
    j.num("count", s.shaders_seen.len() as f64);
    j.num("translated", s.translated as f64);
    j.open("by_reason", '[');
    let mut reasons: Vec<_> = s.shader_reasons.iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (reason, n) in reasons {
        j.open("", '{');
        j.str("reason", reason);
        j.num("count", *n as f64);
        j.close('}');
    }
    j.close(']');
    j.open("recent", '[');
    for sh in s.shaders.iter().rev() {
        j.open("", '{');
        j.str("hash", &sh.hash);
        j.str("stage", &sh.stage);
        j.str("reason", &sh.reason);
        j.num("t", round(sh.at, 2));
        j.close('}');
    }
    j.close(']');
    j.close('}');
    j.open("heap", '{');
    match s.heap_overflow {
        Some((index, count, sampler)) => {
            j.open("overflow", '{');
            j.num("index", index as f64);
            j.num("count", count as f64);
            j.bool("sampler", sampler);
            j.close('}');
        }
        None => j.null("overflow"),
    }
    j.close('}');
    j.open("memory", '{');
    j.opt("vram_used_mb", s.vram_used.map(|b| b as f64 / 1048576.0));
    j.opt(
        "vram_budget_mb",
        s.vram_budget.map(|b| b as f64 / 1048576.0),
    );
    j.opt(
        "working_set_mb",
        s.working_set.map(|b| b as f64 / 1048576.0),
    );
    j.close('}');
    j.pairs("copies", &s.copies);
    j.pairs("shadow", &s.shadow);
    j.pairs("direct_srvs", &s.direct);
    j.open("warns", '{');
    j.num("total", s.warn_total as f64);
    j.num("errors", s.errors as f64);
    j.open("top", '[');
    let mut warns: Vec<_> = s.warns.iter().collect();
    warns.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (text, n) in warns.into_iter().take(40) {
        j.open("", '{');
        j.str("text", text);
        j.num("count", *n as f64);
        j.close('}');
    }
    j.close(']');
    j.close('}');
    j.open("pad", '{');
    j.num("presses", s.presses as f64);
    match &s.pad_script {
        Some(p) => j.str("script", p),
        None => j.null("script"),
    }
    match &s.last_press {
        Some((key, at)) => {
            j.str("last_key", key);
            j.num("last_at", *at);
        }
        None => {
            j.null("last_key");
            j.null("last_at");
        }
    }
    j.close('}');
    j.open("device", '{');
    j.num("removed", s.device_removed as f64);
    j.num("dred_lines", s.dred_lines as f64);
    j.close('}');
    let gauges: Vec<(String, f64)> = s
        .gauges
        .iter()
        .map(|(k, v)| ((*k).to_owned(), *v))
        .collect();
    j.pairs("gauges", &gauges);
    j.end()
}

// ---- the server ----

static CONNECTED: AtomicUsize = AtomicUsize::new(0);

fn serve(listener: TcpListener) {
    {
        let mut s = lock();
        s.pad_script = std::env::var("VICTORY_PAD_SCRIPT")
            .ok()
            .filter(|p| !p.is_empty());
    }
    for stream in listener.incoming().flatten() {
        if CONNECTED.load(Ordering::Relaxed) >= CLIENTS {
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        CONNECTED.fetch_add(1, Ordering::Relaxed);
        let _ = std::thread::Builder::new()
            .name("palmvi-diag-client".into())
            .spawn(move || {
                let _ = client(stream);
                CONNECTED.fetch_sub(1, Ordering::Relaxed);
            });
    }
}

fn client(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut buffer = [0u8; 2048];
    let mut got = 0;
    while got < buffer.len() {
        let n = stream.read(&mut buffer[got..])?;
        if n == 0 {
            break;
        }
        got += n;
        if buffer[..got].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&buffer[..got]);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    match path {
        "/events" => {
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\
                  Connection: keep-alive\r\n\r\nretry: 1000\n\n",
            )?;
            // The buffered events first, so a late joiner has the timeline.
            let mut seen = 0;
            loop {
                let (snap, events) = {
                    let s = lock();
                    let events: Vec<_> = s
                        .events
                        .iter()
                        .filter(|(n, _)| *n > seen)
                        .map(|(n, e)| (*n, e.clone()))
                        .collect();
                    (snapshot(&s), events)
                };
                let mut out = String::new();
                for (n, e) in events {
                    seen = n;
                    let _ = write!(out, "event: ev\ndata: {e}\n\n");
                }
                let _ = write!(out, "event: snapshot\ndata: {snap}\n\n");
                stream.write_all(out.as_bytes())?;
                stream.flush()?;
                std::thread::sleep(PERIOD);
            }
        }
        "/snapshot" => {
            let body = snapshot(&lock());
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(body.as_bytes())
        }
        _ => stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 39\r\n\r\n\
              PalmVI diag stream: /events, /snapshot\n",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(message: &str, values: &[(&'static str, &str)]) -> Fields {
        Fields {
            message: message.into(),
            values: values.iter().map(|(k, v)| (*k, (*v).to_owned())).collect(),
        }
    }

    /// A minimal JSON well-formedness check: balanced brackets outside
    /// strings, valid escapes, nothing after the end.
    fn well_formed(s: &str) -> bool {
        let mut depth: Vec<char> = Vec::new();
        let mut chars = s.chars();
        let mut in_str = false;
        let mut ended = false;
        while let Some(c) = chars.next() {
            if ended && !c.is_whitespace() {
                return false;
            }
            if in_str {
                match c {
                    '\\' => match chars.next() {
                        Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't') => {}
                        Some('u') => {
                            for _ in 0..4 {
                                if !chars.next().is_some_and(|h| h.is_ascii_hexdigit()) {
                                    return false;
                                }
                            }
                        }
                        _ => return false,
                    },
                    '"' => in_str = false,
                    c if (c as u32) < 0x20 => return false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' | '[' => depth.push(c),
                '}' => {
                    if depth.pop() != Some('{') {
                        return false;
                    }
                    ended = depth.is_empty();
                }
                ']' if depth.pop() != Some('[') => return false,
                _ => {}
            }
        }
        !in_str && depth.is_empty() && ended
    }

    const FPS: &str = "12.5 fps, 80.0 ms/frame (41.2 preparing: copies 3.1 ms 25.0 MB, \
        textures 1.5 ms 4.0 MB; 30.4 on the GPU; 2.2 copying back 9.5 MB), 7012 draws/frame, \
        dropped 31 without a host pipeline (4 being made) and 2 indirect";

    #[test]
    fn reads_the_frame_rate_line() {
        let t = parse_timing(FPS, 1.0);
        assert_eq!(
            [
                t.fps,
                t.ms,
                t.prepare_ms,
                t.copies_ms,
                t.copies_mb,
                t.textures_ms,
                t.textures_mb
            ],
            [12.5, 80.0, 41.2, 3.1, 25.0, 1.5, 4.0]
        );
        assert_eq!(
            [
                t.gpu_ms,
                t.copy_back_ms,
                t.copy_back_mb,
                t.draws,
                t.dropped,
                t.making,
                t.indirect
            ],
            [30.4, 2.2, 9.5, 7012.0, 31.0, 4.0, 2.0]
        );
    }

    #[test]
    fn masks_numbers_and_hashes() {
        assert_eq!(
            mask("slot 12 at 0xdeadBEEF hash 0123456789abcdef v1.5 done"),
            "slot # at # hash # v#.# done".replace("v#.#", "v#")
        );
        assert_eq!(mask("plain words"), "plain words");
        assert_eq!(mask("café 3"), "café #");
    }

    #[test]
    fn escapes_strings() {
        let mut out = String::new();
        escape(&mut out, "a\"b\\c\nd\u{1}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn snapshot_is_well_formed_json() {
        let mut s = State {
            started: Some(Instant::now()),
            ..State::default()
        };
        let empty = snapshot(&s);
        assert!(well_formed(&empty), "{empty}");
        assert!(empty.starts_with("{\"schema\":1,"));
        assert!(empty.contains("\"timing\":null"));
        note(&mut s, Level::INFO, "victory_gpu::host", &fields(FPS, &[]));
        note(
            &mut s,
            Level::INFO,
            "victory_gpu::host",
            &fields(
                "dropped 3.5/frame without a host pipeline: \"quoted\" reason",
                &[],
            ),
        );
        note(
            &mut s,
            Level::WARN,
            "victory_gpu::pipeline",
            &fields(
                "shader does not translate; its pipelines' draws are dropped",
                &[
                    ("stage", "1"),
                    ("hash", "0123456789abcdef"),
                    ("error", "lane 64 at 0x10"),
                ],
            ),
        );
        note(
            &mut s,
            Level::INFO,
            "victory_hle::gameinput",
            &fields("scripted press", &[("at", "30"), ("key", "A")]),
        );
        note(
            &mut s,
            Level::WARN,
            "victory_gpu::stall",
            &fields("frame waits on a compute pipeline", &[("ms", "2500")]),
        );
        note(
            &mut s,
            Level::ERROR,
            "victory_gpu::host",
            &fields("host GPU device removed after a submission of 3 lists", &[]),
        );
        s.stalled = true;
        s.stalls.push_back(StallSpan {
            start: 1.0,
            end: None,
            frame: 9,
            cause: "x\ty".into(),
        });
        s.gauges.insert("descriptor_heap_used", 12.0);
        s.copies = parse_copies(
            "copies per frame: 2.0 submissions; 10 title copies 1.5 MB; 3 refreshes 0.5 MB",
        );
        let full = snapshot(&s);
        assert!(well_formed(&full), "{full}");
        for want in [
            "\"fps\":12.5",
            "\"gpu_wait_ms\":30.4",
            "\"why\":\"\\\"quoted\\\" reason\",\"per_frame\":3.5",
            "\"count\":1,\"translated\":0,\"by_reason\":[{\"reason\":\"lane # at #\",\"count\":1}]",
            "\"hash\":\"0123456789abcdef\"",
            "\"presses\":1",
            "\"cause\":\"frame waits on a compute pipeline\"",
            "\"cause_ms\":2500",
            "\"removed\":1",
            "\"errors\":1",
            "\"descriptor_heap_used\":12",
            "\"title_copies_mb\":1.5",
        ] {
            assert!(full.contains(want), "{want} not in {full}");
        }
        // The hash is local-only data: it never reaches the WARN tally.
        assert!(!full.contains("hash=0123456789abcdef"));
        assert!(s.events.iter().all(|(_, e)| well_formed(e)));
        let kinds: Vec<_> = s.events.iter().map(|(_, e)| e.clone()).collect();
        assert!(
            kinds
                .iter()
                .any(|e| e.contains("\"kind\":\"untranslated\""))
        );
        assert!(
            kinds
                .iter()
                .any(|e| e.contains("\"kind\":\"device_removed\""))
        );
        // The stall reporter's own lines (#64): the cause after the colon,
        // seconds from the text, and "stall ended" clears it.
        let stall = "stalled frame=9 for 2.5s: waiting on compute pipeline build (3 queued)";
        note(
            &mut s,
            Level::WARN,
            "victory_gpu::stall",
            &fields(stall, &[]),
        );
        assert_eq!(
            s.stall_cause.as_deref(),
            Some("waiting on compute pipeline build (3 queued)")
        );
        assert_eq!(s.stall_cause_ms, Some(2500.0));
        let ended = "stall ended frame=9 after 3.1s: waiting on compute pipeline build";
        note(
            &mut s,
            Level::WARN,
            "victory_gpu::stall",
            &fields(ended, &[]),
        );
        assert!(s.stall_cause.is_none());
    }

    #[test]
    fn non_finite_numbers_are_null() {
        let mut j = Json::object();
        j.num("a", f64::NAN);
        j.num("b", 1.23456);
        assert_eq!(j.end(), "{\"a\":null,\"b\":1.235}");
    }
}
