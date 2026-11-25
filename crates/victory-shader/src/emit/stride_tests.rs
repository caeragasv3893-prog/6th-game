//! End-to-end tests of descriptors whose emission needs more than one
//! binding: a buffer read by element through the resource heap (no
//! used-resource entry, so no stride; the view-stride table gives it), and
//! one of two immediate samplers chosen by uniform control flow. Synthetic
//! GFX10 programs lowered to DXIL, validated by `dxil.dll` when present,
//! and run on the host GPU and WARP.

use windows::Win32::Graphics::Direct3D12::D3D12_SAMPLER_DESC;

use super::host::{Dispatch, Host, Root, Stride, View};
use super::{HostShader, StaticSampler, VIEW_REMAP_SENTINEL, VIEW_STRIDE_BASE, VIEW_STRIDES, lower, validator};

fn view_metadata(strides: &[u32], identity: u32, overflow: &[u32]) -> Vec<u32> {
    let mut data = vec![0; VIEW_REMAP_SENTINEL as usize + 1 + overflow.len()];
    data[0] = identity;
    data[1] = overflow.len() as u32;
    let start = VIEW_STRIDE_BASE as usize;
    data[start..start + strides.len()].copy_from_slice(strides);
    data[VIEW_REMAP_SENTINEL as usize + 1..].copy_from_slice(overflow);
    data
}
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

/// User data: s0 the heap index of a structured buffer (a root constant),
/// s1 the resource heap, s2 a table with a raw UAV. Each lane stores dword
/// 1 of element `lane` to dword `lane` of the UAV.
const HEAP_ELEMENT_WAVE32: &[u32] = &[
    0x8F04_8500, // s_lshl_b32 s4, s0, 5
    0xBE86_0301, // s_mov_b32 s6, s1
    0xBE87_0380, // s_mov_b32 s7, 0
    0xF408_0203,
    0x0800_0000, // s_load_dwordx4 s[8:11], s[6:7], s4
    0xBE8C_0302, // s_mov_b32 s12, s2
    0xBE8D_0380, // s_mov_b32 s13, 0
    0xF408_0406,
    0xFA00_0000, // s_load_dwordx4 s[16:19], s[12:13], 0x0
    0x3402_0082, // v_lshlrev_b32 v1, 2, v0
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0xE030_2004,
    0x8002_0200, // buffer_load_dword v2, v0, s[8:11], 0 idxen offset:4
    0xBF8C_3F70, // s_waitcnt vmcnt(0)
    0xE070_1000,
    0x8004_0201, // buffer_store_dword v2, v1, s[16:19], 0 offen
    0xBF81_0000, // s_endpgm
];

/// User data: s0 a table with a 2x1 texture, s1 a root constant, s2 a table
/// with a raw UAV. The root constant picks the immediate sampler by a
/// branch, clamping (non-zero) or wrapping (zero); the program samples the
/// texture at u = 1.25 and stores red to dword 0 of the UAV.
const CHOSEN_SAMPLER_WAVE32: &[u32] = &[
    0xBE84_0300, // s_mov_b32 s4, s0
    0xBE85_0380, // s_mov_b32 s5, 0
    0xF40C_0202,
    0xFA00_0000, // s_load_dwordx8 s[8:15], s[4:5], 0x0
    0xBE86_0302, // s_mov_b32 s6, s2
    0xBE87_0380, // s_mov_b32 s7, 0
    0xF408_0403,
    0xFA00_0000, // s_load_dwordx4 s[16:19], s[6:7], 0x0
    0xBF06_8001, // s_cmp_eq_u32 s1, 0
    0xBF85_0006, // s_cbranch_scc1 -> 0x40
    0xBE94_03FF,
    0x0000_0092, // s_mov_b32 s20, 0x92 (clamp x, y, z)
    0xBE95_0380, // s_mov_b32 s21, 0
    0xBE96_0380, // s_mov_b32 s22, 0
    0xBE97_0380, // s_mov_b32 s23, 0
    0xBF82_0004, // s_branch -> 0x50
    0xBE94_0380, // 0x40: s_mov_b32 s20, 0 (wrap)
    0xBE95_0380, // s_mov_b32 s21, 0
    0xBE96_0380, // s_mov_b32 s22, 0
    0xBE97_0380, // s_mov_b32 s23, 0
    0x7E00_02FF,
    0x3FA0_0000, // 0x50: v_mov_b32 v0, 1.25
    0x7E02_02FF,
    0x3F00_0000, // v_mov_b32 v1, 0.5
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0xF09C_0108,
    0x00A2_0200, // image_sample_lz v2, v[0:1], s[8:15], s[20:23] dmask:0x1 dim:2D
    0xBF8C_3F70, // s_waitcnt vmcnt(0)
    0x7E06_0280, // v_mov_b32 v3, 0
    0xE070_1000,
    0x8004_0203, // buffer_store_dword v2, v3, s[16:19], 0 offen
    0xBF81_0000, // s_endpgm
];

