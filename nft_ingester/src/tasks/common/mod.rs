pub mod price;
pub mod redis_task_messenger;

use super::{metric, BgTask, FromTaskData, IngesterError, IntoTaskData, TaskData};
use async_trait::async_trait;
use cadence_macros::{is_global_default_set, statsd_count};
use chrono::NaiveDateTime;
use digital_asset_types::dao::offchain_metadata;
use lazy_static::lazy_static;
use log::{debug, error, warn};
use regex::Regex;
use reqwest::{Client, ClientBuilder};
use sea_orm::{sea_query::OnConflict, *};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
    time::Duration,
};
use url::Url;

pub(crate) const TASK_NAME: &str = "DownloadMetadata";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetadata {
    pub asset_data_id: Vec<u8>,
    pub uri: String,
    #[serde(skip_serializing)]
    pub created_at: Option<NaiveDateTime>,
}

impl DownloadMetadata {
    pub fn sanitize(&mut self) {
        self.uri = self.uri.trim().replace('\0', "");
    }
}

impl IntoTaskData for DownloadMetadata {
    fn into_task_data(self) -> Result<TaskData, IngesterError> {
        let ts = self.created_at;
        let data =
            serde_json::to_value(self).map_err(<serde_json::Error as Into<IngesterError>>::into)?;
        Ok(TaskData {
            name: TASK_NAME,
            data,
            created_at: ts,
        })
    }
}

