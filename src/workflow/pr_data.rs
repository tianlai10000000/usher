//! Session-scoped PR records shared by list, review and background readers.
//!
//! Read tickets are reserved at request initiation; mutation tickets are
//! reserved after provider success, before publishing the confirmed result.
//! Reservation only touches an atomic counter and is safe on the event loop.
//! All publish methods dispatch typed actions
//! and must run on a worker. Subscribers receive bounded domain snapshots;
//! this facade never owns Slint projections, persistence or worker threads.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use usher_state_store::{LocalStateStore, Reduction, StateStore, Subscription};

use crate::{
    git::{
        PullRequestActionState, PullRequestItem, PullRequestReadinessKind, PullRequestReviewAction,
        PullRequestReviewStatus,
    },
    project::ProjectRepositoryMetadata,
};

const RECORD_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrKey {
    repository: String,
    number: String,
}

impl PrKey {
    pub(crate) fn new(metadata: &ProjectRepositoryMetadata, number: &str) -> Self {
        // Reuse the provider/project/remote/credential identity already used
        // for conflict observations. No token or translated copy is retained.
        let key = super::pr_conflict_status::Key::new(metadata, number);
        Self {
            repository: key.repository,
            number: key.number,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PrRecord {
    pub(crate) summary: Option<PullRequestItem>,
    /// None means the decision is unavailable, including after dismissal.
    /// Some(None) is an authoritative read reporting no active decision.
    pub(crate) review_status: Option<PullRequestReviewStatus>,
    pub(crate) lifecycle: Option<String>,
    pub(crate) action_state: Option<PullRequestActionState>,
}

impl PrRecord {
    fn synchronize_summary(&mut self) {
        if let Some(summary) = &mut self.summary {
            summary.review_status = self.review_status.unwrap_or_default();
            if let Some(lifecycle) = &self.lifecycle {
                summary.state.clone_from(lifecycle);
            }
        }
    }

    fn overlay(&self, item: &mut PullRequestItem) {
        if let Some(summary) = &self.summary {
            item.clone_from(summary);
        }
        item.review_status = self.review_status.unwrap_or_default();
        if let Some(lifecycle) = &self.lifecycle {
            item.state.clone_from(lifecycle);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PrDataSnapshot {
    pub(crate) revision: u64,
    pub(crate) records: Vec<(PrKey, PrRecord)>,
}

impl PrDataSnapshot {
    pub(crate) fn overlay(
        &self,
        metadata: &ProjectRepositoryMetadata,
        items: &mut Vec<PullRequestItem>,
    ) {
        let repository = PrKey::new(metadata, "").repository;
        for item in items {
            if let Some((_, record)) = self
                .records
                .iter()
                .find(|(key, _)| key.repository == repository && key.number == item.number)
            {
                record.overlay(item);
            }
        }
    }
}

#[derive(Clone)]
struct Entry {
    key: PrKey,
    ticket: u64,
    list_ticket: u64,
    action_ticket: u64,
    mutation_ticket: u64,
    record: PrRecord,
}

#[derive(Clone, Default)]
struct PrDataState {
    entries: Vec<Entry>,
    floor_ticket: u64,
}

enum PrDataAction {
    List(u64, Vec<(PrKey, PullRequestItem)>),
    ActionState(u64, PrKey, PullRequestActionState),
    ActionUnavailable(u64, PrKey),
    Review(u64, PrKey, PullRequestReviewAction),
    Dismissal(u64, PrKey),
    Merge(u64, PrKey),
}

#[derive(Clone, Copy)]
enum Source {
    List,
    Action,
    Mutation,
}

type Store = dyn StateStore<PrDataState, PrDataAction, bool>;

#[derive(Clone)]
pub(crate) struct PrDataFacade {
    store: Arc<Store>,
    next_ticket: Arc<AtomicU64>,
}

impl PrDataFacade {
    fn new() -> Self {
        Self {
            store: Arc::new(LocalStateStore::new(PrDataState::default(), reduce)),
            next_ticket: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Reserve at read initiation or confirmed mutation success, without a reducer.
    pub(crate) fn ticket(&self) -> u64 {
        self.next_ticket
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("PR data request ticket exhausted")
            + 1
    }

    /// Publish a fetched page on its worker before enqueuing UI projection.
    pub(crate) fn publish_list(
        &self,
        ticket: u64,
        metadata: &ProjectRepositoryMetadata,
        items: &[PullRequestItem],
    ) -> bool {
        let repository = PrKey::new(metadata, "").repository;
        let records = items
            .iter()
            .map(|item| {
                (
                    PrKey {
                        repository: repository.clone(),
                        number: item.number.clone(),
                    },
                    item.clone(),
                )
            })
            .collect();
        self.store
            .dispatch(PrDataAction::List(ticket, records))
            .output
    }

    pub(crate) fn publish_action(
        &self,
        ticket: u64,
        key: &PrKey,
        state: PullRequestActionState,
    ) -> bool {
        self.store
            .dispatch(PrDataAction::ActionState(ticket, key.clone(), state))
            .output
    }

    pub(crate) fn publish_action_unavailable(&self, ticket: u64, key: &PrKey) -> bool {
        self.store
            .dispatch(PrDataAction::ActionUnavailable(ticket, key.clone()))
            .output
    }

    /// A provider-confirmed review updates the record immediately and clears
    /// old write capabilities until a fresh action-state read arrives.
    pub(crate) fn publish_review(
        &self,
        ticket: u64,
        key: &PrKey,
        action: PullRequestReviewAction,
    ) -> bool {
        self.store
            .dispatch(PrDataAction::Review(ticket, key.clone(), action))
            .output
    }

    pub(crate) fn publish_dismissal(&self, ticket: u64, key: &PrKey) -> bool {
        self.store
            .dispatch(PrDataAction::Dismissal(ticket, key.clone()))
            .output
    }

    pub(crate) fn publish_merge(&self, ticket: u64, key: &PrKey) -> bool {
        self.store
            .dispatch(PrDataAction::Merge(ticket, key.clone()))
            .output
    }

    // Stable read selector for additional consumers of the shared PR record.
    #[allow(dead_code)]
    pub(crate) fn get(&self, key: &PrKey) -> Option<PrRecord> {
        self.store.snapshot().select(|state| {
            state
                .entries
                .iter()
                .find(|entry| entry.key == *key)
                .map(|entry| entry.record.clone())
        })
    }

    /// Overlay current shared data when the list chooses to project a page.
    /// This does not dispatch, notify, or rebuild any live Slint model.
    pub(crate) fn overlay(
        &self,
        metadata: &ProjectRepositoryMetadata,
        items: &mut Vec<PullRequestItem>,
    ) {
        let repository = PrKey::new(metadata, "").repository;
        self.store.snapshot().select(|state| {
            for item in items {
                if let Some(entry) = state.entries.iter().find(|entry| {
                    entry.key.repository == repository && entry.key.number == item.number
                }) {
                    entry.record.overlay(item);
                }
            }
        });
    }

    pub(crate) fn subscribe(
        &self,
        listener: impl Fn(PrDataSnapshot) + Send + Sync + 'static,
    ) -> Subscription {
        self.store.subscribe(Arc::new(move |snapshot| {
            listener(PrDataSnapshot {
                revision: snapshot.revision(),
                records: snapshot
                    .state()
                    .entries
                    .iter()
                    .map(|entry| (entry.key.clone(), entry.record.clone()))
                    .collect(),
            });
        }))
    }
}

pub(crate) fn shared() -> PrDataFacade {
    static SHARED: OnceLock<PrDataFacade> = OnceLock::new();
    SHARED.get_or_init(PrDataFacade::new).clone()
}

fn update_record(
    state: &mut PrDataState,
    ticket: u64,
    key: PrKey,
    source: Source,
    update: impl FnOnce(&mut Entry),
) -> bool {
    let position = state.entries.iter().position(|entry| entry.key == key);
    if position.is_none() && ticket <= state.floor_ticket {
        return false;
    }
    if let Some(position) = position {
        let entry = &state.entries[position];
        let source_ticket = match source {
            Source::List => entry.list_ticket,
            Source::Action => entry.action_ticket,
            Source::Mutation => entry.mutation_ticket,
        };
        // Independent read sources cannot obsolete each other. A confirmed
        // mutation is the shared barrier for all reads that began before it.
        if ticket <= source_ticket || ticket < entry.mutation_ticket {
            return false;
        }
    }
    let mut entry = position
        .map(|position| state.entries.remove(position))
        .unwrap_or_else(|| Entry {
            key: key.clone(),
            ticket: 0,
            list_ticket: 0,
            action_ticket: 0,
            mutation_ticket: 0,
            record: PrRecord::default(),
        });
    entry.ticket = entry.ticket.max(ticket);
    match source {
        Source::List => entry.list_ticket = ticket,
        Source::Action => entry.action_ticket = ticket,
        Source::Mutation => entry.mutation_ticket = ticket,
    }
    update(&mut entry);
    entry.record.synchronize_summary();
    state.entries.push(entry);
    if state.entries.len() > RECORD_LIMIT {
        let oldest = state
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.ticket)
            .map(|(position, _)| position)
            .expect("PR record cache is non-empty");
        let evicted = state.entries.remove(oldest);
        state.floor_ticket = state.floor_ticket.max(evicted.ticket);
        return evicted.key != key;
    }
    true
}

fn reduce(current: &PrDataState, action: PrDataAction) -> Reduction<PrDataState, bool> {
    let mut next = current.clone();
    let accepted = match action {
        PrDataAction::List(ticket, items) => {
            let mut accepted = false;
            for (key, item) in items {
                accepted |= update_record(&mut next, ticket, key, Source::List, move |entry| {
                    let preserve_decision =
                        ticket < entry.action_ticket || ticket <= entry.mutation_ticket;
                    let record = &mut entry.record;
                    if !preserve_decision {
                        if record.lifecycle.as_ref() != Some(&item.state)
                            || record.review_status != Some(item.review_status)
                        {
                            record.action_state = None;
                        }
                        record.lifecycle = Some(item.state.clone());
                        record.review_status = Some(item.review_status);
                    }
                    record.summary = Some(item);
                });
            }
            accepted
        }
        PrDataAction::ActionState(ticket, key, state) => {
            update_record(&mut next, ticket, key, Source::Action, move |entry| {
                let record = &mut entry.record;
                record.review_status = action_review_status(&state.review_state);
                match state.readiness.kind {
                    PullRequestReadinessKind::Merged => record.lifecycle = Some("merged".into()),
                    PullRequestReadinessKind::Closed => record.lifecycle = Some("closed".into()),
                    PullRequestReadinessKind::Unavailable => {
                        if record.lifecycle.is_none() {
                            record.lifecycle = Some(
                                record
                                    .summary
                                    .as_ref()
                                    .map(|summary| summary.state.clone())
                                    .unwrap_or_else(|| "open".into()),
                            );
                        }
                    }
                    _ => record.lifecycle = Some("open".into()),
                }
                record.action_state = Some(state);
            })
        }
        PrDataAction::ActionUnavailable(ticket, key) => {
            update_record(&mut next, ticket, key, Source::Action, |entry| {
                entry.record.action_state = None;
            })
        }
        PrDataAction::Review(ticket, key, action) => {
            update_record(&mut next, ticket, key, Source::Mutation, |entry| {
                let record = &mut entry.record;
                match action {
                    PullRequestReviewAction::Approve => {
                        record.review_status = Some(PullRequestReviewStatus::Approved);
                    }
                    PullRequestReviewAction::RequestChanges => {
                        record.review_status = Some(PullRequestReviewStatus::ChangesRequested);
                    }
                    PullRequestReviewAction::Comment => {
                        if let Some(summary) = &mut record.summary {
                            summary.conversation_count = summary
                                .conversation_count
                                .map(|count| count.saturating_add(1));
                        }
                    }
                }
                record.action_state = None;
            })
        }
        PrDataAction::Dismissal(ticket, key) => {
            update_record(&mut next, ticket, key, Source::Mutation, |entry| {
                // Other users can still have active decisions. Only the immediate
                // authoritative refresh can establish the aggregate after dismissal.
                entry.record.review_status = None;
                entry.record.action_state = None;
            })
        }
        PrDataAction::Merge(ticket, key) => {
            update_record(&mut next, ticket, key, Source::Mutation, |entry| {
                entry.record.lifecycle = Some("merged".into());
                entry.record.action_state = None;
            })
        }
    };
    if accepted {
        Reduction::changed(next, true)
    } else {
        Reduction::unchanged(next, false)
    }
}

fn action_review_status(state: &str) -> Option<PullRequestReviewStatus> {
    match state.trim().to_ascii_lowercase().as_str() {
        "" => Some(PullRequestReviewStatus::None),
        "pending" | "review_pending" | "review_required" => {
            Some(PullRequestReviewStatus::ReviewRequired)
        }
        "approved" | "review_approved" => Some(PullRequestReviewStatus::Approved),
        "rejected" | "review_rejected" | "changes_requested" => {
            Some(PullRequestReviewStatus::ChangesRequested)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{
        PullRequestMergeBlockKind, PullRequestMergeMethod, PullRequestReadiness,
        PullRequestRevertStrategy,
    };
    use std::sync::Mutex;

    fn metadata(repository: &str) -> ProjectRepositoryMetadata {
        ProjectRepositoryMetadata {
            repo_type: "github".into(),
            repo_project_id: repository.into(),
            remote_repo: format!("https://github.fixture/example/{repository}.git"),
        }
    }

    fn item() -> PullRequestItem {
        PullRequestItem {
            number: "12".into(),
            title: "Review the shared record".into(),
            state: "open".into(),
            author: "author".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
            review_status: PullRequestReviewStatus::ReviewRequired,
            conflict_status: Default::default(),
            conversation_count: Some(3),
            created_at: String::new(),
            updated_at: String::new(),
            web_url: String::new(),
        }
    }

    fn action_state(state: &str, readiness: PullRequestReadinessKind) -> PullRequestActionState {
        PullRequestActionState {
            can_comment: true,
            can_approve: true,
            can_request_changes: true,
            can_merge: true,
            can_revert: false,
            review_state: state.into(),
            readiness: PullRequestReadiness::new(readiness, "fixture"),
            merge_block_kind: PullRequestMergeBlockKind::None,
            revert_strategy: PullRequestRevertStrategy::Unsupported,
            allowed_merge_methods: vec![PullRequestMergeMethod::Merge],
        }
    }

    #[test]
    fn subscribers_hydrate_and_receive_worker_commits_while_list_is_hidden() {
        let facade = PrDataFacade::new();
        let metadata = metadata("notifications");
        let key = PrKey::new(&metadata, "12");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let notifications = seen.clone();
        let subscription = facade.subscribe(move |snapshot| {
            notifications
                .lock()
                .unwrap()
                .push((snapshot.revision, snapshot.records));
        });
        assert_eq!(seen.lock().unwrap().len(), 1);
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        let record = facade.get(&key).unwrap();
        assert_eq!(
            record.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert_eq!(
            record.summary.unwrap().review_status,
            PullRequestReviewStatus::Approved
        );
        let observed = seen.lock().unwrap();
        assert_eq!(observed.len(), 3);
        assert_eq!(observed.last().unwrap().0, 2);
        assert_eq!(
            observed.last().unwrap().1[0].1.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        drop(observed);
        drop(subscription);
        facade.publish_merge(facade.ticket(), &key);
        assert_eq!(
            facade.get(&key).unwrap().lifecycle.as_deref(),
            Some("merged")
        );
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn duplicate_or_stale_tickets_do_not_notify_subscribers() {
        let facade = PrDataFacade::new();
        let metadata = metadata("no-op");
        let key = PrKey::new(&metadata, "12");
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let _subscription = facade.subscribe(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let stale = facade.ticket();
        let confirmed = facade.ticket();
        assert!(facade.publish_review(confirmed, &key, PullRequestReviewAction::Approve));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert!(!facade.publish_review(confirmed, &key, PullRequestReviewAction::Approve));
        assert!(!facade.publish_list(stale, &metadata, &[item()]));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn subscribers_update_cached_pages_without_projecting_the_hidden_list() {
        let facade = PrDataFacade::new();
        let metadata = metadata("cached-pages");
        let key = PrKey::new(&metadata, "12");
        let cached_page = Arc::new(Mutex::new(vec![item()]));
        let hidden_projection = vec![item()];
        let cached = cached_page.clone();
        let scope = metadata.clone();
        let _subscription = facade.subscribe(move |snapshot| {
            snapshot.overlay(&scope, &mut cached.lock().unwrap());
        });
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        assert_eq!(
            cached_page.lock().unwrap()[0].review_status,
            PullRequestReviewStatus::Approved
        );
        assert_eq!(
            hidden_projection[0].review_status,
            PullRequestReviewStatus::ReviewRequired
        );
    }

    #[test]
    fn older_reads_cannot_undo_a_confirmed_mutation() {
        let facade = PrDataFacade::new();
        let metadata = metadata("race");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        let old_list_ticket = facade.ticket();
        let old_action_ticket = facade.ticket();
        let mutation_ticket = facade.ticket();
        assert!(facade.publish_review(mutation_ticket, &key, PullRequestReviewAction::Approve));
        assert!(!facade.publish_list(old_list_ticket, &metadata, &[item()]));
        assert!(!facade.publish_action(
            old_action_ticket,
            &key,
            action_state("", PullRequestReadinessKind::Ready)
        ));
        assert_eq!(
            facade.get(&key).unwrap().review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert!(facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::ReviewRequired)
        ));
        assert_eq!(
            facade
                .get(&key)
                .unwrap()
                .action_state
                .unwrap()
                .readiness
                .kind,
            PullRequestReadinessKind::ReviewRequired
        );
    }

    #[test]
    fn later_summary_arriving_first_does_not_obsolete_current_action_state() {
        let facade = PrDataFacade::new();
        let metadata = metadata("independent-reads");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        let action_ticket = facade.ticket();
        let list_ticket = facade.ticket();
        let mut summary = item();
        summary.title = "A newer list summary".into();
        summary.review_status = PullRequestReviewStatus::None;
        assert!(facade.publish_list(list_ticket, &metadata, &[summary]));
        assert!(facade.publish_action(
            action_ticket,
            &key,
            action_state("approved", PullRequestReadinessKind::Ready)
        ));
        let record = facade.get(&key).unwrap();
        assert_eq!(
            record.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert!(record.action_state.unwrap().can_merge);
        let summary = record.summary.unwrap();
        assert_eq!(summary.title, "A newer list summary");
        assert_eq!(summary.review_status, PullRequestReviewStatus::Approved);

        let list_ticket = facade.ticket();
        let action_ticket = facade.ticket();
        assert!(facade.publish_action(
            action_ticket,
            &key,
            action_state("approved", PullRequestReadinessKind::Ready)
        ));
        assert!(facade.publish_list(list_ticket, &metadata, &[item()]));
        let record = facade.get(&key).unwrap();
        assert_eq!(
            record.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert!(record.action_state.unwrap().can_merge);
        assert_eq!(
            record.summary.unwrap().review_status,
            PullRequestReviewStatus::Approved
        );
    }

    #[test]
    fn fresh_action_failure_revokes_capabilities_without_erasing_known_status() {
        let facade = PrDataFacade::new();
        let metadata = metadata("action-failure");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::Ready),
        );
        assert!(facade.get(&key).unwrap().action_state.unwrap().can_merge);
        assert!(facade.publish_action_unavailable(facade.ticket(), &key));
        let record = facade.get(&key).unwrap();
        assert!(record.action_state.is_none());
        assert_eq!(
            record.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert_eq!(record.lifecycle.as_deref(), Some("open"));
    }

    #[test]
    fn authoritative_ready_detail_corrects_closed_sparse_summary() {
        let facade = PrDataFacade::new();
        let metadata = metadata("coherent-lifecycle");
        let key = PrKey::new(&metadata, "12");
        let action_ticket = facade.ticket();
        let mut summary = item();
        summary.state = "closed".into();
        facade.publish_list(facade.ticket(), &metadata, &[summary]);
        assert!(facade.publish_action(
            action_ticket,
            &key,
            action_state("approved", PullRequestReadinessKind::Ready)
        ));
        let record = facade.get(&key).unwrap();
        assert_eq!(record.lifecycle.as_deref(), Some("open"));
        assert_eq!(record.summary.unwrap().state, "open");
        assert!(record.action_state.unwrap().can_merge);
    }

    #[test]
    fn pre_mutation_action_failure_cannot_clear_fresh_capabilities() {
        let facade = PrDataFacade::new();
        let key = PrKey::new(&metadata("stale-action-failure"), "12");
        let failure_ticket = facade.ticket();
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::Ready),
        );
        assert!(!facade.publish_action_unavailable(failure_ticket, &key));
        assert!(facade.get(&key).unwrap().action_state.unwrap().can_merge);
    }

    #[test]
    fn approval_and_merge_patch_separate_record_fields() {
        let facade = PrDataFacade::new();
        let metadata = metadata("separate");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("pending", PullRequestReadinessKind::Ready),
        );
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        let approved = facade.get(&key).unwrap();
        assert_eq!(approved.lifecycle.as_deref(), Some("open"));
        assert_eq!(
            approved.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert!(approved.action_state.is_none());
        facade.publish_merge(facade.ticket(), &key);
        let merged = facade.get(&key).unwrap();
        assert_eq!(merged.lifecycle.as_deref(), Some("merged"));
        assert_eq!(
            merged.review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        assert_eq!(merged.summary.unwrap().state, "merged");
        assert!(merged.action_state.is_none());
    }

    #[test]
    fn dismissal_invalidates_decision_until_authoritative_refresh() {
        let facade = PrDataFacade::new();
        let metadata = metadata("dismiss");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        facade.publish_dismissal(facade.ticket(), &key);
        let dismissed = facade.get(&key).unwrap();
        assert!(dismissed.review_status.is_none());
        assert!(dismissed.action_state.is_none());
        assert_eq!(
            dismissed.summary.unwrap().review_status,
            PullRequestReviewStatus::None
        );
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("", PullRequestReadinessKind::Ready),
        );
        assert_eq!(
            facade.get(&key).unwrap().review_status,
            Some(PullRequestReviewStatus::None)
        );
    }

    #[test]
    fn comment_updates_known_count_once_and_overlay_waits_for_list_projection() {
        let facade = PrDataFacade::new();
        let metadata = metadata("comment");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        let mut retained_list = vec![item()];
        let ticket = facade.ticket();
        assert!(facade.publish_review(ticket, &key, PullRequestReviewAction::Comment));
        assert!(!facade.publish_review(ticket, &key, PullRequestReviewAction::Comment));
        assert_eq!(
            facade
                .get(&key)
                .unwrap()
                .summary
                .unwrap()
                .conversation_count,
            Some(4)
        );
        assert_eq!(retained_list[0].conversation_count, Some(3));
        facade.overlay(&metadata, &mut retained_list);
        assert_eq!(retained_list[0].conversation_count, Some(4));
        assert_eq!(
            retained_list[0].review_status,
            PullRequestReviewStatus::ReviewRequired
        );
    }

    #[test]
    fn repository_provider_and_credential_scopes_are_isolated() {
        let facade = PrDataFacade::new();
        let one = metadata("one");
        let two = metadata("two");
        let first = PrKey::new(&one, "12");
        let second = PrKey::new(&two, "12");
        let conflict_key = super::super::pr_conflict_status::Key::new(&one, "12");
        assert_eq!(first.repository, conflict_key.repository);
        assert_eq!(first.number, conflict_key.number);
        let mut different_provider = one.clone();
        different_provider.repo_type = "gitlab".into();
        assert_ne!(first, PrKey::new(&different_provider, "12"));
        let other_credential = PrKey {
            repository: format!("{}\nother-credential", first.repository),
            number: first.number.clone(),
        };
        facade.publish_review(facade.ticket(), &first, PullRequestReviewAction::Approve);
        assert!(facade.get(&second).is_none());
        assert!(facade.get(&other_credential).is_none());
        assert!(facade.get(&PrKey::new(&one, "13")).is_none());
        assert_eq!(
            facade.get(&first).unwrap().review_status,
            Some(PullRequestReviewStatus::Approved)
        );
    }

    #[test]
    fn records_are_bounded_and_recent_updates_are_retained() {
        let facade = PrDataFacade::new();
        let metadata = metadata("bounded");
        for number in 0..RECORD_LIMIT {
            facade.publish_review(
                facade.ticket(),
                &PrKey::new(&metadata, &number.to_string()),
                PullRequestReviewAction::Approve,
            );
        }
        let first = PrKey::new(&metadata, "0");
        facade.publish_review(
            facade.ticket(),
            &first,
            PullRequestReviewAction::RequestChanges,
        );
        facade.publish_merge(facade.ticket(), &PrKey::new(&metadata, "last"));
        assert_eq!(facade.store.snapshot().state().entries.len(), RECORD_LIMIT);
        assert!(facade.get(&PrKey::new(&metadata, "1")).is_none());
        assert_eq!(
            facade.get(&first).unwrap().review_status,
            Some(PullRequestReviewStatus::ChangesRequested)
        );
    }

    #[test]
    fn delayed_reads_do_not_evict_newer_mutations_or_revive_evicted_records() {
        let facade = PrDataFacade::new();
        let metadata = metadata("bounded-race");
        let delayed = (0..RECORD_LIMIT)
            .map(|_| facade.ticket())
            .collect::<Vec<_>>();
        let key = PrKey::new(&metadata, "mutation");
        facade.publish_review(facade.ticket(), &key, PullRequestReviewAction::Approve);
        for (number, ticket) in delayed.iter().enumerate() {
            let mut item = item();
            item.number = format!("read-{number}");
            facade.publish_list(*ticket, &metadata, &[item]);
        }
        assert_eq!(
            facade.get(&key).unwrap().review_status,
            Some(PullRequestReviewStatus::Approved)
        );
        for number in 0..RECORD_LIMIT {
            facade.publish_merge(
                facade.ticket(),
                &PrKey::new(&metadata, &format!("new-{number}")),
            );
        }
        assert!(facade.get(&key).is_none());
        assert!(!facade.publish_action(
            delayed[0],
            &key,
            action_state("", PullRequestReadinessKind::Ready)
        ));
        assert!(facade.get(&key).is_none());
    }

    #[test]
    fn list_status_changes_invalidate_action_capabilities() {
        let facade = PrDataFacade::new();
        let metadata = metadata("list-capabilities");
        let key = PrKey::new(&metadata, "12");
        facade.publish_list(facade.ticket(), &metadata, &[item()]);
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::Ready),
        );
        let mut approved = item();
        approved.review_status = PullRequestReviewStatus::Approved;
        facade.publish_list(facade.ticket(), &metadata, &[approved.clone()]);
        assert!(facade.get(&key).unwrap().action_state.is_some());
        approved.state = "merged".into();
        facade.publish_list(facade.ticket(), &metadata, &[approved]);
        assert!(facade.get(&key).unwrap().action_state.is_none());
        assert_eq!(
            facade.get(&key).unwrap().lifecycle.as_deref(),
            Some("merged")
        );
    }

    #[test]
    fn action_state_normalizes_decisions_and_terminal_lifecycle() {
        for (native, normalized) in [
            ("", PullRequestReviewStatus::None),
            ("review_pending", PullRequestReviewStatus::ReviewRequired),
            ("approved", PullRequestReviewStatus::Approved),
            ("review_approved", PullRequestReviewStatus::Approved),
            ("rejected", PullRequestReviewStatus::ChangesRequested),
            ("review_rejected", PullRequestReviewStatus::ChangesRequested),
            (
                "changes_requested",
                PullRequestReviewStatus::ChangesRequested,
            ),
        ] {
            assert_eq!(action_review_status(native), Some(normalized));
        }
        let facade = PrDataFacade::new();
        let key = PrKey::new(&metadata("terminal"), "12");
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::Closed),
        );
        assert_eq!(
            facade.get(&key).unwrap().lifecycle.as_deref(),
            Some("closed")
        );
        facade.publish_action(
            facade.ticket(),
            &key,
            action_state("approved", PullRequestReadinessKind::Merged),
        );
        assert_eq!(
            facade.get(&key).unwrap().lifecycle.as_deref(),
            Some("merged")
        );
    }
}
