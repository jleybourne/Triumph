//! A capture of the geometry a frame drew, for rendering the world outside
//! this runtime.
//!
//! The translated shaders cost too much to run the title at a modern frame
//! rate on a PC card, but the geometry, the textures and the draw calls all
//! pass through this crate in decoded form. `VICTORY_WORLD_CAPTURE=<dir>`
//! writes them out at the frame named by `VICTORY_WORLD_CAPTURE_FRAME`
//! (default 600, a frame inside the world): a `capture.json` describing each
//! draw and its vertex layout, and `buffers.bin` holding the vertex and index
//! bytes. `tools/capture-to-gltf.py` turns that into a glTF any engine opens,
//! where the same geometry renders at hundreds of frames a second because
//! nothing is translating anything.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// One draw, with what is needed to rebuild its geometry.
#[derive(Clone)]
pub(crate) struct Drawn {
    pub(crate) pipeline: usize,
    /// Input elements: (slot, offset, format, per_instance).
    pub(crate) inputs: Vec<(u32, u32, u32, bool)>,
    /// Per slot: (address, stride, size); zero address: unused.
    pub(crate) views: Vec<(u64, u32, u32)>,
    /// (address, format, size) of the index buffer, if one was drawn.
    pub(crate) index: Option<(u64, u32, u32)>,
    pub(crate) count: u32,
    pub(crate) first: u32,
    pub(crate) base: i32,
    pub(crate) topology: u32,
    /// Vertex bytes per slot, filled when the capture is written.
    pub(crate) vertices: Vec<Option<Vec<u8>>>,
    pub(crate) indices: Option<Vec<u8>>,
}

static ARMED: AtomicBool = AtomicBool::new(false);
static ONCE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static FRAME: AtomicUsize = AtomicUsize::new(0);
static DRAWS: Mutex<Vec<Drawn>> = Mutex::new(Vec::new());

/// The directory a capture goes in, if `VICTORY_WORLD_CAPTURE` names one.
pub(crate) fn dir() -> Option<&'static std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| std::env::var_os("VICTORY_WORLD_CAPTURE").map(std::path::PathBuf::from))
        .as_ref()
}

/// The frame to capture.
fn capture_frame() -> usize {
    *ONCE.get_or_init(|| {
        std::env::var("VICTORY_WORLD_CAPTURE_FRAME")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(600)
    })
}

/// Whether this frame's draws are being recorded. Called once per frame.
pub(crate) fn note_frame(frame: usize) {
    FRAME.store(frame, Ordering::Relaxed);
    if dir().is_none() {
        return;
    }
    let at = capture_frame();
    if frame == at {
        ARMED.store(true, Ordering::Relaxed);
        tracing::info!(frame, "world capture: recording this frame's draws");
    }
}

pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// Records one draw, reading its vertex and index bytes out of the title's
/// copies now: title memory may be rewritten after the draw.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drawn(
    pipeline: usize,
    inputs: &[(u32, u32, u32, bool)],
    views: &[(u64, u32, u32)],
    index: Option<(u64, u32, u32)>,
    count: u32,
    first: u32,
    base: i32,
    topology: u32,
) {
    // A new frame writes out what the last one captured, then arms this one
    // if it is the frame asked for.
    let frame = crate::frame::current() as usize;
    static LAST: AtomicUsize = AtomicUsize::new(usize::MAX);
    if LAST.swap(frame, Ordering::Relaxed) != frame {
        if armed() {
            disarm();
            if let Err(e) = write() {
                tracing::warn!(error = %e, "world capture could not be written");
            }
        }
        note_frame(frame);
    }
    if !armed() {
        return;
    }
    let vertices: Vec<Option<Vec<u8>>> = views
        .iter()
        .map(|&(address, stride, size)| {
            if address == 0 || stride == 0 {
                return None;
            }
            // Whole slot for indexed draws: the base vertex can move the top
            // of the range, and a viewer wants the buffer regardless.
            let len = size as usize;
            let shadow = crate::shadow::find_any(address, len as u64)?;
            shadow.copy_out(address, len)
        })
        .collect();
    let indices = index.and_then(|(address, _, size)| {
        let len = size as usize;
        let shadow = crate::shadow::find_any(address, len as u64)?;
        shadow.copy_out(address, len)
    });
    if let Ok(mut draws) = DRAWS.try_lock() {
        draws.push(Drawn {
            pipeline,
            inputs: inputs.to_vec(),
            views: views.to_vec(),
            index,
            count,
            first,
            base,
            topology,
            vertices,
            indices,
        });
    }
}

/// Nothing more is recorded until the next arming.
pub(crate) fn disarm() {
    ARMED.store(false, Ordering::Relaxed);
}

/// Writes `capture.json` and `buffers.bin` for what was recorded.
pub(crate) fn write() -> std::io::Result<()> {
    let Some(dir) = dir() else {
        return Ok(());
    };
    let draws = DRAWS.lock().unwrap_or_else(|e| e.into_inner());
    if draws.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let mut blob: Vec<u8> = Vec::new();
    let mut json = String::from("{\n  \"draws\": [\n");
    for (n, d) in draws.iter().enumerate() {
        if n > 0 {
            json.push_str(",\n");
        }
        let mut slots = String::new();
        for (i, v) in d.vertices.iter().enumerate() {
            let (off, len) = match v {
                Some(bytes) => {
                    let at = blob.len();
                    blob.extend_from_slice(bytes);
                    (at, bytes.len())
                }
                None => (0, 0),
            };
            if i > 0 {
                slots.push(',');
            }
            slots.push_str(&format!("{{\"offset\":{off},\"length\":{len}}}"));
        }
        let idx = match &d.indices {
            Some(bytes) => {
                let at = blob.len();
                blob.extend_from_slice(bytes);
                format!("{{\"offset\":{at},\"length\":{},\"format\":{}}}", bytes.len(), d.index.map_or(0, |i| i.1))
            }
            None => "null".to_string(),
        };
        let inputs: Vec<String> = d
            .inputs
            .iter()
            .map(|&(slot, offset, format, per_instance)| {
                format!(
                    "{{\"slot\":{slot},\"offset\":{offset},\"format\":{format},\"per_instance\":{per_instance}}}"
                )
            })
            .collect();
        let views: Vec<String> = d
            .views
            .iter()
            .map(|&(address, stride, size)| {
                format!("{{\"address\":{address},\"stride\":{stride},\"size\":{size}}}")
            })
            .collect();
        json.push_str(&format!(
            "    {{\"pipeline\":{},\"count\":{},\"first\":{},\"base\":{},\"topology\":{},\n     \"inputs\":[{}],\n     \"views\":[{}],\n     \"slots\":[{}],\n     \"indices\":{}}}",
            d.pipeline,
            d.count,
            d.first,
            d.base,
            d.topology,
            inputs.join(","),
            views.join(","),
            slots,
            idx
        ));
    }
    json.push_str("\n  ]\n}\n");
    std::fs::write(dir.join("capture.json"), json)?;
    std::fs::write(dir.join("buffers.bin"), &blob)?;
    tracing::info!(draws = draws.len(), bytes = blob.len(), "world capture written");
    Ok(())
}