/// User data: s0 the heap index of a constant buffer (a root constant), s1
/// the resource heap, s2 a table with a raw UAV. Each lane stores dword 2
/// of the constant buffer (a scalar buffer load) to dword `lane` of the UAV.
const HEAP_CONSTANT_WAVE32: &[u32] = &[
    0x8F04_8500, // s_lshl_b32 s4, s0, 5
    0xBE86_0301, // s_mov_b32 s6, s1
    0xBE87_0380, // s_mov_b32 s7, 0
    0xF408_0203,
    0x0800_0000, // s_load_dwordx4 s[8:11], s[6:7], s4
    0xBE8C_0302, // s_mov_b32 s12, s2
    0xBE8D_0380, // s_mov_b32 s13, 0
    0xF408_0406,
    0xFA00_0000, // s_load_dwordx4 s[16:19], s[12:13], 0x0
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0xF420_0504,
    0xFA00_0008, // s_buffer_load_dword s20, s[8:11], 0x8
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0x7E02_0214, // v_mov_b32 v1, s20
    0x3404_0082, // v_lshlrev_b32 v2, 2, v0
    0xE070_1000,
    0x8004_0102, // buffer_store_dword v1, v2, s[16:19], 0 offen
    0xBF81_0000, // s_endpgm
];

/// User data: s0 a table with a raw SRV, s2 a table with a raw UAV. Each
/// lane loads the half at byte `2 × lane` and stores it to dword `lane`.
const HALF_LOAD_WAVE32: &[u32] = &[
    0xBE84_0300, // s_mov_b32 s4, s0
    0xBE85_0380, // s_mov_b32 s5, 0
    0xF408_0202,
    0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0x0
    0xBE8C_0302, // s_mov_b32 s12, s2
    0xBE8D_0380, // s_mov_b32 s13, 0
    0xF408_0406,
    0xFA00_0000, // s_load_dwordx4 s[16:19], s[12:13], 0x0
    0x3402_0081, // v_lshlrev_b32 v1, 1, v0
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0xE028_1000,
    0x8002_0201, // buffer_load_ushort v2, v1, s[8:11], 0 offen
    0xBF8C_3F70, // s_waitcnt vmcnt(0)
    0x3406_0082, // v_lshlrev_b32 v3, 2, v0
    0xE070_1000,
    0x8004_0203, // buffer_store_dword v2, v3, s[16:19], 0 offen
    0xBF81_0000, // s_endpgm
];

/// User data: s2 a table with a raw UAV. Each lane stores its index as the
/// half at byte `2 × lane` (two lanes share each dword).
const HALF_STORE_WAVE32: &[u32] = &[
    0xBE8C_0302, // s_mov_b32 s12, s2
    0xBE8D_0380, // s_mov_b32 s13, 0
    0xF408_0406,
    0xFA00_0000, // s_load_dwordx4 s[16:19], s[12:13], 0x0
    0x3402_0081, // v_lshlrev_b32 v1, 1, v0
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0xE068_1000,
    0x8004_0001, // buffer_store_short v0, v1, s[16:19], 0 offen
    0xBF81_0000, // s_endpgm
];

fn param(root: RootType, user_data: u8) -> RootParam {
    RootParam {
        root,
        table_offset_dwords: 0,
        sgprs: 1,
        base_register: 0,
        user_data,
        slot: user_data,
    }
}

/// An entry at offset 0 of the table in user data `user_data`.
fn entry(kind: ResourceType, dwords: u8, user_data: u8) -> UsedResource {
    UsedResource {
        kind,
        root: RootType::Table,
        table_offset_dwords: 0,
        lower_bound: 0,
        stride: 0,
        dwords,
        user_data,
        slot: user_data,
    }
}

fn build(
    words: &[u32],
    layout: Vec<RootParam>,
    used: Vec<UsedResource>,
    threads: u32,
) -> HostShader {
    build_wave(words, layout, used, threads, false)
}

