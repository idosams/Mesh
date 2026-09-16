//! Deterministic reduction of a failing schedule to a one-minimal reproduction.

use crate::{FailureRecord, Schedule};

/// Reduce a failing schedule without consulting a clock, thread scheduler or random source.
///
/// The predicate is evaluated on the complete schedule first. If that does not fail, there is
/// nothing to minimize and this returns `None`. Otherwise the reducer removes contiguous chunks
/// in a fixed order, then performs a final left-to-right single-delivery pass. The returned
/// reproduction is therefore deterministic and one-minimal: removing any one remaining delivery
/// makes the supplied predicate false.
#[must_use]
pub fn minimize_failure(
    schedule: &Schedule,
    mut fails: impl FnMut(&Schedule) -> bool,
) -> Option<FailureRecord> {
    if !fails(schedule) {
        return None;
    }

    let mut active: Vec<usize> = (0..schedule.changes().len()).collect();
    let mut granularity = 2;
    while active.len() >= 2 {
        let chunk_size = active.len().div_ceil(granularity);
        let mut reduced = false;
        let mut start = 0;
        while start < active.len() {
            let end = (start + chunk_size).min(active.len());
            let candidate: Vec<usize> = active[..start]
                .iter()
                .chain(&active[end..])
                .copied()
                .collect();
            let reproduction = schedule
                .reproduction(&candidate)
                .expect("the minimizer constructs sorted in-range indexes");
            if fails(&reproduction) {
                active = candidate;
                granularity = granularity.saturating_sub(1).max(2);
                reduced = true;
                break;
            }
            start = end;
        }
        if reduced {
            continue;
        }
        if granularity >= active.len() {
            break;
        }
        granularity = (granularity * 2).min(active.len());
    }

    let mut index = 0;
    while index < active.len() {
        let mut candidate = active.clone();
        candidate.remove(index);
        let reproduction = schedule
            .reproduction(&candidate)
            .expect("the minimizer constructs sorted in-range indexes");
        if fails(&reproduction) {
            active = candidate;
            index = 0;
        } else {
            index += 1;
        }
    }

    Some(FailureRecord::capture(schedule, active))
}