impl FromTaskData<DownloadMetadata> for DownloadMetadata {
    fn from_task_data(data: TaskData) -> Result<Self, IngesterError> {
        serde_json::from_value(data.data).map_err(|e| e.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetadataTask {
    pub lock_duration: Option<i64>,
    pub max_attempts: Option<i16>,
    pub timeout: Option<Duration>,
}

// Offchain jsons can use http/https or any subdomain.
// The capture group will provide us with the url path to append when we call the gateway.
//
// NOTE: This is not an exhaustive list. There are many permutations of IPFS and arweave calls that we could route to our gateway.
// We only included the most common ones.
// Other examples:
//    - https://bafybeie5ctfikksdc4gvopphghafyensonn6lwdahyqwozacnq5fvf3axi.ipfs.nftstorage.link/6598.json
//    - https://arweave.net:443/txHV2iTW5vChUmz8akM7WesGXJf1AYA4UGFeK7Duvn0/rock_1274.json
const IPFS_REGEX_STR: &str = r"https?://nftstorage\.link/(.*)";
const ARWEAVE_REGEX_STR: &str = r"https?://(?:[a-zA-Z0-9-]+\.)?arweave\.net/(.*)";
const PINATA_REGEX_STR: &str = r"https?://(?:[a-zA-Z0-9-]+\.)?pinata\.cloud/(.*)";
const CLOUDFLARE_IPFS_REGEX_STR: &str = r"https?://cloudflare-ipfs\.com/(.*)";
const CLOUDFLARE_IPFS_REGEX_STR_2: &str = r"https?://cf-ipfs\.com/(.*)";
const IPFS_GATEWAY_REGEX_STR: &str = r"^https?://[^/]+/(ipfs/.+)$";

// An arweave transaction id is a 32-byte content address rendered as 43 base64url
// characters. The bytes served for one are fixed for the life of the network, as is
// any manifest path or query string beneath it.
const IMMUTABLE_ARWEAVE_REGEX_STR: &str =
    r"^https?://(?:[a-zA-Z0-9-]+\.)?arweave\.net(?::\d+)?/[A-Za-z0-9_-]{43}(?:[/?#].*)?$";

// An IPFS CID is a hash of the content, so any gateway serving one must serve the same
// bytes. CIDv0 is 46 base58 characters starting `Qm`; CIDv1 is base32 starting `ba`.
// A path below a CID resolves inside that CID's DAG and is equally fixed.
// `/ipns/` is deliberately excluded: IPNS names are repointable.
const IMMUTABLE_IPFS_PATH_REGEX_STR: &str = concat!(
    r"^(?:ipfs://|https?://[^/]+/ipfs/)",
    r"(?:Qm[1-9A-HJ-NP-Za-km-z]{44}|ba[a-z2-7]{57,})",
    r"(?:[/?#].*)?$"
);
// Subdomain gateways put the CIDv1 in the host: https://<cid>.ipfs.<gateway>/0.json
const IMMUTABLE_IPFS_SUBDOMAIN_REGEX_STR: &str =
    r"^https?://ba[a-z2-7]{57,}\.ipfs\.[^/]+(?:[/?#].*)?$";
// A CID with an optional path and no scheme or gateway: `Qm.../0.json`.
const BARE_IPFS_CID_REGEX_STR: &str =
    r"^(?:Qm[1-9A-HJ-NP-Za-km-z]{44}|ba[a-z2-7]{57,})(?:/[^\s]*)?$";

async fn parse_json_response(
    response: reqwest::Response,
) -> Result<serde_json::Value, IngesterError> {
    // Check Content-Type before reading the body. If the server explicitly
    // returns a binary type (image, audio, video), this will never be valid
    // JSON metadata — fail permanently to avoid wasting retries and gateway costs.
    if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        if let Ok(ct) = content_type.to_str() {
            let ct_lower = ct.to_lowercase();
            if ct_lower.starts_with("image/")
                || ct_lower.starts_with("audio/")
                || ct_lower.starts_with("video/")
                || ct_lower.starts_with("application/octet-stream")
            {
                return Err(IngesterError::UnrecoverableTaskError(format!(
                    "Response Content-Type is not JSON: {}",
                    ct
                )));
            }
        }
    }

    let response_text = response.text().await?;

    match serde_json::from_str::<serde_json::Value>(&response_text) {
        Ok(val) => Ok(val),
        Err(e) => {
            // If the body is clearly not JSON (binary data, HTML, or empty),
            // fail permanently instead of retrying.
            let trimmed = response_text.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('<')
                || trimmed.as_bytes().first().map_or(false, |b| !b.is_ascii())
            {
                Err(IngesterError::UnrecoverableTaskError(format!(
                    "Response body is not JSON: {}",
                    &response_text[..response_text.len().min(100)]
                )))
            } else {
                Err(IngesterError::BatchInitNetworkingError(e.to_string()))
            }
        }
    }
}

impl DownloadMetadataTask {
    pub async fn request_metadata(
        uri: String,
        timeout: Duration,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
    ) -> Result<serde_json::Value, IngesterError> {
        let uri = uri.trim().to_string();
        let plain_client = ClientBuilder::new().timeout(timeout).build()?;

        // Build a separate client with the Pinata gateway token header for IPFS gateway requests.
        // This ensures the token is only sent to our dedicated Pinata gateway, not to arweave
        // or other third-party servers.
        let ipfs_client = if let Some(ref token) = ipfs_gateway_token {
            let mut headers = reqwest::header::HeaderMap::new();
            if let Ok(val) = reqwest::header::HeaderValue::from_str(token) {
                headers.insert("x-pinata-gateway-token", val);
            }
            ClientBuilder::new()
                .timeout(timeout)
                .default_headers(headers)
                .build()?
        } else {
            plain_client.clone()
        };

        lazy_static! {
            static ref IPFS_REGEX: Regex = Regex::new(IPFS_REGEX_STR).unwrap();
            static ref ARWEAVE_REGEX: Regex = Regex::new(ARWEAVE_REGEX_STR).unwrap();
            static ref PINATA_REGEX: Regex = Regex::new(PINATA_REGEX_STR).unwrap();
            static ref CLOUDFLARE_IPFS_REGEX: Regex =
                Regex::new(CLOUDFLARE_IPFS_REGEX_STR).unwrap();
            static ref CLOUDFLARE_IPFS_REGEX_2: Regex =
                Regex::new(CLOUDFLARE_IPFS_REGEX_STR_2).unwrap();
            static ref IPFS_GATEWAY_REGEX: Regex = Regex::new(IPFS_GATEWAY_REGEX_STR).unwrap();
        }

        // Handle raw ipfs:// scheme URIs by rewriting to the IPFS gateway.
        // These URIs can't be fetched directly by HTTP clients.
        if uri.starts_with("ipfs://") {
            if let Some(ref g) = ipfs_gateway {
                let cid_path = uri.trim_start_matches("ipfs://");
                let new_uri = format!("{}/ipfs/{}", g, cid_path);
                let response = Client::get(&ipfs_client, new_uri.clone()).send().await;

                if let Err(ref e) = response {
                    if e.is_timeout() {
                        warn!(
                            "URI {} timed out with custom gateway for raw ipfs:// rewrite.",
                            new_uri,
                        );
                        return Err(IngesterError::BatchInitNetworkingError(format!(
                            "Timeout fetching raw ipfs:// URI via gateway: {}",
                            new_uri
                        )));
                    }
                }

                let response = response?;
                match response.status() {
                    reqwest::StatusCode::OK => {
                        let val = parse_json_response(response).await?;
                        metric! {
                            statsd_count!("ingester.bgtask.gateway", 1, "found" => "true", "gateway" => "ipfs_scheme");
                        }
                        return Ok(val);
                    }
                    status => {
                        warn!(
                            "URI {} failed with gateway ({}) for raw ipfs:// rewrite.",
                            new_uri, status,
                        );
                        return Err(IngesterError::HttpError {
                            status_code: status.as_str().to_string(),
                            uri: new_uri,
                        });
                    }
                }
            }
            // No gateway configured — raw ipfs:// can't be fetched
            return Err(IngesterError::UnrecoverableTaskError(format!(
                "URL scheme is not allowed. Cannot fetch raw ipfs:// URI without a gateway: {}",
                uri
            )));
        }

        // Try to use a custom gateway first.
        // Use ipfs_client (with Pinata token) for IPFS gateways, plain_client for arweave.
        let ipfs_gw = ipfs_gateway.clone();
        let mut gateway_attempt: Option<(String, String)> = None;
        for (gateway, regex, client, use_raw) in [
            (ipfs_gw.clone(), &*IPFS_REGEX, &ipfs_client, false),
            (ipfs_gw.clone(), &*PINATA_REGEX, &ipfs_client, false),
            (
                ipfs_gw.clone(),
                &*CLOUDFLARE_IPFS_REGEX,
                &ipfs_client,
                false,
            ),
            (
                ipfs_gw.clone(),
                &*CLOUDFLARE_IPFS_REGEX_2,
                &ipfs_client,
                false,
            ),
            (ipfs_gw, &*IPFS_GATEWAY_REGEX, &ipfs_client, false),
            (arweave_gateway, &*ARWEAVE_REGEX, &plain_client, true),
        ] {
            if let Some(g) = gateway {
                if let Some(captures) = regex.captures(&uri) {
                    let new_uri = if use_raw {
                        format!("{}/raw/{}", g, &captures[1])
                    } else {
                        format!("{}/{}", g, &captures[1])
                    };
                    let response = Client::get(client, new_uri.clone()).send().await;

                    if let Err(ref e) = response {
                        if e.is_timeout() {
                            if use_raw {
                                error!("Gateway timeout: {} (original: {})", new_uri, uri);
                            } else {
                                warn!("Gateway timeout: {} (original: {})", new_uri, uri);
                            }
                            gateway_attempt = Some((new_uri, "timeout".to_string()));
                            continue;
                        }
                    }

                    let response = response?;

                    match response.status() {
                        reqwest::StatusCode::OK => {
                            let val = parse_json_response(response).await?;
                            metric! {
                                statsd_count!("ingester.bgtask.gateway", 1, "found" => "true", "gateway" => g.as_str());
                            }
                            return Ok(val);
                        }
                        _ => {
                            let status = response.status();
                            if use_raw {
                                error!(
                                    "Gateway failed: {} returned {} (original: {})",
                                    new_uri, status, uri
                                );
                            } else {
                                warn!(
                                    "Gateway failed: {} returned {} (original: {})",
                                    new_uri, status, uri
                                );
                            }
                            metric! {
                                statsd_count!("ingester.bgtask.gateway", 1, "found" => "false", "gateway" => g.as_str(), "status" => status.as_str());
                            }
                            gateway_attempt = Some((new_uri, status.to_string()));
                        }
                    }
                }
            }
        }

        // If the URI does not match a custom gateway, or the request was not found, try again with the regular URI.
        let response = Client::get(&plain_client, uri.clone()).send().await?;
        if response.status() != reqwest::StatusCode::OK {
            let status_code = response.status().as_str().to_string();
            if let Some((gw_uri, gw_status)) = gateway_attempt {
                error!(
                    "Fallback also failed: {} returned {} (gateway {} returned {})",
                    uri, status_code, gw_uri, gw_status,
                );
            }
            Err(IngesterError::HttpError { status_code, uri })
        } else {
            parse_json_response(response).await
        }
    }
}

#[async_trait]
impl BgTask for DownloadMetadataTask {
    fn name(&self) -> &'static str {
        TASK_NAME
    }

    fn lock_duration(&self) -> i64 {
        self.lock_duration.unwrap_or(5)
    }

    fn max_attempts(&self) -> i16 {
        self.max_attempts.unwrap_or(3)
    }

    async fn task(
        &self,
        db: &DatabaseConnection,
        data: serde_json::Value,
        reindex_interval: i64,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
    ) -> Result<(), IngesterError> {
        let download_metadata: DownloadMetadata = serde_json::from_value(data)?;
        let offchain_model = offchain_metadata::Entity::find()
            .filter(offchain_metadata::Column::MetadataUrl.eq(download_metadata.uri.clone()))
            .one(db)
            .await
            .map_err(|db| {
                IngesterError::TaskManagerError(format!(
                    "Database error with {}. Could not read from offchain_metadata. Error: {}",
                    self.name(),
                    db
                ))
            })?;

        if !should_reindex(offchain_model, reindex_interval) {
            debug!(
                "skip download metadata for {:?}",
                bs58::encode(download_metadata.asset_data_id.clone()).into_string()
            );
            return Ok(());
        }
        debug!(
            "download metadata for {:?}",
            bs58::encode(download_metadata.asset_data_id.clone()).into_string()
        );

        let fetch_uri = normalize_metadata_uri(&download_metadata.uri);
        let meta_url = Url::parse(&fetch_uri);
        let body = match meta_url {
            Ok(_) => DownloadMetadataTask::request_metadata(
                fetch_uri.into_owned(),
                self.timeout.unwrap_or(Duration::from_millis(3000)),
                ipfs_gateway,
                ipfs_gateway_token,
                arweave_gateway,
            )
            .await
            .map_err(|e| {
                let asset = bs58::encode(download_metadata.asset_data_id.clone()).into_string();
                let uri = download_metadata.uri.clone();
                log::error!(
                    "Failed to download metadata for {:?}, uri: {:?}: {}",
                    asset,
                    uri,
                    e
                );
                e
            })?,
            _ => serde_json::Value::String("Invalid Uri".to_string()), //TODO -> enumize this.
        };

        let offchain_metadata_model = offchain_metadata::ActiveModel {
            metadata_url: Set(download_metadata.uri.clone()),
            metadata: Set(body),
            reindex: Set(false),
            updated_at: Set(Some(chrono::Utc::now().fixed_offset())),
            ..Default::default()
        };
        let query = offchain_metadata::Entity::insert(offchain_metadata_model)
            .on_conflict(
                OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                    .update_columns([
                        offchain_metadata::Column::Metadata,
                        offchain_metadata::Column::Reindex,
                        offchain_metadata::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .build(DbBackend::Postgres);
        db.execute(query).await.map_err(|db| {
            IngesterError::TaskManagerError(format!(
                "Database error with {}. Could not write to offchain_metadata table. Error: {}",
                self.name(),
                db
            ))
        })?;

        if meta_url.is_err() {
            return Err(IngesterError::UnrecoverableTaskError(format!(
                "Failed to parse URI: {}",
                download_metadata.uri
            )));
        }

        Ok(())
    }
}

impl Display for DownloadMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DownloadMetadata from {} for {:?}",
            self.uri, self.asset_data_id
        )
    }
}

/// Rewrites a scheme-less IPFS CID to an `ipfs://` URI. Every other URI is returned as is.
pub(crate) fn normalize_metadata_uri(uri: &str) -> Cow<'_, str> {
    lazy_static! {
        static ref BARE_IPFS_CID_REGEX: Regex = Regex::new(BARE_IPFS_CID_REGEX_STR).unwrap();
    }
    let uri = uri.trim();
    if BARE_IPFS_CID_REGEX.is_match(uri) {
        Cow::Owned(format!("ipfs://{}", uri))
    } else {
        Cow::Borrowed(uri)
    }
}

