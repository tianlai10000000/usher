//! Finite accepted response inputs for provider metadata and diff JSON.
//!
//! These limits apply to each consumed HTTP response, not all paginated results,
//! normalized state, decoded text or retained caches. Source and conflict reads
//! keep their separate, stricter contracts.

pub(super) const MAX_METADATA_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_DIFF_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
