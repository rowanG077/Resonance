//! Order simultaneous voice completions before returning slots to the free FIFO.
//! Equal-priority ordering affects the sound because reused slots retain LFO phase.

/// Input is newest-first studio order; preserve the partition’s unstable ties.
pub(crate) fn completion_order(voices: &mut [(usize, u32)]) {
    if voices.len() < 2 {
        return;
    }
    voices.swap(0, (voices.len() - 1) / 2);
    let mut pivot = 0;
    for next in 1..voices.len() {
        if voices[next].1 < voices[0].1 {
            pivot += 1;
            voices.swap(pivot, next);
        }
    }
    voices.swap(0, pivot);
    completion_order(&mut voices[..pivot]);
    completion_order(&mut voices[pivot + 1..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_release_keeps_the_observed_free_fifo_order() {
        // Title at 3660 ms: these four held notes finish in the same block.
        // Dolphin's later snapshot independently records free order 1,6,2,3.
        // Lower-priority voices must participate in partitioning even though
        // they are still playing; sorting only the finished voices is wrong.
        let mut voices: Vec<_> = [20, 19, 18, 17, 16, 15, 14, 6, 3, 2, 1]
            .map(|slot| {
                let priority = if [15, 16, 19].contains(&slot) { 0 } else { 64 };
                let started = if slot < 7 {
                    9
                } else if slot == 14 {
                    3428
                } else {
                    3429
                };
                (slot, (priority << 24) | (60000 - 8 * (3660 - started)))
            })
            .into();
        completion_order(&mut voices);
        assert_eq!(
            voices
                .iter()
                .filter_map(|&(slot, _)| (slot < 7).then_some(slot))
                .collect::<Vec<_>>(),
            [1, 6, 2, 3]
        );
    }
}
