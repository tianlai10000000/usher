//! Pure admission/backoff policy stored inside the Project State Store.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub(super) const MAX_LOGO_REQUESTS: usize = 4;
pub(super) const MAX_LOGO_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Queued,
    Pending(u64),
    Ready(u64),
    Failed(Instant),
}

#[derive(Clone, Debug)]
struct Entry {
    attempts: u8,
    status: Status,
}

#[derive(Clone, Default)]
pub(super) struct LogoRequests {
    sources: BTreeSet<String>,
    entries: BTreeMap<String, Entry>,
    sequence: u64,
}

#[derive(Clone, Debug)]
pub(super) struct LogoRequest {
    pub(super) url: String,
    sequence: u64,
}

impl LogoRequests {
    pub(super) fn sync_sources(&mut self, sources: BTreeSet<String>) -> bool {
        if self.sources == sources {
            return false;
        }
        self.entries.retain(|url, entry| {
            sources.contains(url) || matches!(entry.status, Status::Pending(_))
        });
        for url in &sources {
            self.entries.entry(url.clone()).or_insert(Entry {
                attempts: 0,
                status: Status::Queued,
            });
        }
        self.sources = sources;
        true
    }

    pub(super) fn claim(&mut self, now: Instant) -> Vec<LogoRequest> {
        let mut slots = MAX_LOGO_REQUESTS.saturating_sub(
            self.entries
                .values()
                .filter(|entry| matches!(entry.status, Status::Pending(_)))
                .count(),
        );
        let mut requests = Vec::with_capacity(slots);
        for (url, entry) in &mut self.entries {
            if slots == 0 {
                break;
            }
            if !self.sources.contains(url) {
                continue;
            }
            let eligible = match entry.status {
                Status::Queued => true,
                Status::Failed(retry_at) => entry.attempts < MAX_LOGO_ATTEMPTS && now >= retry_at,
                _ => false,
            };
            if eligible {
                self.sequence += 1;
                entry.attempts += 1;
                entry.status = Status::Pending(self.sequence);
                requests.push(LogoRequest {
                    url: url.clone(),
                    sequence: self.sequence,
                });
                slots -= 1;
            }
        }
        requests
    }

    pub(super) fn complete(
        &mut self,
        request: &LogoRequest,
        succeeded: bool,
        now: Instant,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&request.url) else {
            return false;
        };
        if entry.status != Status::Pending(request.sequence) {
            return false;
        }
        if !self.sources.contains(&request.url) {
            self.entries.remove(&request.url);
            return true;
        }
        entry.status = if succeeded {
            Status::Ready(request.sequence)
        } else {
            Status::Failed(now + Duration::from_secs(2u64.pow(u32::from(entry.attempts))))
        };
        true
    }

    pub(super) fn retry_failed(&mut self) -> bool {
        let mut changed = false;
        for entry in self.entries.values_mut() {
            if matches!(entry.status, Status::Failed(_)) {
                entry.attempts = 0;
                entry.status = Status::Queued;
                changed = true;
            }
        }
        changed
    }

    pub(super) fn next_retry(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|entry| match entry.status {
                Status::Failed(deadline) if entry.attempts < MAX_LOGO_ATTEMPTS => Some(deadline),
                _ => None,
            })
            .min()
    }

    pub(super) fn ready_revision(&self, url: &str) -> u64 {
        self.entries
            .get(url)
            .and_then(|entry| match entry.status {
                Status::Ready(sequence) => Some(sequence),
                _ => None,
            })
            .unwrap_or(0)
    }

    pub(super) fn failed(&self, url: &str) -> bool {
        self.entries
            .get(url)
            .is_some_and(|entry| matches!(entry.status, Status::Failed(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(count: usize) -> LogoRequests {
        let mut requests = LogoRequests::default();
        requests.sync_sources((0..count).map(|i| format!("url-{i:04}")).collect());
        requests
    }

    #[test]
    fn retries_have_cooldowns_and_a_finite_budget_then_manual_reload_can_recover() {
        let mut policy = policy(1);
        let mut now = Instant::now();
        for attempt in 1..=3 {
            let request = policy.claim(now).pop().unwrap();
            assert!(policy.claim(now).is_empty());
            policy.complete(&request, false, now);
            assert!(policy.claim(now).is_empty());
            now += Duration::from_secs(2u64.pow(attempt));
        }
        assert!(policy.claim(now + Duration::from_secs(3600)).is_empty());
        assert!(policy.next_retry().is_none());
        assert!(policy.retry_failed());
        let request = policy.claim(now).pop().unwrap();
        policy.complete(&request, true, now);
        assert!(!policy.failed(&request.url));
        assert!(!policy.retry_failed());
        assert!(policy.claim(now).is_empty());
    }

    #[test]
    fn admission_is_bounded_and_a_reload_does_not_duplicate_in_flight_requests() {
        let mut policy = policy(32);
        let now = Instant::now();
        let active = policy.claim(now);
        assert_eq!(active.len(), MAX_LOGO_REQUESTS);
        assert!(policy.claim(now).is_empty());
        assert!(!policy.retry_failed());
        assert!(policy.claim(now).is_empty());
        policy.complete(&active[0], true, now);
        assert_eq!(policy.claim(now).len(), 1);
        assert!(!policy.complete(&active[0], false, now));
    }

    #[test]
    fn retired_sources_release_metadata_after_their_in_flight_request_completes() {
        let mut policy = policy(1000);
        let now = Instant::now();
        let active = policy.claim(now);
        policy.sync_sources(BTreeSet::new());
        assert_eq!(policy.entries.len(), MAX_LOGO_REQUESTS);
        for request in active {
            policy.complete(&request, false, now);
        }
        assert!(policy.entries.is_empty());
        assert!(policy.claim(now).is_empty());
        assert!(policy.next_retry().is_none());
    }

    #[test]
    fn stale_completion_cannot_replace_a_new_attempt() {
        let mut policy = policy(1);
        let now = Instant::now();
        let old = policy.claim(now).pop().unwrap();
        policy.complete(&old, false, now);
        policy.retry_failed();
        let current = policy.claim(now).pop().unwrap();
        assert!(!policy.complete(&old, true, now));
        assert!(policy.complete(&current, true, now));
    }
}
