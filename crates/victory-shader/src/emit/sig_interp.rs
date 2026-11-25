//! Interpolation modes of signature elements that share a row.
//!
//! DXIL requires every signature element packed into the same row to share
//! an interpolation mode, and the validator rebuilds the signature parts
//! from the module's metadata and compares them byte for byte. The title's
//! compiler declared its modes in the container's `PSV0` part, so those are
//! the starting point ([`psv_interp`]); what the lowering then decides for a
//! read pixel input is spread to its row's unread elements
//! ([`harmonize_row_interp`]).

use super::container::SigElement;

/// Interpolation modes of the input (for pixel) or output (for vertex and
/// mesh) signature elements carried in a container's `PSV0` part:
/// `(row, col, kind, interp)`.
pub(super) fn psv_interp(part: &[u8], pixel: bool) -> Option<Vec<(Option<u32>, u8, u8, u8)>> {
    use crate::dxbc::u32_at;
    let info_sz = u32_at(part, 0).ok()? as usize;
    if info_sz < 32 {
        return None;
    }
    let info = part.get(4..4 + info_sz)?;
    let (n_in, n_out, n_prim) = (info[28] as usize, info[29] as usize, info[30] as usize);
    let mut pos = 4 + info_sz;
    let n_bind = u32_at(part, pos).ok()? as usize;
    pos += 4;
    if n_bind > 0 {
        let bsz = u32_at(part, pos).ok()? as usize;
        pos = pos.checked_add(4)?.checked_add(n_bind.checked_mul(bsz)?)?;
    }
    let str_sz = u32_at(part, pos).ok()? as usize;
    pos = pos.checked_add(4)?.checked_add(str_sz)?;
    let idx_cnt = u32_at(part, pos).ok()? as usize;
    pos = pos.checked_add(4)?.checked_add(idx_cnt.checked_mul(4)?)?;
    if n_in + n_out + n_prim == 0 {
        return Some(Vec::new());
    }
    let esz = u32_at(part, pos).ok()? as usize;
    pos += 4;
    if esz < 14 {
        return None;
    }
    let (start_i, count) = if pixel { (0, n_in) } else { (n_in, n_out) };
    let mut out = Vec::with_capacity(count);
    for i in start_i..start_i + count {
        let off = pos.checked_add(i.checked_mul(esz)?)?;
        let e = part.get(off..off.checked_add(esz)?)?;
        let alloc = e[10] & 0x40 != 0;
        let row = alloc.then_some(u32::from(e[9]));
        let col = if alloc { (e[10] >> 4) & 3 } else { 0 };
        out.push((row, col, e[11], e[13]));
    }
    Some(out)
}

/// Gives the elements the modes the title's `PSV0` declares for them
/// (`modes`, from [`psv_interp`]).
pub(super) fn apply_psv_interp(
    elements: &mut [SigElement],
    modes: &[(Option<u32>, u8, u8, u8)],
    conservative_depth: bool,
) {
    for e in elements {
        let Some(&(_, _, _, m)) = modes
            .iter()
            .find(|&&(row, col, kind, m)| row == e.row && col == e.col && kind == e.kind && m != 0)
        else {
            continue;
        };
        // With a conservative depth output the position is centroid.
        e.interp = if e.kind == 3 && conservative_depth && m == 4 {
            5
        } else {
            m
        };
    }
}

