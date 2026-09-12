use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorInfo {
    pub creator: Vec<u8>,
    pub share: Option<u8>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorsInfo {
    pub creators: Vec<CreatorInfo>,
    pub slot_updated: Option<i64>,
    pub seq: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AuthorityInfo {
    pub authority: Vec<u8>,
    pub seq: i64,
    pub slot_updated: i64,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct CollectionsInfo {
    pub collection_id: Option<String>,
    pub seq: Option<i64>,
    pub slot_updated: i64,
    pub verified: bool,
    pub collection_info_seq: Option<i64>,
    pub collection_nft: Option<bool>,
    pub collection_size: Option<u64>,
    // MPL Core group memberships (base58); parent_groups for a GroupV1.
    pub groups: Vec<String>,
}

impl From<CreatorInfo> for serde_json::Value {
    fn from(info: CreatorInfo) -> Self {
        serde_json::json!({
            "creator": info.creator,
            "share": info.share,
            "verified": info.verified,
        })
    }
}

impl From<CreatorsInfo> for serde_json::Value {
    fn from(info: CreatorsInfo) -> Self {
        serde_json::json!({
            "creators": info.creators,
            "slot_updated": info.slot_updated,
            "seq": info.seq,
        })
    }
}

impl From<AuthorityInfo> for Value {
    fn from(item: AuthorityInfo) -> Self {
        serde_json::json!({
            "authority": item.authority,
            "seq": item.seq,
            "slot_updated": item.slot_updated,
            "scopes": item.scopes,
        })
    }
}

impl From<CollectionsInfo> for Value {
    fn from(item: CollectionsInfo) -> Self {
        serde_json::json!({
            "collection_id": item.collection_id,
            "seq": item.seq,
            "slot_updated": item.slot_updated,
            "verified": item.verified,
            "collection_info_seq": item.collection_info_seq,
            "collection_nft": item.collection_nft,
            "collection_size": item.collection_size,
            "groups": item.groups,
        })
    }
}
