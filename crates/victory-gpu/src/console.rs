//! Which console the title runs on. Titles pick settings (resolution,
//! memory budgets) by it, so the user chooses; Series X unless told. `Pc`
//! reports this machine's own card instead of a console: a title that sizes
//! its allocations from the figure is told what is really there, not what an
//! Xbox of another shape has.

use std::sync::atomic::{AtomicUsize, Ordering};

const GIB: usize = 1 << 30;

/// What Windows and the runtime keep out of the card's memory before the
/// title's share is counted, for the `Pc` profile.
const PC_RESERVE: usize = 512 << 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Console {
    #[default]
    SeriesX,
    SeriesS,
    /// The host card: one range, its own capacity less [`PC_RESERVE`].
    Pc,
}

impl Console {
    /// Title memory in bytes: (GPU-optimal, standard). Series X in the
    /// advanced configuration; Series S has one range, counted as standard
    /// (7,776 MiB, the published console-mode figure); `Pc` has one range,
    /// read from the card at launch.
    // shortcut: later system updates gave Series S titles more (amount not
    // published here); the title sizes nothing from this figure
    // (it goes by the device type), so update it if a title does.
    pub fn title_memory(self) -> (usize, usize) {
        match self {
            Console::SeriesX => (10 * GIB, 7 * GIB / 2),
            Console::SeriesS => (0, 7_776 << 20),
            // The same shape Series X has -- a card-sized pool and a
            // system-memory pool -- with this machine's own numbers. Reporting
            // one pool instead made the title take the whole figure as its
            // system share and allocate more than it had.
            Console::Pc => (
                card_memory().saturating_sub(PC_RESERVE),
                4 * GIB,
            ),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Console::SeriesX => "Xbox Series X",
            Console::SeriesS => "Xbox Series S",
            Console::Pc => "this PC's card",
        }
    }
}

/// This machine's largest discrete card, in bytes, as the profile reports
/// it. Cached: the title asks for it repeatedly.
pub fn card_memory() -> usize {
    static ONCE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        #[cfg(windows)]
        {
            use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
            // SAFETY: plain DXGI enumeration; every failure reports zero, and
            // zero means "not known", which reads as no room to give away.
            unsafe {
                let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                    return 0;
                };
                let mut best = 0usize;
                let mut i = 0;
                while let Ok(adapter) = factory.EnumAdapters1(i) {
                    if let Ok(desc) = adapter.GetDesc1() {
                        // A software adapter is not the card the title runs on.
                        const SOFTWARE: u32 = 2;
                        if desc.Flags & SOFTWARE == 0 {
                            best = best.max(desc.DedicatedVideoMemory);
                        }
                    }
                    i += 1;
                }
                best
            }
        }
        #[cfg(not(windows))]
        {
            0
        }
    })
}

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

/// Set before the title starts.
pub fn set(console: Console) {
    CONSOLE.store(
        match console {
            Console::SeriesX => 0,
            Console::SeriesS => 1,
            Console::Pc => 2,
        },
        Ordering::Relaxed,
    );
}

pub fn current() -> Console {
    match CONSOLE.load(Ordering::Relaxed) {
        1 => Console::SeriesS,
        2 => Console::Pc,
        _ => Console::SeriesX,
    }
}
