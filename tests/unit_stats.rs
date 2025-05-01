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

    let key_a = stats.per_key_stats.get(&30).unwrap();
    assert_eq!(key_a.press.count, 1);
    assert_eq!(key_a.release.count, 1);
    assert_eq!(key_a.repeat.count, 0);

    let key_b = stats.per_key_stats.get(&31);
    assert!(key_b.is_none() || key_b.unwrap().press.count == 0);
}

#[test]
fn test_stats_collector_near_miss() {
    let mut stats = StatsCollector::new();
    stats.record_near_miss((30, 1), 900);
    stats.record_near_miss((30, 1), 800);
    stats.record_near_miss((31, 0), 100);

    assert_eq!(stats.per_key_passed_near_miss_timing.get(&(30, 1)).unwrap().len(), 2);
    assert_eq!(stats.per_key_passed_near_miss_timing.get(&(31, 0)).unwrap().len(), 1);
}

#[test]
fn test_stats_collector_json_output() {
    let mut stats = StatsCollector::new();
    stats.record_event(30, 1, false, None, 1000);
    stats.record_event(30, 1, true, Some(500), 2000);
    stats.record_event(30, 0, true, Some(200), 3000);
    stats.record_event(31, 1, false, None, 4000);

    let mut buf = Vec::new();
    stats.print_stats_json(10_000, true, false, 0, &mut buf);
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

    let key_40 = stats.per_key_stats.get(&40);
    assert!(key_40.is_none() || (key_40.unwrap().press.count == 0 && key_40.unwrap().release.count == 0));
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

    let key_50 = stats.per_key_stats.get(&50).unwrap();
    assert_eq!(key_50.press.count, 2);
    assert_eq!(key_50.release.count, 0);
    assert_eq!(key_50.repeat.count, 0);
    assert_eq!(key_50.press.timings_us, vec![100, 200]);
}