fn build_wave(
    words: &[u32],
    layout: Vec<RootParam>,
    used: Vec<UsedResource>,
    threads: u32,
    wave64: bool,
) -> HostShader {
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64,
        vgprs: 8,
        layout,
        used,
        io: StageIo::Cs(ComputeIo {
            rsrc2: 3 << 1,
            threads: [threads, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("track");
    let io = io::raise(&p, &native).expect("io::raise");
    let lowered = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
        view_marks: false,
    })
    .expect("lower");
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: lowered.dxil,
        user_data: lowered.user_data,
        static_samplers: lowered
            .samplers
            .iter()
            .map(|&w| StaticSampler::from_words(w))
            .collect(),
        samplers: lowered.samplers,
        root_cbvs: lowered.root_cbvs,
        root_srvs: lowered.root_srvs,
        root_uavs: lowered.root_uavs,
        memory: lowered.memory,
        memory_writes: lowered.memory_writes,
        view_addresses: lowered.view_addresses,
        heap: lowered.heap,
        heap_writes: lowered.heap_writes,
        view_marks: lowered.view_marks,
    };
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref()) {
        v.validate(&mut shader.dxil.clone())
            .expect("DXIL validation failed");
    } else {
        eprintln!("no dxil.dll; validation skipped");
    }
    shader
}