/// Makes every element of a row share one interpolation mode. Integer
/// attributes and `SV_IsFrontFace` can only be constant (1), so a float
/// packed into their row is constant as well; otherwise the unread elements
/// of a row take the mode of a read one (what the lowering chose for it).
pub(super) fn harmonize_row_interp(elements: &mut [SigElement]) {
    let mut rows: Vec<u32> = elements.iter().filter_map(|e| e.row).collect();
    rows.sort_unstable();
    rows.dedup();
    for row in rows {
        let forced_constant = elements
            .iter()
            .any(|e| e.row == Some(row) && (e.kind == 13 || (e.kind == 0 && e.comp != 9)));
        let mode = if forced_constant {
            Some(1)
        } else {
            elements
                .iter()
                .find(|e| e.row == Some(row) && e.usage != 0 && e.interp != 0)
                .map(|e| e.interp)
        };
        if let Some(m) = mode {
            for e in elements.iter_mut() {
                if e.row == Some(row) && e.interp != 0 && e.usage == 0 {
                    e.interp = m;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(name: &str, comp: u8, row: u32, col: u8, cols: u8, interp: u8) -> SigElement {
        SigElement {
            name: name.into(),
            index: 0,
            kind: 0,
            comp,
            interp,
            row: Some(row),
            col,
            cols,
            usage: 0,
            half: false,
            packed: false,
        }
    }

    #[test]
    fn a_row_shares_the_mode_of_its_integer_or_read_element() {
        let mut elements = vec![
            // An integer and a float in row 1: both constant.
            element("COLOR", 5, 1, 0, 1, 1),
            element("COLOR", 9, 1, 1, 1, 2),
            // A read flat float and an unread float in row 2: both flat.
            SigElement {
                usage: 1,
                ..element("TEXCOORD", 9, 2, 0, 2, 1)
            },
            element("TEXCOORD", 9, 2, 2, 2, 2),
            // Nothing read in row 3: as declared.
            element("NORMAL", 9, 3, 0, 3, 2),
        ];
        harmonize_row_interp(&mut elements);
        let modes: Vec<u8> = elements.iter().map(|e| e.interp).collect();
        assert_eq!(modes, [1, 1, 1, 1, 2]);
    }

    /// A `PSV0` part with runtime info of `info` bytes and these elements
    /// (`(row, col, kind, interp)`), `inputs` of them inputs.
    fn psv(info: usize, inputs: u8, elements: &[(u8, u8, u8, u8)]) -> Vec<u8> {
        let mut part = (info as u32).to_le_bytes().to_vec();
        let mut runtime = vec![0u8; info];
        runtime[28] = inputs;
        runtime[29] = elements.len() as u8 - inputs;
        part.extend(runtime);
        // No bindings, an empty string table, no semantic indices.
        part.extend(0u32.to_le_bytes());
        part.extend(4u32.to_le_bytes());
        part.extend([0u8; 4]);
        part.extend(0u32.to_le_bytes());
        part.extend(16u32.to_le_bytes());
        for &(row, col, kind, interp) in elements {
            let mut e = [0u8; 16];
            e[9] = row;
            e[10] = 0x40 | col << 4;
            e[11] = kind;
            e[13] = interp;
            part.extend(e);
        }
        part
    }

    #[test]
    fn the_modes_of_the_title_are_read_per_stage_and_applied() {
        let part = psv(48, 1, &[(0, 0, 0, 2), (1, 0, 0, 3), (1, 2, 0, 6)]);
        let pixel = psv_interp(&part, true).unwrap();
        assert_eq!(pixel, [(Some(0), 0, 0, 2)]);
        let vertex = psv_interp(&part, false).unwrap();
        assert_eq!(vertex, [(Some(1), 0, 0, 3), (Some(1), 2, 0, 6)]);
        let mut outputs = vec![
            element("TEXCOORD", 9, 1, 0, 2, 2),
            element("TEXCOORD", 9, 1, 2, 2, 2),
            element("COLOR", 9, 2, 0, 4, 2),
        ];
        apply_psv_interp(&mut outputs, &vertex, false);
        let modes: Vec<u8> = outputs.iter().map(|e| e.interp).collect();
        assert_eq!(modes, [3, 6, 2]);
        // A truncated part reads as no modes.
        assert!(psv_interp(&part[..part.len() - 20], false).is_none());
        assert!(psv_interp(&part[..8], true).is_none());
    }

    #[test]
    fn a_conservative_depth_keeps_the_position_centroid() {
        let mut inputs = vec![SigElement {
            kind: 3,
            ..element("SV_Position", 9, 0, 0, 4, 5)
        }];
        apply_psv_interp(&mut inputs, &[(Some(0), 0, 3, 4)], true);
        assert_eq!(inputs[0].interp, 5);
        apply_psv_interp(&mut inputs, &[(Some(0), 0, 3, 4)], false);
        assert_eq!(inputs[0].interp, 4);
    }
}
