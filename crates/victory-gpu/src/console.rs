//! Which console the title runs on. Titles pick settings (resolution,
//! memory budgets) by it, so the user chooses; Series X unless told.

use std::sync::atomic::{AtomicBool, Ordering};

const GIB: usize = 1 << 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Console {
    #[default]
    SeriesX,
    SeriesS,
}

impl Console {
    /// Title memory in bytes: (GPU-optimal, standard). Series X in the
    /// advanced configuration; Series S has one range, counted as standard
    /// (7,776 MiB, the published console-mode figure).
    // shortcut: later system updates gave Series S titles more (amount not
    // published here); New Game 6 sizes nothing from this figure
    // (it goes by the device type), so update it if a title does.
    /// The title's memory budget: the graphics pool the title streams into, and
    /// standard memory.
    ///
    /// A host card smaller than the console's pool must be reported as such:
    /// the title streams textures up to the pool it is told about, and a 10 GiB
    /// pool on a 12 GB card ends in `device removed` once the card is full.
    pub fn title_memory(self) -> (usize, usize) {
        let (gpu, standard) = match self {
            Console::SeriesX => (10 * GIB, 7 * GIB / 2),
            Console::SeriesS => (0, 7_776 << 20),
        };
        let limit = graphics_limit();
        (gpu.min(limit), standard)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Console::SeriesX => "Xbox Series X",
            Console::SeriesS => "Xbox Series S",
        }
    }
}

/// What the host card can hold for the title's graphics pool: its dedicated
/// video memory less a margin the driver, the swap chain and the host's own
/// buffers need. `VICTORY_GPU_MIB` overrides it; 0 means no cap (a card we
/// could not ask).
fn graphics_limit() -> usize {
    use std::sync::OnceLock;
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        if let Some(mib) = std::env::var("VICTORY_GPU_MIB")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            return mib << 20;
        }
        let dedicated = host_vram();
        if dedicated == 0 {
            return usize::MAX;
        }
        tracing::info!(
            vram_mib = dedicated >> 20,
            limit_mib = (dedicated.saturating_sub(2 * GIB)) >> 20,
            "title graphics budget from the host card"
        );
        // Leave 2 GiB: the driver's own allocations, the swap chain, and the
        // host copies the title's non-graphics memory needs.
        dedicated.saturating_sub(2 * GIB)
    })
}

/// The host card's dedicated video memory in MiB (0 if it could not be asked):
/// what the host's own texture budget is scaled from.
pub fn host_video_megabytes() -> u64 {
    (host_vram() >> 20) as u64
}

/// The dedicated video memory of the host's first adapter (0 if not asked).
fn host_vram() -> usize {
    use windows::Win32::Graphics::Dxgi::*;
    // SAFETY: plain DXGI queries.
    unsafe {
        let Ok(factory) = CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) else {
            return 0;
        };
        let Ok(adapter) = factory.EnumAdapters1(0) else {
            return 0;
        };
        let Ok(desc) = adapter.GetDesc1() else {
            return 0;
        };
        desc.DedicatedVideoMemory
    }
}

static SERIES_S: AtomicBool = AtomicBool::new(false);

/// Set before the title starts.
pub fn set(console: Console) {
    SERIES_S.store(console == Console::SeriesS, Ordering::Relaxed);
}

pub fn current() -> Console {
    if SERIES_S.load(Ordering::Relaxed) {
        Console::SeriesS
    } else {
        Console::SeriesX
    }
}
