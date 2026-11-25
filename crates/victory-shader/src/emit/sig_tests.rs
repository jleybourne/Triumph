//! Signatures with several elements in one row, emitted whole and checked
//! by dxil.dll's validator: it rebuilds `ISG1`/`OSG1` from the module and
//! compares them byte for byte, and wants one interpolation mode per row.

use super::container::{self, Psv, SigElement};
use super::sig_interp::psv_interp;
use super::*;
use crate::native::{PsInput, StageIo, Usage};

fn element(name: &str, index: u32, kind: u8, comp: u8, row: u32, col: u8, cols: u8) -> SigElement {
    SigElement {
        name: name.into(),
        index,
        kind,
        comp,
        interp: 0,
        row: Some(row),
        col,
        cols,
        usage: 0,
        half: false,
        packed: false,
    }
}

/// Translates a pixel program whose container has these inputs (and, when
/// `psv_inputs` is given, a `PSV0` declaring them) and a float4 target.
fn pixel(
    words: &[u32],
    inputs: &[SigElement],
    psv_inputs: Option<&[SigElement]>,
    attributes: Vec<PsInput>,
) -> HostShader {
    let native = NativeProgram {
        stage: HwStage::Ps,
        wave64: false,
        vgprs: 4,
        layout: vec![],
        used: vec![],
        io: StageIo::Ps {
            inputs: attributes,
            input_addr: 0,
            db_shader_control: 0,
            patches: vec![],
        },
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let target = element("SV_Target", 0, 16, 9, 0, 0, 4);
    let isg1 = container::signature(inputs, true);
    let osg1 = container::signature(std::slice::from_ref(&target), false);
    let psv0 = psv_inputs.map(|inputs| {
        container::psv(&Psv {
            stage: 0,
            flag: false,
            inputs,
            outputs: std::slice::from_ref(&target),
            prims: &[],
            mesh: None,
            geometry: None,
            bindings: &[],
            threads: [0; 3],
            wave_size: None,
        })
    });
    let mut parts = vec![(FourCC::ISG1, &isg1[..]), (FourCC::OSG1, &osg1[..])];
    if let Some(psv0) = &psv0 {
        parts.push((FourCC::PSV0, psv0));
    }
    let bytes = crate::dxbc::build(&parts);
    let c = Container::parse(&bytes).unwrap();
    let p = ir::build(&native.patched_code(), native.wave64, native.vgprs).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, None).unwrap();
    let io = io::raise(&p, &native).unwrap();
    let st = structure::structure(&p).unwrap();
    emit(&c, &native, &p, &st, &io, &res).unwrap()
}

fn validate(shader: &HostShader) {
    #[cfg(windows)]
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref())
    {
        v.validate(&mut shader.dxil.clone()).unwrap();
    }
    let _ = shader;
}

/// The interpolation modes the translation's `PSV0` declares for its inputs.
fn input_modes(shader: &HostShader) -> Vec<u8> {
    let c = Container::parse(&shader.dxil).unwrap();
    psv_interp(c.part(FourCC::PSV0).unwrap().data, true)
        .unwrap()
        .iter()
        .map(|e| e.3)
        .collect()
}

/// `v_mov_b32 v0, 0; exp mrt0 v0, v0, v0, v0 done; s_endpgm`.
const NO_READS: [u32; 4] = [0x7E00_0280, 0xF800_080F, 0, 0xBF81_0000];
/// `v_interp_mov_f32 v0, p0, attr0.x`, then the same export.
const FLAT_READ: [u32; 4] = [0xC802_0002, 0xF800_080F, 0, 0xBF81_0000];

#[test]
fn elements_of_a_row_keep_their_order_in_the_signature_part() {
    // In row 1 the earlier element sits in the higher column (w), the
    // later one in x, y and z: the part lists them in element order.
    let inputs = [
        element("SV_Position", 0, 3, 9, 0, 0, 4),
        element("TEXCOORD", 3, 0, 9, 1, 3, 1),
        element("NORMAL", 0, 0, 9, 1, 0, 3),
    ];
    let shader = pixel(&NO_READS, &inputs, None, vec![]);
    let c = Container::parse(&shader.dxil).unwrap();
    let rows = signature(c.part(FourCC::ISG1).unwrap().data).unwrap();
    let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["SV_Position", "TEXCOORD", "NORMAL"]);
    validate(&shader);
}

#[test]
fn a_row_with_a_constant_or_flat_read_element_validates() {
    // Row 1: a float next to the front face (constant). Row 2: a float
    // read flat next to an unread one.
    let inputs = [
        element("SV_Position", 0, 3, 9, 0, 0, 4),
        element("TEXCOORD", 0, 0, 9, 1, 0, 1),
        element("SV_IsFrontFace", 0, 13, 5, 1, 1, 1),
        element("TEXCOORD", 1, 0, 9, 2, 0, 2),
        element("TEXCOORD", 2, 0, 9, 2, 2, 2),
    ];
    let attribute = PsInput {
        usage: Usage::Generic,
        row: 2,
        mask: 0x3,
        attr: 0,
        flat: true,
        fp16: false,
        pack2: false,
        row2: 0,
    };
    let shader = pixel(&FLAT_READ, &inputs, None, vec![attribute]);
    assert_eq!(input_modes(&shader), [4, 1, 1, 1, 1]);
    validate(&shader);
}

#[test]
fn unread_inputs_keep_the_modes_the_title_declared() {
    // Row 1 holds two elements the title declared no-perspective; DXC's
    // default (linear) for one of them alone would split the row.
    let mut inputs = vec![
        element("SV_Position", 0, 3, 9, 0, 0, 4),
        element("TEXCOORD", 0, 0, 9, 1, 0, 2),
        element("TEXCOORD", 1, 0, 9, 1, 2, 2),
    ];
    let mut declared = inputs.clone();
    for (e, mode) in declared.iter_mut().zip([4, 4, 4]) {
        e.interp = mode;
    }
    let shader = pixel(&NO_READS, &inputs, Some(&declared), vec![]);
    assert_eq!(input_modes(&shader), [4, 4, 4]);
    validate(&shader);
    // Without the title's `PSV0`: DXC's defaults.
    inputs[2].name = "COLOR".into();
    let shader = pixel(&NO_READS, &inputs, None, vec![]);
    assert_eq!(input_modes(&shader), [4, 2, 2]);
    validate(&shader);
}