/// True when the URI is a content address, so the document behind it can never change.
pub(crate) fn is_content_addressed(uri: &str) -> bool {
    lazy_static! {
        static ref IMMUTABLE_ARWEAVE_REGEX: Regex =
            Regex::new(IMMUTABLE_ARWEAVE_REGEX_STR).unwrap();
        static ref IMMUTABLE_IPFS_PATH_REGEX: Regex =
            Regex::new(IMMUTABLE_IPFS_PATH_REGEX_STR).unwrap();
        static ref IMMUTABLE_IPFS_SUBDOMAIN_REGEX: Regex =
            Regex::new(IMMUTABLE_IPFS_SUBDOMAIN_REGEX_STR).unwrap();
    }
    let uri = normalize_metadata_uri(uri);
    IMMUTABLE_ARWEAVE_REGEX.is_match(&uri)
        || IMMUTABLE_IPFS_PATH_REGEX.is_match(&uri)
        || IMMUTABLE_IPFS_SUBDOMAIN_REGEX.is_match(&uri)
}

/// True when `metadata` holds a fetched document rather than a placeholder.
/// The placeholders are the `processing` sentinel written at mint, the
/// `Invalid Uri` string, and the permanent-failure record.
pub(crate) fn holds_fetched_document(metadata: &serde_json::Value) -> bool {
    match metadata.as_object() {
        Some(map) => map.get("error").and_then(|e| e.as_str()) != Some("permanent_failure"),
        None => false,
    }
}

