#[cfg(test)]
mod tests;

use std::{
    thread,
    time::{Duration, SystemTime},
};

use ::http::StatusCode;
use curl::easy::{Auth, Easy, List};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::AzureError;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: usize = 3;
const MAX_RESPONSE_BYTES: usize = 10_000_000;
pub(super) const OCTET_STREAM: &str = "application/octet-stream";

pub(super) struct Transport(Easy);

struct Response {
    status: StatusCode,
    body: Vec<u8>,
    retry_after: Option<String>,
}

impl Transport {
    pub(super) fn new(token: &SecretString) -> Result<Self, AzureError> {
        let mut curl = Easy::new();
        configure(&mut curl, token).map_err(|_| AzureError::Transport)?;
        Ok(Self(curl))
    }

    pub(super) fn get<T: serde::de::DeserializeOwned>(
        &mut self,
        endpoint: &Url,
    ) -> Result<T, AzureError> {
        let body = self.get_bytes(endpoint, "application/json")?;
        serde_json::from_slice(&body).map_err(|_| AzureError::Response)
    }

    pub(super) fn get_bytes(
        &mut self,
        endpoint: &Url,
        accept: &str,
    ) -> Result<Vec<u8>, AzureError> {
        self.0.get(true).map_err(|_| AzureError::Transport)?;
        self.0
            .custom_request("GET")
            .map_err(|_| AzureError::Transport)?;
        set_headers(&mut self.0, accept, false).map_err(|_| AzureError::Transport)?;
        let mut retries = 0;
        loop {
            let response = self.request(endpoint)?;
            if response.status.is_success() {
                return Ok(response.body);
            }
            if retries == MAX_RETRIES || !retryable(response.status) {
                return Err(AzureError::Status(response.status.as_u16()));
            }
            thread::sleep(retry_delay(response.retry_after.as_deref()));
            retries += 1;
        }
    }

    pub(super) fn write<T: serde::de::DeserializeOwned>(
        &mut self,
        endpoint: &Url,
        (method, body): (&str, &serde_json::Value),
    ) -> Result<T, AzureError> {
        configure_write(&mut self.0, method, body)?;
        let response = self.request(endpoint)?;
        if !response.status.is_success() {
            return Err(AzureError::Status(response.status.as_u16()));
        }
        serde_json::from_slice(&response.body).map_err(|_| AzureError::Response)
    }

    fn request(&mut self, endpoint: &Url) -> Result<Response, AzureError> {
        self.0
            .url(endpoint.as_str())
            .map_err(|_| AzureError::Endpoint)?;
        let mut body = Vec::new();
        let mut retry_after = None;
        let result = read_response(&mut self.0, &mut body, &mut retry_after);
        let code = self.0.response_code().map_err(|_| AzureError::Transport)?;
        let status = StatusCode::from_u16(u16::try_from(code).map_err(|_| AzureError::Transport)?)
            .map_err(|_| AzureError::Transport)?;
        if status.is_success() {
            result.map_err(|_| AzureError::Transport)?;
        }
        Ok(Response {
            status,
            body,
            retry_after,
        })
    }
}

fn configure_write(
    curl: &mut Easy,
    method: &str,
    body: &serde_json::Value,
) -> Result<(), AzureError> {
    let bytes = serde_json::to_vec(body).map_err(|_| AzureError::Response)?;
    curl.post_fields_copy(&bytes)
        .map_err(|_| AzureError::Transport)?;
    curl.custom_request(method)
        .map_err(|_| AzureError::Transport)?;
    set_headers(curl, "application/json", true).map_err(|_| AzureError::Transport)
}

fn set_headers(curl: &mut Easy, accept: &str, json: bool) -> Result<(), curl::Error> {
    let mut headers = List::new();
    headers.append(&format!("Accept: {accept}"))?;
    if json {
        headers.append("Content-Type: application/json")?;
    }
    curl.http_headers(headers)
}

fn configure(curl: &mut Easy, token: &SecretString) -> Result<(), curl::Error> {
    curl.timeout(TIMEOUT)?;
    curl.get(true)?;
    curl.follow_location(false)?;
    curl.ssl_verify_peer(true)?;
    curl.ssl_verify_host(true)?;
    if let Some(path) = std::env::var_os("SSL_CERT_FILE").filter(|path| !path.is_empty()) {
        curl.cainfo(path)?;
    }
    curl.username("")?;
    curl.password(token.expose_secret())?;
    curl.http_auth(Auth::new().basic(true))
}

fn read_response(
    curl: &mut Easy,
    body: &mut Vec<u8>,
    retry_after: &mut Option<String>,
) -> Result<(), curl::Error> {
    let mut transfer = curl.transfer();
    transfer.write_function(|chunk| {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Ok(0);
        }
        body.extend_from_slice(chunk);
        Ok(chunk.len())
    })?;
    transfer.header_function(|bytes| {
        capture_retry_after(bytes, retry_after);
        true
    })?;
    transfer.perform()
}

fn capture_retry_after(bytes: &[u8], retry_after: &mut Option<String>) {
    if bytes.starts_with(b"HTTP/") {
        *retry_after = None;
        return;
    }
    if let Some((name, value)) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|line| line.split_once(':'))
        && name.eq_ignore_ascii_case("Retry-After")
    {
        *retry_after = Some(value.trim().to_owned());
    }
}

fn retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| parse_delay(value, SystemTime::now()))
        .unwrap_or_else(|| Duration::from_secs(1))
}

fn parse_delay(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.parse() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(date.duration_since(now).unwrap_or_default())
}
