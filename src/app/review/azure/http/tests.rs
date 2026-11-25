use std::time::UNIX_EPOCH;

use serde::Deserialize;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

use super::*;

#[derive(Debug, Deserialize)]
struct Record {
    id: u32,
}

fn transport() -> Transport {
    Transport::new(&"sample".into()).unwrap()
}

#[tokio::test]
async fn retries_rate_limited_gets_and_reads_the_response() {
    let server = MockServer::start().await;
    Mock::given(path("/records"))
        .and(method("GET"))
        .and(header("Authorization", "Basic OnNhbXBsZQ=="))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .with_priority(1)
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/records"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(r#"{"id":42}"#, "application/json"))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    let endpoint = Url::parse(&format!("{}/records", server.uri())).unwrap();
    assert_eq!(transport().get::<Record>(&endpoint).unwrap().id, 42);
}

#[tokio::test]
async fn does_not_follow_redirects() {
    let server = MockServer::start().await;
    Mock::given(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/target"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/target"))
        .respond_with(ResponseTemplate::new(401))
        .expect(0)
        .mount(&server)
        .await;
    let endpoint = Url::parse(&format!("{}/redirect", server.uri())).unwrap();
    assert!(matches!(
        transport().get::<Record>(&endpoint),
        Err(AzureError::Status(302))
    ));
}

#[tokio::test]
async fn reports_only_status_for_http_failures() {
    let server = MockServer::start().await;
    Mock::given(path("/denied"))
        .respond_with(ResponseTemplate::new(403).set_body_string("private diagnostic"))
        .expect(1)
        .mount(&server)
        .await;
    let endpoint = Url::parse(&format!("{}/denied", server.uri())).unwrap();
    let error = transport().get::<Record>(&endpoint).unwrap_err();
    assert_eq!(error.to_string(), "Azure DevOps returned HTTP 403");
}

#[tokio::test]
async fn stops_retrying_when_the_server_remains_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/unavailable"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .expect(4)
        .mount(&server)
        .await;
    let endpoint = Url::parse(&format!("{}/unavailable", server.uri())).unwrap();
    assert!(matches!(
        transport().get::<Record>(&endpoint),
        Err(AzureError::Status(503))
    ));
}

#[test]
fn understands_retry_after_dates_and_seconds() {
    let date = "Thu, 01 Jan 1970 00:00:10 GMT";
    assert_eq!(parse_delay(date, UNIX_EPOCH), Some(Duration::from_secs(10)));
    assert_eq!(parse_delay("3", UNIX_EPOCH), Some(Duration::from_secs(3)));
    assert_eq!(parse_delay("soon", UNIX_EPOCH), None);
}
