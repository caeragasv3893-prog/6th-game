# cgbj runtime integration candidate (2026-11-26)

This branch takes the entire source tree and history of [cputroll/cgbj-reupload](https://github.com/cputroll/cgbj-reupload) at commit f4b48ad766ddbec1a1867022cd42f7645b0dd55e, under the existing GPL-3.0-or-later license. All original credits and license files are retained.

It is a source-based candidate, NOT a completed merge of the NVIDIA branch. At the common ancestor ffe310eb2098e52169f4c44db5ffdabf43a72622, the imported history was 414 commits ahead and 577 behind our stable NVIDIA branch.

## Preserved references

- fix/nvidia-compatibility: 808f4e29da199e9331d366c679e307e1b62b738c (validated baseline).
- perf/wave64-exchange-fusion: 2f83b99 (unvalidated candidate).
- perf/runtime-memory-merge: 01a1ca1 (unvalidated candidate).
- upstream-snapshot/cgbj-2026-11-26: exact external source baseline.

No branch was reset or force-pushed. Our portable LDS lowering and its specialized tests remain available in the original NVIDIA history; they are not automatically included in this different shader translator. Do not treat this candidate as a superset of the validated branch.

## Integration changes

The zero-dword D3D12 root constants workaround from our validated NVIDIA branch was adapted in the shader test harness: compute and graphics use one zero dword when the constant list is empty, as the signature reserves at least one dword. The external resource binding ABI and shader lowering are otherwise unchanged.

The external source already includes the newer residency guard, texture pools and staging limits, thin command lists, asynchronous submission, descriptor synchronization, and NVIDIA wave32 lowering. Their implementation, existing dependencies and tests are carried together, rather than assembled from incompatible individual files.

The source tree retains the external project's asset policy (including its documented 640 KiB source-file limit); no title data or derived captures were added in this integration. The launcher is retained as source, not a prebuilt binary. No private release was downloaded.

## Windows validation

Run from this branch, with the pinned toolchain and MSVC tools:

```powershell
cargo test -p victory-shader --lib --locked wave64 -- --test-threads=1 --nocapture
cargo test -p victory-gpu --lib --locked residency -- --test-threads=1 --nocapture
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
cargo build --release --locked -p victory
```

No Windows build or hardware run has been performed by this integration session. The external author's performance reports do not establish performance on the RTX 5060 Ti or a 16 GB system. Validate before promotion. If a test fails, keep the stable branch unchanged and resolve the candidate's issue there. A later source-level reconciliation should preserve working runtime behavior while reintroducing NVIDIA-specific corrections and regression tests appropriate to this translator.
