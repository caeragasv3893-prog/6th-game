//! Merge completed GPU writes into a title-memory snapshot and refresh the
//! host copy. The byte-level conflict policy is identical to the original
//! scalar loops in shadow.rs: a changed GPU byte wins, otherwise keep title.
//!
//! Motivation: the CPU comparison bottleneck reported by cputroll/cgbj-reupload
//! (docs/findings.md). This implementation is independent of that fork's
//! dword-run and asynchronous landing code.

pub(super) fn merge(title: &mut [u8], host: &mut [u8], before: &[u8]) -> bool {
    assert_eq!(title.len(), host.len());
    assert_eq!(title.len(), before.len());

    #[cfg(target_arch = "x86_64")]
    if title.len() >= 32 && std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 was detected and the three slices have equal length.
        return unsafe { merge_avx2(title, host, before) };
    }
    merge_scalar(title, host, before)
}

fn merge_scalar(title: &mut [u8], host: &mut [u8], before: &[u8]) -> bool {
    let mut changed = false;
    for ((t, h), b) in title.iter_mut().zip(host.iter_mut()).zip(before) {
        if *h != *b && *t != *h {
            *t = *h;
            changed = true;
        }
        *h = *t;
    }
    changed
}

/// # Safety
/// Requires AVX2 and three equal-length, non-overlapping slices.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn merge_avx2(title: &mut [u8], host: &mut [u8], before: &[u8]) -> bool {
    use std::arch::x86_64::*;

    let end = title.len() / 32 * 32;
    let mut changed = false;
    for i in (0..end).step_by(32) {
        // SAFETY: each load/store stays within its slice. Unaligned
        // intrinsics support arbitrary byte offsets and slice alignment.
        unsafe {
            let t = _mm256_loadu_si256(title.as_ptr().add(i).cast());
            let h = _mm256_loadu_si256(host.as_ptr().add(i).cast());
            let b = _mm256_loadu_si256(before.as_ptr().add(i).cast());
            let unchanged = _mm256_cmpeq_epi8(h, b);
            // Select title where GPU did not change the original byte.
            let merged = _mm256_blendv_epi8(h, t, unchanged);
            let same_title = _mm256_cmpeq_epi8(merged, t);
            changed |= _mm256_movemask_epi8(same_title) != -1;
            _mm256_storeu_si256(title.as_mut_ptr().add(i).cast(), merged);
            _mm256_storeu_si256(host.as_mut_ptr().add(i).cast(), merged);
        }
    }
    changed | merge_scalar(&mut title[end..], &mut host[end..], &before[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_merge_matches_scalar_for_unaligned_slices_and_tails() {
        let mut state = 0x93c7_91a5u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        for len in [0usize, 1, 15, 31, 32, 33, 63, 64, 65, 255, 256, 257, 4095, 65536] {
            for offset in 0..32 {
                for mode in 0..5 {
                    let before: Vec<u8> = (0..len + offset).map(|_| next()).collect();
                    let mut t = before.clone();
                    let mut h = before.clone();
                    for i in offset..offset + len {
                        if mode == 1 || mode >= 3 && next().is_multiple_of(3) {
                            t[i] = next();
                        }
                        if mode == 2 || mode >= 3 && next().is_multiple_of(5) {
                            h[i] = next();
                        }
                        if mode == 4 && next().is_multiple_of(7) {
                            t[i] = h[i];
                        }
                    }
                    let mut expected_t = t.clone();
                    let mut expected_h = h.clone();
                    let expected = merge_scalar(&mut expected_t[offset..], &mut expected_h[offset..], &before[offset..]);
                    let actual = merge(&mut t[offset..], &mut h[offset..], &before[offset..]);
                    assert_eq!(actual, expected, "{len}, {offset}, {mode}");
                    assert_eq!(t, expected_t);
                    assert_eq!(h, expected_h);
                }
            }
        }
    }

    #[test]
    fn runtime_merge_preserves_cpu_only_bytes_and_gpu_conflict_policy() {
        let before = [1, 2, 3, 4, 5];
        let mut title = [9, 2, 8, 4, 6];
        let mut host = [1, 7, 0, 4, 6];
        assert!(merge(&mut title, &mut host, &before));
        assert_eq!(title, [9, 7, 0, 4, 6]);
        assert_eq!(host, title);
        assert!(!merge(&mut title, &mut host, &before));
    }
}