#[test]
fn a_heap_buffer_read_by_element_takes_the_views_stride() {
    let shader = build(
        HEAP_ELEMENT_WAVE32,
        vec![
            param(RootType::Constants, 0),
            param(RootType::ResourceHeap, 1),
            param(RootType::Table, 2),
        ],
        vec![entry(ResourceType::Uav, 4, 2)],
        32,
    );
    assert_eq!(shader.root_srvs, [VIEW_STRIDES]);
    // Lane 31 reads beyond the last element.
    let data: Vec<u32> = (0..31).flat_map(|e| [100 + e, 200 + e, 300 + e]).collect();
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            ud[u.index as usize] = match u.root {
                // The title's heap index 3 maps to structured host view 1.
                // The UAV's table already names host slot 0.
                RootType::Constants => 3,
                RootType::Table => 0,
                other => panic!("unexpected user data {other:?}"),
            };
        }
        for prefix in 0..4 {
            let mut raw = vec![0xdeadbeef; prefix];
            raw.extend_from_slice(&data);
            let views = [
                View::RawUav(32),
                View::Buffer { data: &data, stride: Stride::Structured(12) },
                View::Buffer { data: &raw, stride: Stride::Raw },
            ];
            let packed = 12 | prefix as u32 | (3 << 12);
            let dispatch = Dispatch {
                roots: vec![
                    (0, Root::Constants(ud.clone())),
                    (1, Root::View { srv: true, data: view_metadata(&[4, packed, 4], 2, &[0, 1]) }),
                ],
                views: &views,
                samplers: &[],
                groups: [1, 1, 1],
            };
            let words = host.dispatch(&root, &pso, &dispatch)
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            let want: Vec<u32> = (200..231).chain([0]).collect();
            assert_eq!(words[0], want, "{label}, {prefix} prefix dwords");
        }
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// A buffer read at a heap index from data whose slot holds no buffer view
/// (the view-stride table holds 0 there; a texture here) reads the host's
/// null buffer view instead (`emit`, "Buffer views at computed heap
/// indices"): a texture descriptor read as a buffer one is a wild address.
#[test]
fn a_heap_buffer_read_where_no_buffer_view_is_reads_zeros() {
    let shader = build(
        HEAP_ELEMENT_WAVE32,
        vec![
            param(RootType::Constants, 0),
            param(RootType::ResourceHeap, 1),
            param(RootType::Table, 2),
        ],
        vec![entry(ResourceType::Uav, 4, 2)],
        32,
    );
    let texel = [[1.0f32, 2.0, 3.0, 4.0]];
    let views = [View::RawUav(32), View::Texture(1, 1, &texel), View::NullBuffers];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for index in [1, 2, u32::MAX >> 5] {
            for u in &shader.user_data {
                ud[u.index as usize] = if u.root == RootType::Constants { index } else { 0 };
            }
            let dispatch = Dispatch {
                // A texture, an unwritten overflow entry, then an out-of-range index.
                roots: vec![
                    (0, Root::Constants(ud.clone())),
                    (1, Root::View { srv: true, data: view_metadata(&[4, 0], 2, &[0]) }),
                ],
                views: &views,
                samplers: &[],
                groups: [1, 1, 1],
            };
            let words = host
                .dispatch(&root, &pso, &dispatch)
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(words[0], [0; 32], "{label}, title slot {index}");
        }
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// Constant data through the resource heap itself is read from a raw view
/// (the host's constant buffer views need an alignment the title's lack).
#[test]
fn a_constant_buffer_through_the_heap_reads_as_a_raw_buffer() {
    let shader = build(
        HEAP_CONSTANT_WAVE32,
        vec![
            param(RootType::Constants, 0),
            param(RootType::ResourceHeap, 1),
            param(RootType::Table, 2),
        ],
        vec![entry(ResourceType::Uav, 4, 2)],
        32,
    );
    assert!(shader.heap && !shader.heap_writes, "reads the heap itself, writes through a table");
    assert_eq!(shader.root_srvs, [VIEW_STRIDES], "the guard's stride table");
    let data: Vec<u32> = vec![7, 8, 9, 10];
    let views = [
        View::RawUav(32),
        View::Buffer {
            data: &data,
            stride: Stride::Raw,
        },
    ];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            ud[u.index as usize] = match u.root {
                // The constants are at heap index 1, the UAV's table at 0.
                RootType::Constants => 1,
                RootType::Table => 0,
                other => panic!("unexpected user data {other:?}"),
            };
        }
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    // User data, then the stride table: heap index 1 holds a
                    // buffer view.
                    roots: vec![(0, Root::Constants(ud)), (1, Root::View { srv: true, data: view_metadata(&[4, 4], 2, &[]) })],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        assert_eq!(words[0], vec![9; 32], "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// `buffer_load_ushort`: the half at its byte address, zero-extended (the
/// BVH builder reads 16-bit fields this way).
#[test]
fn a_half_buffer_load_reads_the_half_at_its_address() {
    let shader = build(
        HALF_LOAD_WAVE32,
        vec![param(RootType::Table, 0), param(RootType::Table, 2)],
        vec![entry(ResourceType::Srv, 4, 0), entry(ResourceType::Uav, 4, 2)],
        32,
    );
    // 32 halves 0x8000 + 3 × lane, two per dword (low half first).
    let half = |l: u32| 0x8000 + 3 * l;
    let data: Vec<u32> = (0..16).map(|k| half(2 * k) | half(2 * k + 1) << 16).collect();
    let views = [
        View::Buffer {
            data: &data,
            stride: Stride::Raw,
        },
        View::RawUav(32),
    ];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            // The SRV's table at heap index 0, the UAV's at 1.
            ud[u.index as usize] = if u.slot == 0 { 0 } else { 32 };
        }
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        let want: Vec<u32> = (0..32).map(half).collect();
        assert_eq!(words[0], want, "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// The same half loads through a 12-byte structured table SRV.
#[test]
fn a_half_buffer_load_through_structured_srv_reads_the_half_at_its_address() {
    let shader = build(
        HALF_LOAD_WAVE32,
        vec![param(RootType::Table, 0), param(RootType::Table, 2)],
        vec![
            UsedResource { stride: 12, ..entry(ResourceType::Srv, 4, 0) },
            entry(ResourceType::Uav, 4, 2),
        ],
        32,
    );
    // 32 halves 0x8000 + 3 × lane, two per dword (low half first).
    let half = |l: u32| 0x8000 + 3 * l;
    // Six 12-byte elements; the last two dwords are padding.
    let data: Vec<u32> = (0..18).map(|k| half(2 * k) | half(2 * k + 1) << 16).collect();
    let views = [
        View::Buffer {
            data: &data,
            stride: Stride::Structured(12),
        },
        View::RawUav(32),
    ];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            // The SRV's table at heap index 0, the UAV's at 1.
            ud[u.index as usize] = if u.slot == 0 { 0 } else { 32 };
        }
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        let want: Vec<u32> = (0..32).map(half).collect();
        assert_eq!(words[0], want, "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// An indexed sub-dword load selects the element before extracting its field.
#[test]
fn wave64_literal_lane_numbering_runs_without_fixed_wave_width() {
    let words = [
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202, 0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0xD765_0001, 0x0001_00C1, // v_mbcnt_lo_u32_b32 v1, -1, 0
        0xD766_0001, 0x0002_02C1, // v_mbcnt_hi_u32_b32 v1, -1, v1
        0x3404_0082, // v_lshlrev_b32 v2, 2, v0
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0xE070_1000, 0x8002_0102, // buffer_store_dword v1, v2, s[8:11], 0 offen
        0xBF81_0000, // s_endpgm
    ];
    let shader = build_wave(
        &words,
        vec![param(RootType::Table, 0)],
        vec![entry(ResourceType::Uav, 4, 0)],
        128,
        true,
    );
    let container = crate::Container::parse(&shader.dxil).unwrap();
    let psv = container.part(crate::FourCC::PSV0).unwrap().data;
    assert_eq!(u32::from_le_bytes(psv[20..24].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(psv[24..28].try_into().unwrap()), u32::MAX);

    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host.compute_pipeline(&shader).unwrap_or_else(
            |e| panic!("{label}: compute pipeline failed: {e}"),
        );
        let out = host.dispatch(
            &root,
            &pso,
            &Dispatch {
                roots: vec![(0, Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]))],
                views: &[View::RawUav(128)],
                samplers: &[],
                groups: [1, 1, 1],
            },
        ).unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        let want: Vec<u32> = (0..128).map(|i| i % 64).collect();
        assert_eq!(out[0], want, "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

#[test]
fn wave64_bpermute_exchanges_values_across_warps() {
    let words = [
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202, 0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0x3404_0082, // v_lshlrev_b32 v2, 2, v0
        0xDACC_0080, 0x0300_0002, // ds_bpermute_b32 v3, v2, v0 offset:128
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0xDACC_0004, 0x0400_0302, // ds_bpermute_b32 v4, v2, v3 offset:4
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0xE070_1000, 0x8002_0402, // buffer_store_dword v4, v2, s[8:11], 0 offen
        0xBF81_0000, // s_endpgm
    ];
    let make = |threads| build_wave(
        &words,
        vec![param(RootType::Table, 0)],
        vec![entry(ResourceType::Uav, 4, 0)],
        threads,
        true,
    );
    let shader = make(128);
    let wave_bounds = |shader: &HostShader| {
        let container = crate::Container::parse(&shader.dxil).unwrap();
        let psv = container.part(crate::FourCC::PSV0).unwrap().data;
        (
            u32::from_le_bytes(psv[20..24].try_into().unwrap()),
            u32::from_le_bytes(psv[24..28].try_into().unwrap()),
        )
    };
    assert_eq!(wave_bounds(&shader), (0, u32::MAX));
    // Partial logical waves cannot use the full-wave exchange lowering.
    assert_eq!(wave_bounds(&make(96)), (64, 64));
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host.compute_pipeline(&shader).unwrap_or_else(
            |e| panic!("{label}: compute pipeline failed: {e}"),
        );
        let want: Vec<u32> = (0..128).map(|i| (i / 64) * 64 + (i + 33) % 64).collect();
        for _ in 0..3 {
            let out = host.dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]))],
                    views: &[View::RawUav(128)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            ).unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(out[0], want, "{label}");
        }
        eprintln!("{label}: two logical waves, consecutive exchanges passed");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

#[test]
fn wave64_ballot_reconstructs_both_mask_words_and_count() {
    let words = [
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202, 0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0x3404_0082, // v_lshlrev_b32 v2, 2, v0
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        0x7D88_00A8, // v_cmp_gt_u32 vcc, 40, v0
        0x7E06_026A, // v_mov_b32 v3, vcc_lo
        0xE070_1000, 0x8002_0302, // buffer_store_dword v3, v2, s[8:11], 0 offen
        0x7E06_026B, // v_mov_b32 v3, vcc_hi
        0xE070_1200, 0x8002_0302, // store at offset 512
        0xBE90_106A, // s_bcnt1_i32_b64 s16, vcc
        0x7E06_0210, // v_mov_b32 v3, s16
        0xE070_1400, 0x8002_0302, // store at offset 1024
        0x7D88_00FF, 96, // v_cmp_gt_u32 vcc, 96, v0
        0x7E06_026A, // v_mov_b32 v3, vcc_lo
        0xE070_1600, 0x8002_0302, // store at offset 1536
        0x7E06_026B, // v_mov_b32 v3, vcc_hi
        0xE070_1800, 0x8002_0302, // store at offset 2048
        0x7D84_009F, // v_cmp_eq_u32 vcc, 31, v0
        0x7E06_026A, // v_mov_b32 v3, vcc_lo
        0xE070_1A00, 0x8002_0302, // store at offset 2560
        0x7E06_026B, // v_mov_b32 v3, vcc_hi
        0xE070_1C00, 0x8002_0302, // store at offset 3072
        0x7D84_00FF, 127, // v_cmp_eq_u32 vcc, 127, v0
        0x7E06_026B, // v_mov_b32 v3, vcc_hi
        0xE070_1E00, 0x8002_0302, // store at offset 3584
        0xBF81_0000, // s_endpgm
    ];
    let shader = build_wave(
        &words,
        vec![param(RootType::Table, 0)],
        vec![entry(ResourceType::Uav, 4, 0)],
        128,
        true,
    );
    let c = crate::Container::parse(&shader.dxil).unwrap();
    let psv = c.part(crate::FourCC::PSV0).unwrap().data;
    assert_eq!(u32::from_le_bytes(psv[20..24].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(psv[24..28].try_into().unwrap()), u32::MAX);
    let want: Vec<u32> = [
        [u32::MAX, 0],
        [0xff, 0],
        [40, 0],
        [u32::MAX, u32::MAX],
        [u32::MAX, 0],
        [0x8000_0000, 0],
        [0, 0],
        [0, 0x8000_0000],
    ].into_iter().flat_map(|pair| (0..128).map(move |i| pair[i / 64])).collect();
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host.compute_pipeline(&shader).unwrap_or_else(
            |e| panic!("{label}: compute pipeline failed: {e}"),
        );
        for _ in 0..3 {
            let out = host.dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]))],
                    views: &[View::RawUav(1024)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            ).unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(out[0], want, "{label}");
        }
        eprintln!("{label}: both ballot words, population count and scratch reuse passed");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

