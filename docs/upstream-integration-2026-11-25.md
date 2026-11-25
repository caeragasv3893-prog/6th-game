# Upstream integration (2026-11-25)

Candidate only: compilation and GPU execution have not been verified.

- Common ancestor: 940d32f69b22265690a0b5a15d6ded572ba33baf
- NVIDIA checkpoint: 3908bb52ec21362bfc5f700b90673dfb2e16aef2
- Upstream snapshot: 13f91caee25f2fdf2c062be5f7a27f058e51cbca
- 83 upstream commits; 152 NVIDIA-side commits since the common ancestor.
- All 41 upstream-changed files are represented in the integration, including 11 overlapping files reconciled with the NVIDIA implementation. The two histories and the previous NVIDIA branch are preserved.

## Integration decisions

- Preserve portable wave64 lowering, descriptor remapping, raw SRV companions, full resource-property cache keys, and the existing NVIDIA regression suite.
- Adopt upstream GPU texture preparation, eviction, read marks, descriptor joins, synchronization, and memory improvements.
- Retain configurable snapshot limits with upstream's 512 MiB default.
- Use separate packed-stride/companion and typed-format planes, with overflow remapping after both. Update the host writers, shader lowering, and test metadata builders together.
- Allocate and initialize 16-byte view-address records per physical slot.
- Resolve physical read marks back to title descriptor indices, including overflow slots, and preserve lazy texture creation. Publish overflow mappings after the descriptor and metadata.
- Add checks for independent metadata planes and reverse overflow mapping.

## Validation gate

Source patches were checked against the corresponding revisions before reconciliation. No Windows compilation, Clippy, or GPU tests were executed by this integration environment. Run the regression suite, all workspace tests, and a release build on Windows before promoting this candidate. Do not claim game compatibility, performance, AMD validation, or the absence of regressions from this merge alone. The prior NVIDIA branch remains available unchanged.
