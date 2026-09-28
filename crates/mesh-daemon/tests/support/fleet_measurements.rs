//! Nearest-rank percentile for the native acceptance report, not an event-lag estimate.
pub fn p95(samples: &[u64]) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() - sorted.len() / 20 - 1])
}
#[test]
fn nearest_rank_preserves_outliers_and_never_invents_empty_measurements() {
    assert_eq!(p95(&[]), None);
    assert_eq!(p95(&[900]), Some(900));
    assert_eq!(p95(&[9, 1, 4, 5]), Some(9));
    assert_eq!(p95(&(1..=20).rev().collect::<Vec<_>>()), Some(19));
    assert_eq!(p95(&(1..=21).collect::<Vec<_>>()), Some(20));
    assert_eq!(p95(&(1..=100).collect::<Vec<_>>()), Some(95));
}
