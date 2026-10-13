//! Bounded page links shared by application-owned Primer pagination bridges.

/// Primer's default one-page margin and two surrounding pages, with expanded
/// seven-page edge windows. Zero denotes a noninteractive truncation indicator.
///
/// Page targets are bounded to Slint's positive `int` range before constructing
/// links, so very large counts cannot produce duplicate saturated page targets.
pub fn pagination_links(current: usize, total: usize) -> Vec<i32> {
    let total = total.min(i32::MAX as usize);
    if total == 0 {
        return Vec::new();
    }
    let current = current.clamp(1, total);
    if total <= 9 {
        return (1..=total).map(|page| page as i32).collect();
    }
    let start = current.saturating_sub(2).clamp(1, total - 6);
    let end = (current + 2).max(7).min(total);
    let mut links = vec![1];
    if start > 3 {
        links.push(0);
    } else if start == 3 {
        links.push(2);
    }
    links.extend((start.max(2)..=end.min(total - 1)).map(|page| page as i32));
    if end < total - 2 {
        links.push(0);
    } else if end == total - 2 {
        links.push((total - 1) as i32);
    }
    links.push(total as i32);
    links
}

#[cfg(test)]
mod tests {
    use super::pagination_links;

    #[test]
    fn small_collections_show_every_page() {
        assert!(pagination_links(1, 0).is_empty());
        assert_eq!(pagination_links(1, 1), [1]);
        assert_eq!(pagination_links(4, 9), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn edge_windows_show_seven_pages_and_the_opposite_margin() {
        assert_eq!(pagination_links(1, 20), [1, 2, 3, 4, 5, 6, 7, 0, 20]);
        assert_eq!(pagination_links(2, 20), [1, 2, 3, 4, 5, 6, 7, 0, 20]);
        assert_eq!(pagination_links(5, 20), [1, 2, 3, 4, 5, 6, 7, 0, 20]);
        assert_eq!(pagination_links(18, 20), [1, 0, 14, 15, 16, 17, 18, 19, 20]);
        assert_eq!(pagination_links(20, 20), [1, 0, 14, 15, 16, 17, 18, 19, 20]);
    }

    #[test]
    fn middle_windows_keep_two_neighbors_and_both_margins() {
        assert_eq!(pagination_links(6, 20), [1, 0, 4, 5, 6, 7, 8, 0, 20]);
        assert_eq!(pagination_links(10, 20), [1, 0, 8, 9, 10, 11, 12, 0, 20]);
        assert_eq!(pagination_links(16, 20), [1, 0, 14, 15, 16, 17, 18, 19, 20]);
    }

    #[test]
    fn current_page_is_clamped_and_large_counts_remain_bounded() {
        assert_eq!(pagination_links(0, 20), pagination_links(1, 20));
        assert_eq!(pagination_links(usize::MAX, 20), pagination_links(20, 20));
        assert_eq!(
            pagination_links(usize::MAX, usize::MAX),
            [
                1,
                0,
                i32::MAX - 6,
                i32::MAX - 5,
                i32::MAX - 4,
                i32::MAX - 3,
                i32::MAX - 2,
                i32::MAX - 1,
                i32::MAX
            ]
        );
        assert_eq!(
            pagination_links(1, usize::MAX),
            [1, 2, 3, 4, 5, 6, 7, 0, i32::MAX]
        );
    }

    #[test]
    fn all_windows_fit_the_nine_slots_and_expose_the_current_page_once() {
        for total in 1..=150 {
            for current in 1..=total {
                let links = pagination_links(current, total);
                assert!(links.len() <= 9, "current={current}, total={total}");
                assert_eq!(links.first(), Some(&1));
                assert_eq!(links.last(), Some(&(total as i32)));
                assert_eq!(
                    links.iter().filter(|&&page| page == current as i32).count(),
                    1
                );
                let pages = links
                    .iter()
                    .copied()
                    .filter(|&page| page != 0)
                    .collect::<Vec<_>>();
                assert!(pages.windows(2).all(|pair| pair[0] < pair[1]));
                for surrounding in current.saturating_sub(2).max(1)..=(current + 2).min(total) {
                    assert!(pages.contains(&(surrounding as i32)));
                }
                for (index, &link) in links.iter().enumerate() {
                    if link == 0 {
                        assert!(index > 0 && index < links.len() - 1);
                        assert!(links[index + 1] - links[index - 1] > 2);
                    }
                }
            }
        }
    }
}
