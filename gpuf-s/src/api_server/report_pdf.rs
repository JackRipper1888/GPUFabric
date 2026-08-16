use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{redirect::Policy, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{env, time::Duration};

const MAX_PDF_BYTES: usize = 32 << 20;
const MAX_RESPONSE_BYTES: u64 = 45 << 20;

#[derive(Debug)]
pub enum RenderError {
    Configuration,
    Unavailable,
    InvalidArtifact,
}

#[derive(Deserialize)]
struct RenderEnvelope {
    success: bool,
    data: RenderData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderData {
    pdf_base64: String,
    pdf_sha256: String,
}

pub async fn render(
    report_id: &str,
    html: &str,
    html_sha256: &str,
) -> Result<(Vec<u8>, String), RenderError> {
    let base_url = required_env("GPUF_REPORT_SUPPORT_URL")?;
    let token = required_env("GPUF_REPORT_SUPPORT_RENDERER_TOKEN")?;
    if token.len() < 32 {
        return Err(RenderError::Configuration);
    }
    let subject = env::var("GPUF_REPORT_SUPPORT_RENDERER_SUBJECT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "asset-assessment-service".to_string());
    if subject.len() > 128 || subject.bytes().any(|byte| byte <= 0x20 || byte >= 0x7f) {
        return Err(RenderError::Configuration);
    }

    let endpoint = renderer_endpoint(&base_url)?;
    let timeout = env::var("GPUF_REPORT_SUPPORT_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=120).contains(value))
        .unwrap_or(60);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .redirect(Policy::none())
        .build()
        .map_err(|_| RenderError::Configuration)?;
    let response = client
        .post(endpoint)
        .bearer_auth(token)
        .header("X-Service-Subject", subject)
        .header("Idempotency-Key", format!("pre-evaluation-pdf-{report_id}"))
        .json(&serde_json::json!({
            "reportId": report_id,
            "html": html,
            "htmlSha256": html_sha256,
        }))
        .send()
        .await
        .map_err(|_| RenderError::Unavailable)?;
    if !response.status().is_success() {
        return Err(RenderError::Unavailable);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(RenderError::InvalidArtifact);
    }
    let response_body = response
        .bytes()
        .await
        .map_err(|_| RenderError::Unavailable)?;
    if response_body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(RenderError::InvalidArtifact);
    }
    let envelope: RenderEnvelope =
        serde_json::from_slice(&response_body).map_err(|_| RenderError::InvalidArtifact)?;
    let declared_hash = envelope.data.pdf_sha256.to_ascii_lowercase();
    if !envelope.success
        || !valid_sha256(&declared_hash)
        || envelope.data.pdf_base64.len() > (MAX_PDF_BYTES * 4 / 3) + 16
    {
        return Err(RenderError::InvalidArtifact);
    }
    let pdf = STANDARD
        .decode(envelope.data.pdf_base64)
        .map_err(|_| RenderError::InvalidArtifact)?;
    let calculated_hash = format!("{:x}", Sha256::digest(&pdf));
    if pdf.len() < 8
        || pdf.len() > MAX_PDF_BYTES
        || !pdf.starts_with(b"%PDF-")
        || calculated_hash != declared_hash
    {
        return Err(RenderError::InvalidArtifact);
    }
    Ok((pdf, calculated_hash))
}

fn required_env(name: &str) -> Result<String, RenderError> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(RenderError::Configuration)
}

fn renderer_endpoint(base_url: &str) -> Result<Url, RenderError> {
    let mut url = Url::parse(base_url).map_err(|_| RenderError::Configuration)?;
    let secure = url.scheme() == "https";
    let loopback = url
        .host_str()
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if (!secure && !(url.scheme() == "http" && loopback))
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(RenderError::Configuration);
    }
    url.set_path("/internal/v1/pdf-renders");
    Ok(url)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{renderer_endpoint, valid_sha256};

    #[test]
    fn renderer_url_requires_tls_except_loopback() {
        assert!(renderer_endpoint("https://report-support.internal").is_ok());
        assert!(renderer_endpoint("http://127.0.0.1:28080").is_ok());
        assert!(renderer_endpoint("http://report-support.internal").is_err());
        assert!(renderer_endpoint("https://user:password@report-support.internal").is_err());
    }

    #[test]
    fn pdf_hash_is_lowercase_hex() {
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256("short"));
    }
}
