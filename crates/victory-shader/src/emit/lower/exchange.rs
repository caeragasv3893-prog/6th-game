//! Compose adjacent, unmasked logical wave64 rotations before emitting LDS.
//! A partial wave cannot use this identity: an absent intermediate lane
//! reads zero even if the final composed source would be present.

use std::collections::{HashMap, HashSet};

use super::{Parts, Value};
use crate::gfx10::{Fields, Operand};
use crate::io::Input;
use crate::ir::{Exit, Inst, Reg};
use crate::native::StageIo;
use crate::structure::Bool;

#[derive(Clone, Copy, Debug)]
pub(super) struct Fusion {
    pub end: usize,
    pub source: Value,
    pub result: Value,
    pub shift: u32,
}

fn vector(op: &Operand) -> Option<Reg> {
    match *op {
        Operand::Vector { reg, dwords: 1 } => Some(Reg::V(reg)),
        _ => None,
    }
}

/// (address SSA value, source SSA value, result SSA value, lane rotation).
fn rotation(inst: &Inst, addresses: &HashSet<Value>) -> Option<(Value, Value, Value, u32)> {
    if inst.name() != "ds_bpermute_b32" {
        return None;
    }
    let Fields::Ds { dst: Some(dst), addr: Some(addr), data, offset0, offset1, gds: false } =
        &inst.fields
    else {
        return None;
    };
    let [source] = data.as_slice() else { return None };
    let address = inst.read(vector(addr)?)?;
    if !addresses.contains(&address) || inst.writes.len() != 1 {
        return None;
    }
    let source = inst.read(vector(source)?)?;
    let result = inst.write(vector(dst)?)?;
    let offset = u32::from(*offset0) | u32::from(*offset1) << 8;
    // Address is 4 * flat thread id. The low two byte bits never affect
    // the source lane, including when the immediate is not dword aligned.
    Some((address, source, result, (offset >> 2) & 63))
}

/// Only full-EXEC, one-dimensional, complete logical waves in a single
/// terminal block. SSA use counts prevent dropping observable intermediate
/// values, and an address must be proven to be the thread id shifted by 2.
pub(super) fn plan(x: &Parts<'_>) -> HashMap<usize, Fusion> {
    let mut fusions = HashMap::new();
    let StageIo::Cs(cs) = x.native.io else { return fusions };
    let [threads, y, z] = cs.threads;
    if !x.native.wave64 || threads == 0 || !threads.is_multiple_of(64)
        || y != 1 || z != 1 || x.p.blocks.len() != 1
        || !matches!(x.p.blocks[0].exit, Exit::End)
        || !x.p.blocks[0].phis.is_empty()
        || x.st.guards[0].iter().any(|g| g.is_some_and(|g| g != Bool::TRUE))
    {
        return fusions;
    }
    let insts = &x.p.blocks[0].insts;
    let ids: HashSet<Value> = x.io.inputs.iter().filter_map(|&(v, input)| {
        matches!(input, Input::ThreadIdInGroup(0)).then_some(v)
    }).collect();
    let mut addresses = HashSet::new();
    let mut uses = HashMap::<Value, usize>::new();
    for inst in insts {
        for &(_, value) in &inst.reads {
            *uses.entry(value).or_default() += 1;
        }
        if inst.name() != "v_lshlrev_b32" || inst.writes.len() != 1 {
            continue;
        }
        let Fields::Valu(v) = &inst.fields else { continue };
        if v.dpp.is_some() || v.sdwa.is_some() || v.abs != 0 || v.neg != 0
            || v.clamp || v.omod != 0 || v.opsel != 0
        {
            continue;
        }
        let [Operand::Const(130) | Operand::Literal(2), src] = v.src.as_slice() else {
            continue;
        };
        if vector(src).and_then(|r| inst.read(r)).is_some_and(|v| ids.contains(&v)) {
            addresses.insert(inst.writes[0].1);
        }
    }
    let mut i = 0;
    while i < insts.len() {
        let Some((addr, source, mut result, mut shift)) = rotation(&insts[i], &addresses) else {
            i += 1;
            continue;
        };
        let mut end = i;
        while let Some(next) = insts.get(end + 1) {
            let Some((next_addr, next_src, next_result, next_shift)) = rotation(next, &addresses) else {
                break;
            };
            if next_addr != addr || next_src != result || uses.get(&result) != Some(&1) {
                break;
            }
            shift = (shift + next_shift) & 63;
            result = next_result;
            end += 1;
        }
        if end > i || shift == 0 {
            fusions.insert(i, Fusion { end, source, result, shift });
        }
        i = end + 1;
    }
    fusions
}
