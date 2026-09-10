use crate::{Message, MessageSnapshot};

pub const MESSAGE_PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MessageReadFilter {
    #[default]
    All,
    Unread,
    Read,
}

impl MessageReadFilter {
    fn matches(self, message: &Message) -> bool {
        match self {
            Self::All => true,
            Self::Unread => !message.read,
            Self::Read => message.read,
        }
    }
}

/// One page borrows at most 20 records from an immutable Store snapshot.
pub struct MessagePage<'a> {
    pub messages: Vec<&'a Message>,
    pub number: usize,
    pub page_count: usize,
    pub matching_count: usize,
}

impl MessageSnapshot {
    pub fn page(&self, filter: MessageReadFilter, requested_page: usize) -> MessagePage<'_> {
        let matching_count = self
            .messages
            .iter()
            .filter(|message| filter.matches(message))
            .count();
        let page_count = matching_count.div_ceil(MESSAGE_PAGE_SIZE).max(1);
        let number = requested_page.clamp(1, page_count);
        let messages = self
            .messages
            .iter()
            .filter(|message| filter.matches(message))
            .skip((number - 1) * MESSAGE_PAGE_SIZE)
            .take(MESSAGE_PAGE_SIZE)
            .collect();
        MessagePage {
            messages,
            number,
            page_count,
            matching_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageSeverity;

    fn snapshot(count: usize) -> MessageSnapshot {
        MessageSnapshot {
            messages: (0..count)
                .map(|index| Message {
                    id: index.to_string(),
                    title: String::new(),
                    body: String::new(),
                    source: String::new(),
                    channel: "application".into(),
                    category: "general".into(),
                    severity: MessageSeverity::Info,
                    created_at: chrono::Utc::now(),
                    read: index % 2 == 0,
                    target: None,
                    deduplication_key: None,
                    metadata: Default::default(),
                })
                .collect(),
            unread_count: count / 2,
        }
    }

    #[test]
    fn pages_cover_history_once_with_a_fixed_projection_bound() {
        for count in [0, 1, 20, 21, 78, 1_000] {
            let snapshot = snapshot(count);
            let mut ids = Vec::new();
            let first = snapshot.page(MessageReadFilter::All, 0);
            assert_eq!(first.number, 1);
            for number in 1..=first.page_count {
                let page = snapshot.page(MessageReadFilter::All, number);
                assert!(page.messages.len() <= MESSAGE_PAGE_SIZE);
                assert_eq!(page.matching_count, count);
                ids.extend(page.messages.iter().map(|message| message.id.clone()));
            }
            assert_eq!(
                ids,
                (0..count)
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                snapshot.page(MessageReadFilter::All, usize::MAX).number,
                first.page_count
            );
        }
    }

    #[test]
    fn filtering_precedes_paging_and_borrows_the_original_records() {
        let snapshot = snapshot(78);
        let unread = snapshot.page(MessageReadFilter::Unread, 2);
        assert_eq!(unread.matching_count, 39);
        assert_eq!(unread.page_count, 2);
        assert_eq!(unread.messages.len(), 19);
        assert_eq!(unread.messages[0].id, "41");
        assert_eq!(unread.messages.last().unwrap().id, "77");
        assert!(std::ptr::eq(unread.messages[0], &snapshot.messages[41]));
        assert!(unread.messages.iter().all(|message| !message.read));
        let read = snapshot.page(MessageReadFilter::Read, 2);
        assert_eq!(read.messages[0].id, "40");
        assert!(read.messages.iter().all(|message| message.read));
    }

    #[test]
    fn a_shrinking_result_clamps_to_the_last_nonempty_page() {
        let before = snapshot(41);
        assert_eq!(before.page(MessageReadFilter::All, 3).messages.len(), 1);
        let after = snapshot(40);
        let page = after.page(MessageReadFilter::All, 3);
        assert_eq!(page.number, 2);
        assert_eq!(page.messages.len(), 20);
        let empty = MessageSnapshot::default();
        let page = empty.page(MessageReadFilter::Unread, 3);
        assert_eq!(
            (page.number, page.page_count, page.matching_count),
            (1, 1, 0)
        );
        assert!(page.messages.is_empty());
    }
}