#[test]
fn wave64_integer_butterfly_reductions_cross_warps() {
    // Each operation explicitly combines the value with its partner at
    // lane XOR 1, 2, 4, 8, 16 and 32. The final step crosses warps.
    // Start with alternating signed-negative and positive values to
    // distinguish signed extrema, unsigned extrema and wrapping addition.
    let ops = [0x25u32, 0x13, 0x14, 0x11, 0x12, 0x1b, 0x1c, 0x1d];
    let vop2 = |op: u32, dst: u32, src0: u32, src1: u32| {
        (op << 25) | (dst << 17) | (src1 << 9) | src0
    };
    let mut words = vec![
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202, 0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0x3404_0082, // v_lshlrev_b32 v2, 2, v0
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    ];
    for (slot, &op) in ops.iter().enumerate() {
        words.extend([
            0x3402_009F, // v_lshlrev_b32 v1, 31, v0
            vop2(0x1c, 1, 256, 1), // v_or_b32 v1, v0, v1
        ]);
        for distance in [1u32, 2, 4, 8, 16, 32] {
            let bytes = distance * 4;
            // v_xor_b32 v3, byte distance, v2.
            if bytes <= 64 {
                words.push(vop2(0x1d, 3, 128 + bytes, 2));
            } else {
                words.extend([vop2(0x1d, 3, 255, 2), bytes]);
            }
            words.extend([
                0xDACC_0000, 0x0400_0103, // ds_bpermute_b32 v4, v3, v1
                0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
                vop2(op, 1, 260, 1), // combine v4 and v1
            ]);
        }
        words.extend([
            0xE070_1000 | (slot as u32 * 512),
            0x8002_0102, // buffer_store_dword v1, v2, s[8:11], 0 offen
        ]);
    }
    words.push(0xBF81_0000);
    let shader = build_wave(
        &words,
        vec![param(RootType::Table, 0)],
        vec![entry(ResourceType::Uav, 4, 0)],
        128,
        true,
    );
    let c = crate::Container::parse(&shader.dxil).unwrap();
    let psv = c.part(crate::FourCC::PSV0).unwrap().data;
    assert_eq!(u32::from_le_bytes(psv[20..24].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(psv[24..28].try_into().unwrap()), u32::MAX);
    let combine = |op, a: u32, b: u32| match op {
        0x25 => a.wrapping_add(b),
        0x13 => a.min(b),
        0x14 => a.max(b),
        0x11 => (a as i32).min(b as i32) as u32,
        0x12 => (a as i32).max(b as i32) as u32,
        0x1b => a & b,
        0x1c => a | b,
        0x1d => a ^ b,
        _ => unreachable!(),
    };
    let mut want = Vec::new();
    for op in ops {
        for base in [0u32, 64] {
            let result = (base..base + 64)
                .map(|i| (i << 31) | i)
                .reduce(|a, b| combine(op, a, b))
                .unwrap();
            want.extend([result; 64]);
        }
    }
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host.compute_pipeline(&shader).unwrap_or_else(
            |e| panic!("{label}: compute pipeline failed: {e}"),
        );
        for _ in 0..3 {
            let out = host.dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]))],
                    views: &[View::RawUav(1024)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            ).unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(out[0], want, "{label}");
        }
        eprintln!("{label}: eight integer reductions across two logical waves passed");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

