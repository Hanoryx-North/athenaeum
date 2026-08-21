use std::collections::BTreeMap;
use std::path::PathBuf;

use athenaeum_core::wiki::{WikiRequestEnvelope, WikiResponse, WikiResponseEnvelope, WikiService};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/wiki-protocol/v1")
        .join(name)
}

fn assert_lossless<T>(name: &str)
where
    T: DeserializeOwned + Serialize,
{
    let bytes = std::fs::read(fixture(name)).unwrap();
    let fixtures: BTreeMap<String, Value> = serde_json::from_slice(&bytes).unwrap();
    assert!(!fixtures.is_empty(), "{name} contains no fixtures");
    for (fixture_name, value) in fixtures {
        let typed: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("{fixture_name}: {error}"));
        let round_trip = serde_json::to_value(typed).unwrap();
        assert_eq!(round_trip, value, "{fixture_name} is not canonical");
    }
}

fn values(name: &str) -> BTreeMap<String, Value> {
    let bytes = std::fs::read(fixture(name)).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn every_request_fixture_round_trips_through_protocol_v1() {
    assert_lossless::<WikiRequestEnvelope>("requests.json");
}

#[test]
fn every_response_fixture_round_trips_through_protocol_v1() {
    assert_lossless::<WikiResponseEnvelope>("responses.json");
}

#[test]
fn fixture_operations_exactly_cover_advertised_protocol_operations() {
    let requests = values("requests.json");
    let mut fixture_methods = requests
        .values()
        .map(|value| value["request"]["method"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let mut advertised = WikiService::capabilities().operations;
    fixture_methods.sort();
    advertised.sort();
    assert_eq!(fixture_methods, advertised);
}

#[test]
fn capabilities_fixture_is_the_actual_advertised_response() {
    let expected = values("responses.json").remove("capabilities_ok").unwrap();
    let actual = serde_json::to_value(WikiResponseEnvelope::success(
        Some("capabilities-1".to_string()),
        WikiResponse::Capabilities {
            capabilities: WikiService::capabilities(),
        },
    ))
    .unwrap();
    assert_eq!(actual, expected);
}
