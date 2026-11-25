# Upstream integration checkpoint (2026-11-25)

This branch is an incomplete integration, not a buildable release.

- Common ancestor: 940d32f69b22265690a0b5a15d6ded572ba33baf
- NVIDIA checkpoint: 3908bb52ec21362bfc5f700b90673dfb2e16aef2
- Upstream snapshot: 13f91caee25f2fdf2c062be5f7a27f058e51cbca
- Staged 34 upstream files (30 upstream-only blobs and 4 mechanically merged files). Both side patches for the latter were checked against the full source versions before combining.
- The following seven overlapping files retain the NVIDIA checkpoint version until semantic conflicts are resolved:

- `crates/victory-gpu/src/descriptor/host.rs`
- `crates/victory-gpu/src/host.rs`
- `crates/victory-gpu/src/shadow.rs`
- `crates/victory-shader/src/emit/lower.rs`
- `crates/victory-shader/src/emit/mod.rs`
- `crates/victory-shader/src/emit/shader.rs`
- `crates/victory-shader/src/emit/stride_tests.rs`

## Critical integration contract

The NVIDIA stride word packs a raw SRV companion index and alignment prefix. Upstream adds integer/signed flags to the high bits of the same word; these representations cannot simply be ORed. Separate metadata fields (or an independently proven disjoint encoding) are required. The shader and CPU writers, test harness tables, and cache keys must agree. Upstream view-address records grow from 8 to 16 bytes; allocation, addressing and zero initialization must match. Upstream lazy texture read marks must map back from physical descriptor slots to title slots even for overflow mappings, without publishing uninitialized descriptors. The upstream preparation/eviction lock ordering must be reconciled with stable remap publication and raw companion lifetime. Review joined handles and typed/raw load paths together, and run Clippy, shader regression tests, all workspace tests and release build on Windows before promoting this branch. No game or AMD validation is claimed.

Do not merge this partial checkpoint to the main or NVIDIA development branch.
