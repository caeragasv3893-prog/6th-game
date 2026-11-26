# Primary source baseline (2026-11-26)

The primary branch adopts [fxceless/victory-fork](https://github.com/fxceless/victory-fork) at f6a09d3ba4ca425e71429ddbcb43b18ad507202a, as requested by this fork's owner. This is an explicit source-tree adoption, not a conflict-resolved combination of all previous implementations. The original license, authorship and upstream history are retained.

The former main (940d32f69b22265690a0b5a15d6ded572ba33baf) is retained at `backup/main-before-fxceless-2026-11-26`. The adoption commit has both the previous main and upstream commit as parents, so no force-push is required. The exact external snapshot is at `upstream-snapshot/fxceless-2026-11-26`.

The validated `fix/nvidia-compatibility` branch, experimental `perf/wave64-exchange-fusion`, `perf/runtime-memory-merge`, and `integration/cgbj-nvidia-2026-11-26` branches remain independent. Their changes are not automatically included in this source tree. The only local code change at adoption is the zero-dword root constants workaround in the graphics/compute shader test harness.

Upstream includes its own diagnostic logs, documented as scrubbed and permitted by its asset policy. No title files, executable builds, or new captures were added by this adoption. The original logs have not been independently audited.

## Validation on Windows

```powershell
cargo build --release --locked -p victory
cargo test --workspace --locked -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

The source baseline is not yet validated on our RTX 5060 Ti, and upstream's measured RTX 4070 results are not performance guarantees on an 8 GB VRAM / 16 GB RAM machine. The previous validated branch remains available for comparison.
