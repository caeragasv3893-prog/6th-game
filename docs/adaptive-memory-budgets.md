# Adaptive memory budgets

This first memory optimization targets idle storage only, not live resources
or the title's own allocations. The primary baseline is f58ed80.

## RAM

The periodic submission sweep samples available physical RAM. It reduces
the allowed size of three spare caches with hysteresis:

| State | Snapshot | Texture staging | List scratch | Total |
|---|---:|---:|---:|---:|
| Normal | 512 MiB | 256 MiB | 128 MiB | 896 MiB |
| Moderate | 256 MiB | 128 MiB | 64 MiB | 448 MiB |
| High | 128 MiB | 64 MiB | 32 MiB | 224 MiB |

Enter moderate below 4 GiB free, high below 2 GiB. Recover from high at
3 GiB and from moderate at 5 GiB. The sweep trims existing spare storage;
returned buffers also obey the current limit. Unlike the old snapshot
pool, an oversized single snapshot cannot bypass the aggregate cap.

These are upper bounds for idle storage with default settings, not measured
process-memory savings. Active allocations, pending GPU work, and texture
or buffer references retained by command lists are unaffected. More pool
misses and allocations are possible under pressure.

Set `VICTORY_ADAPTIVE_CACHE=0` to disable pressure-dependent reductions
(the corrected strict snapshot cap remains). `VICTORY_SNAPSHOT_POOL_MB`
and `VICTORY_STAGING_POOL_MB` set the normal limits. The existing
`VICTORY_SWEEP=0` disables periodic sweeping.

## VRAM

Speculative title-filled texture admission checks actual DXGI usage against
the configured budget percentage even below the minimum texture working set
or with an explicit texture cap. The floor scales to the smaller of 2 GiB
and one quarter of the DXGI local budget. Required render targets are not
subject to this admission check. Missing/zero DXGI budget retains the
fallback cap. This is a soft admission policy, not a hard VRAM allocation
limit: a texture can cross the threshold and live resources remain live.

## Validation

```powershell
cargo test -p victory-gpu --lib --locked idle_cache_pressure -- --test-threads=1
cargo test -p victory-gpu --lib --locked snapshot_pool_strict_budget -- --test-threads=1
cargo test -p victory-gpu --lib --locked texture_room_respects -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
cargo build --release --locked -p victory
```

For runtime evaluation use the same profile, scene, cache warmup and driver
on the baseline and new main. Compare peak private committed bytes, physical
working set, local/nonlocal DXGI usage, frame-time median and p95, device
removal and missing textures. No game data or new diagnostic captures belong
in this commit. Windows compilation and GPU validation remain pending.