#[test]
fn wave64_recognized_integer_reductions_use_shared_accumulators() {
    use super::wave::{Op, Shape};
    let vop3 = |dst: u32, op: u32, a: u32, b: u32, c: u32| {
        [0xD400_0000 | (op << 16) | dst, a | (b << 9) | (c << 18)]
    };
    let cases = [
        (Op::AddU, 0x25u32, 0u32),
        (Op::MinU, 0x13, u32::MAX),
        (Op::MaxU, 0x14, 0),
        (Op::MinI, 0x11, i32::MAX as u32),
        (Op::MaxI, 0x12, i32::MIN as u32),
        (Op::And, 0x1b, u32::MAX),
        (Op::Or, 0x1c, 0),
        (Op::Xor, 0x1d, 0),
    ];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (op, opcode, identity) in cases {
        let combine = |dst: u32, a: u32, b: u32| {
            (opcode << 25) | (dst << 17) | (b << 9) | a
        };
        let signed_or_unsigned_extrema = matches!(op, Op::MinU | Op::MaxU | Op::MinI | Op::MaxI);
        let source = if signed_or_unsigned_extrema { 261 } else { 256 };
        let mut words = vec![
            0xBE84_0300, // s_mov_b32 s4, s0
            0xBE85_0380, // s_mov_b32 s5, 0
            0xF408_0202, 0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
            0x3404_0082, // v_lshlrev_b32 v2, 2, v0
            0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        ];
        if signed_or_unsigned_extrema {
            words.push(0x340A_009A); // v_lshlrev_b32 v5, 26, v0
        }
        words.push(0xBE98_25C1); // s_or_saveexec_b64 s[24:25], -1
        let ident_operand = match identity {
            0 => 128,
            u32::MAX => 193,
            _ => 255,
        };
        words.extend(vop3(1, 0x101, ident_operand, source, 24));
        if ident_operand == 255 {
            words.push(identity);
        }
        for shift in [1u32, 2, 4, 8] {
            words.push(combine(1, 0xFA, 1));
            words.push(0xFF00_0001 | ((0x110 + shift) << 8)); // row_shr DPP
        }
        for (reg, lane) in [(16, 15), (17, 31), (18, 47), (19, 63)] {
            words.extend(vop3(reg, 0x360, 257, 128 + lane, 0)); // read each row sum
        }
        words.extend([
            0x7E06_0210, // v_mov_b32 v3, s16
            combine(3, 17, 3),
            combine(3, 18, 3),
            combine(3, 19, 3),
            0xBEFE_0418, // s_mov_b64 exec, s[24:25]
            0xE070_1000, 0x8002_0302, // buffer_store_dword v3, v2, s[8:11], 0 offen
            0xBF81_0000,
        ]);
        // Verify the actual symbolic recognizer, not only the emitted output.
        let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let p = ir::build(&code, true, 8).unwrap();
        let sections = super::wave::sections(&p, 0);
        assert!(sections.values().any(|sec| sec.shapes.values().any(
            |sh| matches!(sh, Shape::Active(found, _, _, ident) if *found == op && *ident == identity)
        )), "{op:?}: the reduction must be recognized");
        let shader = build_wave(
            &words,
            vec![param(RootType::Table, 0)],
            vec![entry(ResourceType::Uav, 4, 0)],
            128,
            true,
        );
        let c = crate::Container::parse(&shader.dxil).unwrap();
        let psv = c.part(crate::FourCC::PSV0).unwrap().data;
        assert_eq!(u32::from_le_bytes(psv[20..24].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(psv[24..28].try_into().unwrap()), u32::MAX);
        let reference = |a: u32, b: u32| match op {
            Op::AddU => a.wrapping_add(b),
            Op::MinU => a.min(b),
            Op::MaxU => a.max(b),
            Op::MinI => (a as i32).min(b as i32) as u32,
            Op::MaxI => (a as i32).max(b as i32) as u32,
            Op::And => a & b,
            Op::Or => a | b,
            Op::Xor => a ^ b,
            _ => unreachable!(),
        };
        let mut want = Vec::new();
        for base in [0u32, 64] {
            let result = (base..base + 64)
                .map(|i| if signed_or_unsigned_extrema { i << 26 } else { i })
                .fold(identity, reference);
            want.extend([result; 64]);
        }
        let mut ran = 0;
        for (label, warp) in [("hardware", false), ("warp", true)] {
            let Ok(host) = Host::new(warp) else {
                eprintln!("no SM 6.6 {label} device; skipped");
                continue;
            };
            let (root, pso) = host.compute_pipeline(&shader).unwrap_or_else(
                |e| panic!("{label}: compute pipeline failed: {e}"),
            );
            let out = host.dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]))],
                    views: &[View::RawUav(128)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            ).unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(out[0], want, "{label}: {op:?}");
            eprintln!("{label}: recognized wave64 {op:?} passed");
            ran += 1;
        }
        assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
    }
}