/// How long a permanent-failure record suppresses refetching. One probe per URI
/// per horizon replaces one probe per touch, and a misclassification heals at
/// the next probe.
pub(crate) const PERMANENT_FAILURE_RETRY_HOURS: i64 = 24;

/// True when `metadata` is a permanent-failure record.
pub(crate) fn is_permanent_failure(metadata: &serde_json::Value) -> bool {
    metadata
        .as_object()
        .and_then(|map| map.get("error"))
        .and_then(|e| e.as_str())
        == Some("permanent_failure")
}

/// HTTP statuses that mean the document is gone at this URI, as opposed to rate
/// limiting (429), bot blocking (403), or upstream faults (5xx).
///
/// 404 is excluded. A content gateway answers 404 when it has not pinned the
/// content, not when the content is gone, so the same URI often resolves from
/// another gateway or on a later attempt. Treating that as permanent would blank
/// an asset's metadata for a full horizon on the word of one gateway.
pub(crate) fn is_permanent_http_status(status_code: &str) -> bool {
    matches!(status_code, "402" | "410" | "451")
}

/// True while a permanent-failure record is younger than the retry horizon.
pub(crate) fn permanent_failure_is_fresh(model: &offchain_metadata::Model) -> bool {
    if !is_permanent_failure(&model.metadata) {
        return false;
    }
    match model.updated_at {
        Some(updated_at) => {
            let age = chrono::Utc::now().naive_utc() - updated_at.naive_utc();
            age < chrono::Duration::hours(PERMANENT_FAILURE_RETRY_HOURS)
        }
        None => false,
    }
}

