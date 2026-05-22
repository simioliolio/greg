use std::collections::VecDeque;
use std::time::Instant;

const CONFIRMATION_TOLERANCE: f64 = 0.020;
const CONFIRMATION_THRESHOLD: usize = 3;
const MAX_CANDIDATE_RUNS: usize = 5;

#[derive(Clone, Debug)]
pub struct ConfirmedBeat {
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
struct CandidateBeat {
    timestamp: Instant,
}

pub struct BeatConfirmation {
    runs: VecDeque<Vec<CandidateBeat>>,
}

impl Default for BeatConfirmation {
    fn default() -> Self {
        Self::new()
    }
}

impl BeatConfirmation {
    pub fn new() -> Self {
        Self {
            runs: VecDeque::new(),
        }
    }

    pub fn add_candidates(&mut self, candidates: Vec<Instant>) {
        if self.runs.len() >= MAX_CANDIDATE_RUNS {
            self.runs.pop_front();
        }
        self.runs.push_back(
            candidates
                .into_iter()
                .map(|timestamp| CandidateBeat { timestamp })
                .collect(),
        );
    }

    pub fn confirm(&self) -> Vec<ConfirmedBeat> {
        if self.runs.len() < CONFIRMATION_THRESHOLD {
            return Vec::new();
        }

        let mut all_candidates: Vec<Instant> = self
            .runs
            .iter()
            .flat_map(|run| run.iter().map(|c| c.timestamp))
            .collect();
        all_candidates.sort();

        let mut confirmed = Vec::new();
        let mut used = vec![false; all_candidates.len()];

        for i in 0..all_candidates.len() {
            if used[i] {
                continue;
            }

            let anchor = all_candidates[i];
            let mut cluster = vec![i];

            for j in (i + 1)..all_candidates.len() {
                if used[j] {
                    continue;
                }
                let diff = all_candidates[j].duration_since(anchor).as_secs_f64();
                if diff > CONFIRMATION_TOLERANCE {
                    break;
                }
                cluster.push(j);
            }

            if cluster.len() >= CONFIRMATION_THRESHOLD && has_distinct_runs(&self.runs, &cluster, &all_candidates) {
                for &idx in &cluster {
                    used[idx] = true;
                }
                let median_idx = cluster[cluster.len() / 2];
                confirmed.push(ConfirmedBeat {
                    timestamp: all_candidates[median_idx],
                });
            }
        }

        confirmed
    }
}

fn has_distinct_runs(
    runs: &VecDeque<Vec<CandidateBeat>>,
    cluster_indices: &[usize],
    all_candidates: &[Instant],
) -> bool {
    let cluster_timestamps: Vec<Instant> = cluster_indices
        .iter()
        .map(|&i| all_candidates[i])
        .collect();

    let mut run_count = 0;
    for run in runs {
        let has_match = run.iter().any(|c| {
            cluster_timestamps
                .iter()
                .any(|&ct| instant_diff(c.timestamp, ct) <= CONFIRMATION_TOLERANCE)
        });
        if has_match {
            run_count += 1;
        }
    }
    run_count >= CONFIRMATION_THRESHOLD
}

fn instant_diff(a: Instant, b: Instant) -> f64 {
    if a >= b {
        a.duration_since(b).as_secs_f64()
    } else {
        b.duration_since(a).as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_instants(base: Instant, offsets_ms: &[u64]) -> Vec<Instant> {
        offsets_ms
            .iter()
            .map(|&ms| base + Duration::from_millis(ms))
            .collect()
    }

    #[test]
    fn three_runs_within_tolerance_confirms() {
        let base = Instant::now();
        let mut bc = BeatConfirmation::new();

        bc.add_candidates(make_instants(base, &[1000, 2000]));
        bc.add_candidates(make_instants(base, &[1005, 2010]));
        bc.add_candidates(make_instants(base, &[1010, 2015]));

        let confirmed = bc.confirm();
        assert_eq!(confirmed.len(), 2, "expected 2 confirmed beats");

        let t0_offset = confirmed[0].timestamp.duration_since(base).as_millis();
        let t1_offset = confirmed[1].timestamp.duration_since(base).as_millis();
        assert!(
            (1000..=1010).contains(&t0_offset),
            "first confirmed beat at {t0_offset}ms"
        );
        assert!(
            (2000..=2015).contains(&t1_offset),
            "second confirmed beat at {t1_offset}ms"
        );
    }

    #[test]
    fn two_runs_not_enough() {
        let base = Instant::now();
        let mut bc = BeatConfirmation::new();

        bc.add_candidates(make_instants(base, &[1000]));
        bc.add_candidates(make_instants(base, &[1005]));

        let confirmed = bc.confirm();
        assert!(confirmed.is_empty());
    }

    #[test]
    fn beats_beyond_tolerance_not_merged() {
        let base = Instant::now();
        let mut bc = BeatConfirmation::new();

        // 50ms apart — well beyond 20ms tolerance
        bc.add_candidates(make_instants(base, &[1000]));
        bc.add_candidates(make_instants(base, &[1050]));
        bc.add_candidates(make_instants(base, &[1100]));

        let confirmed = bc.confirm();
        assert!(confirmed.is_empty());
    }

    #[test]
    fn oldest_run_dropped_after_max() {
        let base = Instant::now();
        let mut bc = BeatConfirmation::new();

        // Add 5 runs all agreeing on beat at 1000ms
        for i in 0..5 {
            bc.add_candidates(make_instants(base, &[1000 + i * 5]));
        }
        assert_eq!(bc.confirm().len(), 1);

        // Add a 6th run with a beat at 5000ms — oldest (1000ms) should be dropped
        // Now only 4 runs have beats near 1000ms, but that's still >= 3
        bc.add_candidates(make_instants(base, &[5000]));
        assert_eq!(bc.runs.len(), 5);
    }

    #[test]
    fn empty_candidates_handled() {
        let mut bc = BeatConfirmation::new();
        bc.add_candidates(vec![]);
        bc.add_candidates(vec![]);
        bc.add_candidates(vec![]);
        let confirmed = bc.confirm();
        assert!(confirmed.is_empty());
    }
}