#[test]
fn a_half_buffer_load_by_index_reads_the_structured_element() {
    let mut code = HALF_LOAD_WAVE32.to_vec();
    // buffer_load_ushort v2, v0, s[8:11], 0 idxen offset:6.
    code[10] = 0xE028_2006;
    code[11] = 0x8002_0200;
    let shader = build(
        &code,
        vec![param(RootType::Table, 0), param(RootType::Table, 2)],
        vec![
            UsedResource { stride: 12, ..entry(ResourceType::Srv, 4, 0) },
            entry(ResourceType::Uav, 4, 2),
        ],
        32,
    );
    // Each 12-byte element has its distinct field in bytes 6 and 7.
    let half = |l: u32| 0x8000 + 3 * l;
    let data: Vec<u32> = (0..32)
        .flat_map(|l| [0xdeadbeef, (half(l) << 16) | 0x1234, 0xabcdef01])
        .collect();
    let views = [
        View::Buffer {
            data: &data,
            stride: Stride::Structured(12),
        },
        View::RawUav(32),
    ];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            // The SRV's table at heap index 0, the UAV's at 1.
            ud[u.index as usize] = if u.slot == 0 { 0 } else { 32 };
        }
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        let want: Vec<u32> = (0..32).map(half).collect();
        assert_eq!(words[0], want, "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

/// `buffer_store_short`: each lane's half lands at its byte address and
/// leaves the other half of the dword alone.
#[test]
fn half_buffer_stores_leave_the_other_half_alone() {
    let shader = build(
        HALF_STORE_WAVE32,
        vec![param(RootType::Table, 2)],
        vec![entry(ResourceType::Uav, 4, 2)],
        32,
    );
    let views = [View::RawUav(16)];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        let want: Vec<u32> = (0..16).map(|k| (2 * k) | ((2 * k + 1) << 16)).collect();
        assert_eq!(words[0], want, "{label}");
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}

#[test]
fn a_sampler_chosen_by_a_uniform_branch_samples_as_chosen() {
    let shader = build(
        CHOSEN_SAMPLER_WAVE32,
        vec![
            param(RootType::Table, 0),
            param(RootType::Constants, 1),
            param(RootType::Table, 2),
        ],
        vec![
            entry(ResourceType::Srv, 8, 0),
            entry(ResourceType::Uav, 4, 2),
        ],
        1,
    );
    assert_eq!(shader.samplers.len(), 2);
    // Red 1 at u < 0.5, 2 from there: u = 1.25 wraps to 0.25, clamps to 1.
    let texels = [[1.0f32, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]];
    let views = [View::Texture(2, 1, &texels), View::RawUav(1)];
    let samplers: [D3D12_SAMPLER_DESC; 0] = [];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        for (clamp, want) in [(1, 2.0f32), (0, 1.0)] {
            let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
            for u in &shader.user_data {
                ud[u.index as usize] = match (u.root, u.slot) {
                    (RootType::Table, 0) => 0,
                    (RootType::Constants, 1) => clamp,
                    // The UAV at heap index 1: 32 bytes in.
                    (RootType::Table, 2) => 32,
                    other => panic!("unexpected user data {other:?}"),
                };
            }
            let words = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(ud))],
                        views: &views,
                        samplers: &samplers,
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            assert_eq!(f32::from_bits(words[0][0]), want, "{label}, clamp {clamp}");
        }
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}