fn should_reindex(offchain_model: Option<offchain_metadata::Model>, reindex_interval: i64) -> bool {
    match offchain_model {
        None => true,
        Some(model) => {
            if model.reindex {
                return true;
            }

            // A URI that most recently answered with a gone/unpurchasable status is
            // probed once per horizon instead of once per touch.
            if permanent_failure_is_fresh(&model) {
                metric! {
                    statsd_count!("ingester.bgtask.permafail_skip", 1);
                }
                return false;
            }

            // A content address that already yielded its document has nothing left to
            // re-read. Editing an NFT repoints its on-chain uri at a new id, which
            // reaches us as a separate row and is fetched on its own.
            if model.updated_at.is_some()
                && is_content_addressed(&model.metadata_url)
                && holds_fetched_document(&model.metadata)
            {
                metric! {
                    statsd_count!("ingester.bgtask.immutable_skip", 1);
                }
                return false;
            }
            // If the model has been updated within the "grace period", skip reindexing.
            if let Some(updated_at) = model.updated_at {
                let updated_at = updated_at.naive_utc();
                let now = chrono::Utc::now().naive_utc();
                let duration = chrono::Duration::minutes(reindex_interval);
                let within_grace_period = updated_at > now - duration;

                if within_grace_period {
                    return false;
                }
                metric! {
                    statsd_count!("ingester.bgtask.duplicate_processing", 1);
                }
                return true;
            }
            true
        }
    }
}

