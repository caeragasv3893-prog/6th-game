# Runtime optimization integration plan

Source investigated: [cputroll/cgbj-reupload](https://github.com/cputroll/cgbj-reupload), especially [findings](https://github.com/cputroll/cgbj-reupload/blob/main/docs/findings.md), [replay](https://github.com/cputroll/cgbj-reupload/blob/main/docs/replay.md) and its historical [NVIDIA fork notes](https://github.com/cputroll/cgbj-reupload/blob/main/docs/README-fxceless.md).

Baseline: NVIDIA branch 808f4e2. Exchange Fusion remains a separate unvalidated candidate.

The external fork's measurements are from different hardware and diagnostic builds. They are hypotheses to test here, not our measured results. Its host, draw, list, queue, watch, shadow, descriptor/host, resource, texture and indirect modules differ substantially from ours. Its comparison endpoint does not expose a common merge base. Do not wholesale replace these modules or import undocumented binaries or captures.

| Priority | Candidate | Constraints |
|---|---|---|
| P0 | Byte-exact AVX2 merge | Preserve existing byte conflict policy, synchronization, mappings and scalar fallback. Implemented here; Windows validation pending. |
| P1 | VRAM/residency budgeting | Audit existing eviction, placed-resource lifetimes and DXGI budgets; do not assume 24 GB VRAM or 64 GB RAM. |
| P2 | Texture upload scheduling | Fence completion and resource lifetime before publishing descriptors; preserve current NVIDIA heap layout. |
| P3 | Descriptor update work | Compare existing byte-record mirroring, sparse remaps, null descriptors and raw companion views; avoid reimplementing upstream fixes. |
| P4 | Asynchronous submissions and thin lists | Port in dependency order with explicit queue, fence and allocator ownership tests; retain synchronous fallback. |
| P5 | Synthetic frame profiling | Use synthetic data or authorized local captures outside the repository; never commit title assets. |

The initial AVX2 change is an independent adaptation of the CPU comparison optimization idea, not a direct import of the fork's dword landing implementation. Its merge rules differ from ours. No changes to descriptor layouts, queue ordering, shader lowering, asset policy or game data are included.

## Validation

Run on Windows:

```powershell
cargo test -p victory-gpu --lib --locked runtime_merge -- --test-threads=1 --nocapture
cargo test -p victory-gpu --lib --locked shadow::tests -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --release -p victory-gpu --lib --locked benchmark_runtime_memory_merge -- --ignored --test-threads=1 --nocapture
cargo test --workspace --locked -- --test-threads=1
```

The benchmark reports scalar and selected implementation times for identical synthetic buffers. It excludes input reset time, and does not measure gameplay.
