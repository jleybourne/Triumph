//! The size the title's render targets are made at, as a fraction of what
//! it asks for (`VICTORY_RENDER_SCALE`).
//!
//! The title renders at 3840x2160 because that is what the console it was
//! built for does, and every translated shader then runs four times the
//! pixels a 1080p picture needs. On a host card that is the single largest
//! cost in the frame.
//!
//! The factor is applied in three places that must agree, or the picture
//! comes out squashed, cropped, or blank:
//!
//! - the resource description, so a target is created smaller;
//! - the viewport, so geometry fills what was created (`SetViewports`);
//! - the scissor rectangle, so nothing is clipped away (`SetScissorRects`).
//!
//! `VICTORY_RENDER_SCALE=1` (or unset) leaves everything exactly as it was.

use std::sync::OnceLock;

/// The factor, clamped to something a picture survives.
pub fn render_scale() -> f32 {
    static SCALE: OnceLock<f32> = OnceLock::new();
    *SCALE.get_or_init(|| {
        let asked = std::env::var("VICTORY_RENDER_SCALE")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(1.0);
        let scale = asked.clamp(0.25, 1.0);
        if scale != 1.0 {
            tracing::info!(scale, "render targets are made smaller than the title asks");
        }
        scale
    })
}

/// Whether anything is being scaled: the fast path for every caller.
pub fn scaled() -> bool {
    render_scale() != 1.0
}

/// A length in a target, scaled. Never zero: a target of no pixels is not a
/// target.
pub fn len(v: u32) -> u32 {
    if !scaled() {
        return v;
    }
    let s = (v as f64 * f64::from(render_scale())).round() as u32;
    s.max(1)
}

/// A viewport or scissor coordinate, scaled.
pub fn pos(v: f32) -> f32 {
    if !scaled() {
        return v;
    }
    v * render_scale()
}