// Test ipfs gateway regex
// Add a test function

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipfs_gateway_regex() {
        let replacement_url = "https://test.infura-ipfs.io";
        let regex = Regex::new(IPFS_GATEWAY_REGEX_STR).unwrap();
        let test_url = "https://gateway.pinit.io/ipfs/Qmcm5RD1AqFinZcgZRP8ecZTj7FBnp8oUpKDc1A4dejfGw/2150.json";
        let captures = regex.captures(test_url).unwrap();
        let new_url = format!("{}/{}", replacement_url, &captures[1]);
        assert_eq!(new_url, "https://test.infura-ipfs.io/ipfs/Qmcm5RD1AqFinZcgZRP8ecZTj7FBnp8oUpKDc1A4dejfGw/2150.json");
    }

    #[test]
    fn bare_cid_is_rewritten_to_ipfs_scheme() {
        assert_eq!(
            normalize_metadata_uri("QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk"),
            "ipfs://QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk"
        );
        assert_eq!(
            normalize_metadata_uri("  QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk/0.json "),
            "ipfs://QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk/0.json"
        );
        assert_eq!(
            normalize_metadata_uri(
                "bafybeie5ctfikksdc4gvopphghafyensonn6lwdahyqwozacnq5fvf3axi/6598.json"
            ),
            "ipfs://bafybeie5ctfikksdc4gvopphghafyensonn6lwdahyqwozacnq5fvf3axi/6598.json"
        );
        assert!(is_content_addressed(
            "QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk"
        ));
    }

    #[test]
    fn non_cid_uris_are_left_unchanged() {
        for uri in [
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            "ipfs://QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk",
            "https://ipfs.io/ipfs/QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkAk",
            // 45 characters, one short of a CIDv0.
            "QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdkA",
            // Base58 excludes 0, O, I and l.
            "QmUzGdu1Zbm3rbbLhh1cJowxq23WAxqd8CwjubLzVPdk0O",
            "./metadata.json",
            "/api/jsonBlob/019b4e87-9c73-798b-8229-60923d3ea092",
            "",
        ] {
            assert_eq!(normalize_metadata_uri(uri), uri.trim(), "changed: {}", uri);
        }
    }

    #[test]
    fn content_addressed_accepts_arweave_transaction_ids() {
        for uri in [
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            "http://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            "https://www.arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY?ext=json",
            "https://arweave.net:443/txHV2iTW5vChUmz8akM7WesGXJf1AYA4UGFeK7Duvn0/rock_1274.json",
            "  https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY  ",
        ] {
            assert!(is_content_addressed(uri), "expected immutable: {}", uri);
        }
    }

    #[test]
    fn content_addressed_rejects_malformed_and_mutable_urls() {
        for uri in [
            // Malformed ids observed in production rows.
            "https://arweave.net/\"AC-3jbsDLNn5Y9BgyJsYv1LKyQv_NGJ8UKmai-qaVJM",
            "https://arweave.net/#NAME?",
            "https://arweave.net/$6tJx9B9DDJ8WpSreD6VJ2Esm26y9_yxX2dku1HgoJPU",
            // 42 characters, one short of a transaction id.
            "https://arweave.net/-jNr8TPmaVKthensa0sOlwZsxzbJDD7H3zXQlB5kCC",
            // Hosts that can serve different bytes at a fixed path.
            "https://storage.googleapis.com/fractal-launchpad-public/1.json",
            "https://api.stepn.com/run/nftjson/103/1",
            "https://nftstorage.link/ipfs/bafybeib3wg/157.json",
            // Host that merely starts with the gateway name.
            "https://arweave.net.example.com/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            // Transaction id embedded in someone else's query string.
            "https://example.com/x?u=https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            // 44 characters, one over a transaction id.
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pYX",
        ] {
            assert!(!is_content_addressed(uri), "expected mutable: {}", uri);
        }
    }

    #[test]
    fn content_addressed_accepts_ipfs_cids() {
        for uri in [
            // Real shapes taken from production task rows.
            "https://ipfs.io/ipfs/QmPxpwQPq9GhzWUnGGfUXf3HPZZ7bHgyQDAfniFKYK5i8u",
            "https://nftstorage.link/ipfs/bafkreibuumx4y6ag3df5ztgizddvy4nlbmargm24hcmuirvl3ob4t62ycq",
            "https://nftstorage.link/ipfs/bafybeihwsoh2tk3zqhci7hbvk7vz5muwi7ufdt6nr7ey3m62hdidk6fc74/51.json",
            "https://gateway.pinit.io/ipfs/QmSvJoWqtfaH8q6bRwLdm1tgtGbWKji8UgsRcL67p5d9zL/596.json",
            "https://bafybeieaxznycgxwu3vj2zfb2jf3ka5vyhf3lzzf6ttinbsndfhgvrcyia.ipfs.nftstorage.link/0.json",
            "ipfs://QmPxpwQPq9GhzWUnGGfUXf3HPZZ7bHgyQDAfniFKYK5i8u",
            "ipfs://bafybeidc5ovozegzithm6ab35zkeiq3gomc6l4bw6oik35uxr5hykpkfa4/1578.json",
        ] {
            assert!(is_content_addressed(uri), "expected immutable: {}", uri);
        }
    }

    #[test]
    fn content_addressed_rejects_ipns_and_malformed_cids() {
        for uri in [
            // IPNS names are repointable, so they must keep refreshing.
            "https://ipfs.io/ipns/k51qzi5uqu5dkkciu33khkzbcmxtyhn376i1e83tya8kuy7z9euedzyr5nhoew",
            "https://ipfs.io/ipns/example.com/metadata.json",
            // Truncated CIDv0, 45 characters rather than 46.
            "https://ipfs.io/ipfs/QmPxpwQPq9GhzWUnGGfUXf3HPZZ7bHgyQDAfniFKYK5i8",
            // Not a CID at all.
            "https://ipfs.io/ipfs/metadata.json",
            // CID-looking segment on a host that is not a gateway path.
            "https://example.com/QmPxpwQPq9GhzWUnGGfUXf3HPZZ7bHgyQDAfniFKYK5i8u",
        ] {
            assert!(!is_content_addressed(uri), "expected mutable: {}", uri);
        }
    }

    #[test]
    fn skips_immutable_ipfs_uri_we_already_fetched() {
        let m = model(
            "https://ipfs.io/ipfs/QmPxpwQPq9GhzWUnGGfUXf3HPZZ7bHgyQDAfniFKYK5i8u",
            serde_json::json!({"name": "Some NFT"}),
            false,
        );
        assert!(!should_reindex(Some(m), 60));
    }

    #[test]
    fn still_fetches_ipns_uri() {
        let m = model(
            "https://ipfs.io/ipns/example.com/metadata.json",
            serde_json::json!({"name": "Some NFT"}),
            false,
        );
        assert!(should_reindex(Some(m), 60));
    }

    #[test]
    fn permanent_http_statuses_are_gone_or_unpurchasable_only() {
        for code in ["402", "410", "451"] {
            assert!(
                is_permanent_http_status(code),
                "expected permanent: {}",
                code
            );
        }
        // 404 is retryable on purpose: a gateway miss is not proof the document is gone.
        for code in ["401", "403", "404", "429", "500", "502", "530"] {
            assert!(
                !is_permanent_http_status(code),
                "expected retryable: {}",
                code
            );
        }
    }

    fn permafail_model(updated_hours_ago: i64, reindex: bool) -> offchain_metadata::Model {
        let mut m = model(
            "https://api.stepn.com/run/nftjson/103/1",
            serde_json::json!({"error": "permanent_failure", "msg": "HttpError 410", "code": "410"}),
            reindex,
        );
        m.updated_at =
            Some((chrono::Utc::now() - chrono::Duration::hours(updated_hours_ago)).into());
        m
    }

    #[test]
    fn fresh_permanent_failure_suppresses_reindex() {
        assert!(!should_reindex(Some(permafail_model(1, false)), 60));
    }

    #[test]
    fn stale_permanent_failure_probes_again() {
        assert!(should_reindex(
            Some(permafail_model(PERMANENT_FAILURE_RETRY_HOURS + 1, false)),
            60
        ));
    }

    #[test]
    fn reindex_flag_overrides_permanent_failure() {
        assert!(should_reindex(Some(permafail_model(1, true)), 60));
    }

    #[test]
    fn fetched_document_distinguishes_placeholders() {
        assert!(holds_fetched_document(
            &serde_json::json!({"name": "Some NFT"})
        ));
        assert!(!holds_fetched_document(&serde_json::json!("processing")));
        assert!(!holds_fetched_document(&serde_json::json!("Invalid Uri")));
        assert!(!holds_fetched_document(
            &serde_json::json!({"error": "permanent_failure", "msg": "bad uri"})
        ));
    }

    fn model(url: &str, metadata: serde_json::Value, reindex: bool) -> offchain_metadata::Model {
        let long_ago = chrono::Utc::now() - chrono::Duration::days(400);
        offchain_metadata::Model {
            id: 1,
            metadata_url: url.to_string(),
            mutability: digital_asset_types::dao::sea_orm_active_enums::Mutability::Mutable,
            metadata,
            created_at: long_ago.into(),
            updated_at: Some(long_ago.into()),
            reindex,
        }
    }

    #[test]
    fn skips_immutable_uri_we_already_fetched() {
        let m = model(
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            serde_json::json!({"name": "Some NFT"}),
            false,
        );
        assert!(!should_reindex(Some(m), 60));
    }

    #[test]
    fn reindex_flag_overrides_immutability() {
        let m = model(
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            serde_json::json!({"name": "Some NFT"}),
            true,
        );
        assert!(should_reindex(Some(m), 60));
    }

    #[test]
    fn still_retries_immutable_uri_we_never_fetched() {
        let m = model(
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            serde_json::json!("processing"),
            false,
        );
        assert!(should_reindex(Some(m), 60));
    }

    #[test]
    fn still_fetches_immutable_uri_with_no_updated_at() {
        let mut m = model(
            "https://arweave.net/OIX7PqIWsN4JIx5TLgRt4yF7DvUQOX-gtYDmIrjR-pY",
            serde_json::json!({"name": "Some NFT"}),
            false,
        );
        m.updated_at = None;
        assert!(should_reindex(Some(m), 60));
    }

    #[test]
    fn mutable_host_still_refreshes_when_stale() {
        let m = model(
            "https://storage.googleapis.com/fractal-launchpad-public/1.json",
            serde_json::json!({"name": "Some NFT"}),
            false,
        );
        assert!(should_reindex(Some(m), 60));
    }

    #[tokio::test]
    #[ignore]
    async fn retries_when_timeout() {
        let result = DownloadMetadataTask::request_metadata(
            "https://gateway.pinit.io/ipfs/QmXuCg2Q4d3fSBBpmuQpRvev2dN5G6EPLnW911y6iWtqQj/0"
                .to_string(),
            Duration::from_secs(10),
            Some("https://amethyst-realistic-herring-771.mypinata.cloud".to_string()),
            None,
            Some("https://gateway.bundlr.network".to_string()),
        )
        .await;

        assert!(result.is_ok());
    }
}
