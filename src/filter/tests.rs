//! Unit tests for the BounceFilter logic.

#[cfg(test)]
mod tests {
    use crate::filter::BounceFilter;
    use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN};

    // --- Test Helpers ---

    const KEY_A: u16 = 30;
    const KEY_B: u16 = 48;
    const DEBOUNCE_MS: u64 = 10; // Use a consistent debounce time for most tests
    const DEBOUNCE_US: u64 = DEBOUNCE_MS * 1000;

    // Helper to create a key event with a specific microsecond timestamp
    fn key_ev(ts_us: u64, code: u16, value: i32) -> input_event {
        input_event {
            time: timeval {
                tv_sec: (ts_us / 1_000_000) as i64,
                tv_usec: (ts_us % 1_000_000) as i64,
            },
            type_: EV_KEY as u16,
            code,
            value,
        }
    }

    // Helper to create a non-key event (e.g., SYN)
    fn non_key_ev(ts_us: u64) -> input_event {
        input_event {
            time: timeval {
                tv_sec: (ts_us / 1_000_000) as i64,
                tv_usec: (ts_us % 1_000_000) as i64,
            },
            type_: EV_SYN as u16, // Cast i32 constant to u16
            code: 0,       // SYN_REPORT
            value: 0,
        }
    }

    // Helper to process a sequence of events and return which ones were dropped (true = dropped)
    fn process_sequence(filter: &mut BounceFilter, events: &[input_event]) -> Vec<bool> {
        events.iter().map(|ev| filter.process_event(ev)).collect()
    }

    // --- Basic Bounce Tests ---

    #[test]
    fn drops_press_bounce() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
        let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A again within window (bounce)
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, true]); // e1 passes, e2 drops
    }

    #[test]
    fn drops_release_bounce() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 0); // Release A at 0ms
        let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 0); // Release A again within window (bounce)
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, true]); // e1 passes, e2 drops
    }

    #[test]
    fn passes_outside_window() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
        let e2 = key_ev(DEBOUNCE_US + 1, KEY_A, 1); // Press A again outside window
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, false]); // Both pass
    }

    #[test]
    fn passes_at_window_boundary() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
        let e2 = key_ev(DEBOUNCE_US, KEY_A, 1); // Press A exactly at window boundary
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, false]); // Both should pass (>= check)
    }

    #[test]
    fn drops_just_below_window_boundary() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
        let e2 = key_ev(DEBOUNCE_US - 1, KEY_A, 1); // Press A just inside window
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, true]); // e2 should drop (< check)
    }

    // --- Independent Filtering Tests ---

    #[test]
    fn filters_different_keys_independently() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A (Pass)
        let e2 = key_ev(DEBOUNCE_US / 3, KEY_B, 1); // Press B (Pass) - different key
        let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) - bounce of e1
        let e4 = key_ev(DEBOUNCE_US * 2 / 3, KEY_B, 1); // Press B (Drop) - bounce of e2
        let results = process_sequence(&mut filter, &[e1, e2, e3, e4]);
        assert_eq!(results, vec![false, false, true, true]);
    }

    #[test]
    fn filters_press_release_independently() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        // Scenario: Rapid press/release passes, subsequent bounces drop
        let e1 = key_ev(0, KEY_A, 1); // Press A (Pass)
        let e2 = key_ev(DEBOUNCE_US / 4, KEY_A, 0); // Release A (Pass) - different value
        let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) - bounce of e1
        let e4 = key_ev(DEBOUNCE_US * 3 / 4, KEY_A, 0); // Release A (Drop) - bounce of e2
        let results = process_sequence(&mut filter, &[e1, e2, e3, e4]);
        assert_eq!(results, vec![false, false, true, true]);
    }

     #[test]
    fn filters_release_press_independently() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        // Scenario: Start with release, then rapid press
        let e1 = key_ev(0, KEY_A, 0); // Release A (Pass) - first event
        let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Pass) - different value
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, false]);
    }

    #[test]
    fn independent_filtering_allows_release_after_dropped_press() {
         let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
         // Press A (Pass) -> Press A (Drop) -> Release A (Pass, because last *passed* release was long ago)
         let e1 = key_ev(0, KEY_A, 1); // Press A (Pass)
         let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) - bounce of e1
         let e3 = key_ev(DEBOUNCE_US, KEY_A, 0); // Release A (Pass) - first release event seen
         let results = process_sequence(&mut filter, &[e1, e2, e3]);
         assert_eq!(results, vec![false, true, false]);
    }


    // --- Special Value/Type Tests ---

    #[test]
    fn passes_non_key_events() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Press A (Pass)
        let e2 = non_key_ev(DEBOUNCE_US / 4); // SYN event (Pass)
        let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) - bounce of e1
        let e4 = non_key_ev(DEBOUNCE_US * 3 / 4); // SYN event (Pass)
        let results = process_sequence(&mut filter, &[e1, e2, e3, e4]);
        assert_eq!(results, vec![false, false, true, false]); // Only e3 drops
    }

    #[test]
    fn passes_key_repeats() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        // Key repeats (value 2) are NOT debounced
        let e1 = key_ev(0, KEY_A, 1);     // Press A (Pass)
        let e2 = key_ev(500_000, KEY_A, 2); // Repeat A (Pass)
        let e3 = key_ev(500_000 + DEBOUNCE_US / 2, KEY_A, 2); // Repeat A again quickly (Pass)
        let results = process_sequence(&mut filter, &[e1, e2, e3]);
        assert_eq!(results, vec![false, false, false]); // All pass
    }

    // --- Edge Case Tests ---

    #[test]
    fn window_zero_passes_all_key_events() {
        let mut filter = BounceFilter::new(0, 0, false, false); // Debounce time = 0ms
        let e1 = key_ev(0, KEY_A, 1);     // Press A (Pass)
        let e2 = key_ev(1, KEY_A, 1); // Press A again very quickly (Pass)
        let e3 = key_ev(2, KEY_A, 0);     // Release A (Pass)
        let e4 = key_ev(3, KEY_A, 0); // Release A again very quickly (Pass)
        let results = process_sequence(&mut filter, &[e1, e2, e3, e4]);
        assert_eq!(results, vec![false, false, false, false]); // All pass
    }

    #[test]
    fn handles_time_going_backwards() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(DEBOUNCE_US * 2, KEY_A, 1); // Press A at 20ms (Pass)
        let e2 = key_ev(DEBOUNCE_US, KEY_A, 1); // Press A "again" at 10ms (Pass) - time went back
        let results = process_sequence(&mut filter, &[e1, e2]);
        assert_eq!(results, vec![false, false]); // Both pass
    }

     #[test]
    fn initial_state_empty() {
        let filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        assert!(filter.last_event_us.is_empty());
        assert!(filter.stats.key_events_processed == 0);
    }

    #[test]
    fn stats_tracking() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let e1 = key_ev(0, KEY_A, 1); // Pass
        let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Drop
        let e3 = key_ev(DEBOUNCE_US * 2, KEY_B, 1); // Pass
        let e4 = key_ev(DEBOUNCE_US * 2 + 1, KEY_B, 0); // Pass
        let e5 = key_ev(DEBOUNCE_US * 3, KEY_B, 0); // Drop
        let _results = process_sequence(&mut filter, &[e1, e2, e3, e4, e5]);

        assert_eq!(filter.stats.key_events_processed, 5);
        assert_eq!(filter.stats.key_events_passed, 3);
        assert_eq!(filter.stats.key_events_dropped, 2);

        // Check stats for KEY_A
        let key_a_stats = filter.stats.per_key_stats.get(&KEY_A).unwrap();
        assert_eq!(key_a_stats.press.count, 1); // One dropped press
        assert_eq!(key_a_stats.release.count, 0);
        assert_eq!(key_a_stats.press.timings_us.len(), 1);
        assert_eq!(key_a_stats.press.timings_us[0], DEBOUNCE_US / 2); // Bounce diff

        // Check stats for KEY_B
        let key_b_stats = filter.stats.per_key_stats.get(&KEY_B).unwrap();
        assert_eq!(key_b_stats.press.count, 0);
        assert_eq!(key_b_stats.release.count, 1); // One dropped release
        assert_eq!(key_b_stats.release.timings_us.len(), 1);
        // Bounce diff for e5 relative to e4
        assert_eq!(key_b_stats.release.timings_us[0], (DEBOUNCE_US * 3) - (DEBOUNCE_US * 2 + 1));

        // Check near miss (none in this specific sequence)
        assert!(filter.stats.per_key_passed_near_miss_timing.is_empty());
    }

     #[test]
    fn near_miss_tracking() {
        let mut filter = BounceFilter::new(DEBOUNCE_MS, 0, false, false);
        let near_miss_time = DEBOUNCE_US + 500; // Just outside window, but < 100ms
        let far_time = DEBOUNCE_US + 200_000; // Way outside window

        let e1 = key_ev(0, KEY_A, 1); // Pass
        let e2 = key_ev(near_miss_time, KEY_A, 1); // Pass (Near Miss)
        let e3 = key_ev(far_time, KEY_A, 1); // Pass (Far)

        let _results = process_sequence(&mut filter, &[e1, e2, e3]);

        assert_eq!(filter.stats.key_events_processed, 3);
        assert_eq!(filter.stats.key_events_passed, 3);
        assert_eq!(filter.stats.key_events_dropped, 0);

        let near_misses = filter.stats.per_key_passed_near_miss_timing.get(&(KEY_A, 1)).unwrap();
        assert_eq!(near_misses.len(), 1);
        assert_eq!(near_misses[0], near_miss_time); // Diff between e2 and e1
        // e3 doesn't trigger near miss as it's > 100ms after e2
    }
}
