//! Wire-format checks for the draft profile. JSON Schema validation runs separately.

use spanscope::profile::Profile;

#[test]
fn fixtures_round_trip_without_losing_fields_or_integer_precision() {
    for fixture in [
        include_str!("fixtures/empty.json"),
        include_str!("fixtures/nested.json"),
        include_str!("fixtures/async-partial.json"),
        include_str!("fixtures/large-integer.json"),
    ] {
        let original: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let profile: Profile = serde_json::from_str(fixture).unwrap();
        assert_eq!(profile.schema_version, spanscope::SCHEMA_VERSION);
        assert_eq!(serde_json::to_value(&profile).unwrap(), original);
        let encoded = serde_json::to_vec(&profile).unwrap();
        assert_eq!(
            serde_json::from_slice::<Profile>(&encoded).unwrap(),
            profile
        );
    }
}

#[test]
fn counters_above_javascript_safe_integer_range_remain_exact() {
    let profile: Profile =
        serde_json::from_str(include_str!("fixtures/large-integer.json")).unwrap();
    assert_eq!(profile.meta.duration_ns, 9_007_199_254_740_993);
}

#[test]
fn serialization_does_not_require_collection() {
    // This test is also run with only `serialization`, outside workspace feature unification.
    let _: Profile = serde_json::from_str(include_str!("fixtures/empty.json")).unwrap();
}
