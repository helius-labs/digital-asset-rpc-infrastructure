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
    fmt::{Display, Formatter},
    time::Duration,
};
use url::Url;

const TASK_NAME: &str = "DownloadMetadata";

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
            (ipfs_gw.clone(), &*CLOUDFLARE_IPFS_REGEX, &ipfs_client, false),
            (ipfs_gw.clone(), &*CLOUDFLARE_IPFS_REGEX_2, &ipfs_client, false),
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
                                error!("Gateway failed: {} returned {} (original: {})", new_uri, status, uri);
                            } else {
                                warn!("Gateway failed: {} returned {} (original: {})", new_uri, status, uri);
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
            Err(IngesterError::HttpError {
                status_code,
                uri,
            })
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

        let meta_url = Url::parse(&download_metadata.uri);
        let body = match meta_url {
            Ok(_) => DownloadMetadataTask::request_metadata(
                download_metadata.uri.clone(),
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

fn should_reindex(offchain_model: Option<offchain_metadata::Model>, reindex_interval: i64) -> bool {
    match offchain_model {
        None => true,
        Some(model) => {
            if model.reindex {
                return true;
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
