use intercept_bounce::stats::StatsCollector;

#[test]
fn test_stats_collector_basic_counts() {
    let mut stats = StatsCollector::new();
    stats.record_event(30, 1, false, None, 1000);
    stats.record_event(30, 1, true, Some(500), 2000);
    stats.record_event(30, 0, true, Some(200), 3000);
    stats.record_event(31, 1, false, None, 4000);

    assert_eq!(stats.key_events_processed, 4);
    assert_eq!(stats.key_events_passed, 2);
    assert_eq!(stats.key_events_dropped, 2);

    // Use direct indexing for arrays
    let key_a = &stats.per_key_stats[30];
    assert_eq!(key_a.press.count, 1);
    assert_eq!(key_a.release.count, 1);
    assert_eq!(key_a.repeat.count, 0);

    // Check key_b stats directly (will be default if no drops)
    let key_b = &stats.per_key_stats[31];
    assert_eq!(key_b.press.count, 0); // Key B had no drops
}

#[test]
fn test_stats_collector_near_miss() {
    let mut stats = StatsCollector::new();
    stats.record_near_miss((30, 1), 900);
    stats.record_near_miss((30, 1), 800);
    stats.record_near_miss((31, 0), 100);

    // Use direct indexing for near miss array
    let idx_a1 = 30 * 3 + 1;
    let idx_b0 = 31 * 3 + 0;
    assert_eq!(stats.per_key_passed_near_miss_timing[idx_a1].len(), 2);
    assert_eq!(stats.per_key_passed_near_miss_timing[idx_b0].len(), 1);
}

#[test]
fn test_stats_collector_json_output() {
    let mut stats = StatsCollector::new();
    stats.record_event(30, 1, false, None, 1000);
    stats.record_event(30, 1, true, Some(500), 2000);
    stats.record_event(30, 0, true, Some(200), 3000);
    stats.record_event(31, 1, false, None, 4000);

    let mut buf = Vec::new();
    // Pass None for runtime_us argument
    stats.print_stats_json(10_000, true, false, 0, None, &mut buf);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("\"debounce_time_us\""));
    assert!(s.contains("\"key_events_processed\": 4"));
    assert!(s.contains("\"press\""));
    assert!(s.contains("\"release\""));
    assert!(s.contains("\"repeat\""));
}

// Removed test_stats_collector_runtime_fields as these fields are gone from StatsCollector

// Additional test: ensure stats are correct for only passed events
#[test]
fn test_stats_collector_only_passed() {
    let mut stats = StatsCollector::new();
    stats.record_event(40, 1, false, None, 1000);
    stats.record_event(40, 0, false, None, 2000);
    stats.record_event(41, 1, false, None, 3000);

    assert_eq!(stats.key_events_processed, 3);
    assert_eq!(stats.key_events_passed, 3);
    assert_eq!(stats.key_events_dropped, 0);

    // Use direct indexing
    let key_40 = &stats.per_key_stats[40];
    // Check counts directly, they should be 0 as no events were dropped for this key
    assert_eq!(key_40.press.count, 0);
    assert_eq!(key_40.release.count, 0);
}

// Additional test: ensure stats are correct for only dropped events
#[test]
fn test_stats_collector_only_dropped() {
    let mut stats = StatsCollector::new();
    stats.record_event(50, 1, true, Some(100), 1000);
    stats.record_event(50, 1, true, Some(200), 2000);

    assert_eq!(stats.key_events_processed, 2);
    assert_eq!(stats.key_events_passed, 0);
    assert_eq!(stats.key_events_dropped, 2);

    // Use direct indexing
    let key_50 = &stats.per_key_stats[50];
    assert_eq!(key_50.press.count, 2);
    assert_eq!(key_50.release.count, 0);
    assert_eq!(key_50.repeat.count, 0);
    assert_eq!(key_50.press.timings_us, vec![100, 200]);
}
