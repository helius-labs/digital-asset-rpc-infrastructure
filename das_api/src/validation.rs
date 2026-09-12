use crate::config::Config;
use crate::error::DasApiError;
use digital_asset_types::{
    dao::{scopes::asset::TokenType, SearchAssetsQuery},
    rpc::{
        filter::{AssetSortBy, AssetSorting},
        options::Options,
    },
};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub fn validate_pubkey(str_pubkey: String) -> Result<Pubkey, DasApiError> {
    Pubkey::from_str(&str_pubkey).map_err(|_| DasApiError::PubkeyValidationError(str_pubkey))
}

pub fn validate_search_with_name(
    name: &Option<String>,
    owner: &Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>, DasApiError> {
    let opt_name = if let Some(n) = name {
        if owner.is_none() {
            return Err(DasApiError::ValidationError(
                "Owner address must be provided in order to search assets by name".to_owned(),
            ));
        }
        Some(n.clone().into_bytes())
    } else {
        None
    };
    Ok(opt_name)
}

pub fn validate_opt_pubkey(pubkey: &Option<String>) -> Result<Option<Vec<u8>>, DasApiError> {
    let opt_bytes = if let Some(pubkey) = pubkey {
        let pubkey = Pubkey::from_str(pubkey)
            .map_err(|_| DasApiError::ValidationError(format!("Invalid pubkey {}", pubkey)))?;
        Some(pubkey.to_bytes().to_vec())
    } else {
        None
    };
    Ok(opt_bytes)
}

pub fn validate_search_assets_query(
    saq: &SearchAssetsQuery,
    sort_by: &Option<AssetSorting>,
) -> Result<(), DasApiError> {
    // validation for queries that include "not"
    if let Some(not) = &saq.not_filter {
        // don't allow negate for "not" queries
        if saq.negate == Some(true) {
            return Err(DasApiError::ValidationError(
                "Cannot use both 'negate' and 'not' in the same query".to_string(),
            ));
        }

        // Don't allow "not" field without a search on grouping/owner/authority/creator
        if saq.owner_address.is_none()
            && saq.creator_address.is_none()
            && saq.authority_address.is_none()
            && saq.grouping.is_none()
        {
            return Err(DasApiError::ValidationError(
                    "Cannot use 'not' field without 'ownerAddress', 'creatorAddress', 'authorityAddress', or 'grouping' specified".to_string(),
                ));
        }

        // Don't allow "not" field on the same field that has a regular search
        if (not.owners.is_some() && saq.owner_address.is_some())
            || (not.creators.is_some() && saq.creator_address.is_some())
            || (not.authorities.is_some() && saq.authority_address.is_some())
            || (not.collections.is_some() && saq.grouping.is_some())
        {
            return Err(DasApiError::ValidationError(
                "Cannot specify 'not' filter on the same field that has a regular search'"
                    .to_string(),
            ));
        }

        // Enforce the sortBy is "none" for performance reasons
        if sort_by.clone().map(|s| s.sort_by) != Some(AssetSortBy::None) {
            return Err(DasApiError::ValidationError(
                format!("Sorting is not supported for queries that include the 'not' field. Please set 'sortBy' to 'none' to disable sorting."),
            ));
        }
    }

    // For token type
    if let Some(token_type) = &saq.token_type {
        if saq.owner_address.is_none() {
            return Err(DasApiError::ValidationError(
                "Must provide `owner_address` when using `token_type` field".to_string(),
            ));
        }

        if saq.tree.is_some() && token_type != &TokenType::CompressedNft {
            return Err(DasApiError::ValidationError(
                "`tree` is only supported when specifying `compressedNft in the `token_type` field"
                    .to_string(),
            ));
        }

        if saq.owner_type.is_some() {
            return Err(DasApiError::ValidationError(
                "`owner_type` is not supported when using `token_type` field".to_string(),
            ));
        }

        if saq.specification_asset_class.is_some() {
            return Err(DasApiError::ValidationError(
                "`specification_asset_class` is not supported when using `token_type` field"
                    .to_string(),
            ));
        }

        if saq.compressed.is_some() {
            return Err(DasApiError::ValidationError(
                "`compressed` is not supported when using `token_type` field".to_string(),
            ));
        }

        if saq.compressible.is_some() {
            return Err(DasApiError::ValidationError(
                "`compressible` is not supported when using `token_type` field".to_string(),
            ));
        }

        if saq.specification_version.is_some() {
            return Err(DasApiError::ValidationError(
                "`specification_version` is not supported when using `token_type` field"
                    .to_string(),
            ));
        }

        if *token_type == TokenType::Fungible || *token_type == TokenType::All {
            if saq.authority_address.is_some() {
                return Err(DasApiError::ValidationError(
                    "`authority_address` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.creator_address.is_some() {
                return Err(DasApiError::ValidationError(
                    "`creator_address` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.creator_verified.is_some() {
                return Err(DasApiError::ValidationError(
                    "`creator_verified` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.authority_address.is_some() {
                return Err(DasApiError::ValidationError(
                    "`authority_address` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.delegate.is_some() {
                return Err(DasApiError::ValidationError(
                    "`delegate` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.frozen.is_some() {
                return Err(DasApiError::ValidationError(
                    "`frozen` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.supply.is_some() {
                return Err(DasApiError::ValidationError(
                    "`supply` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.royalty_target_type.is_some()
                || saq.royalty_target.is_some()
                || saq.royalty_amount.is_some()
            {
                return Err(DasApiError::ValidationError(
                    "`royalty` field is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.burnt.is_some() {
                return Err(DasApiError::ValidationError(
                    "`burnt` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.json_uri.is_some() {
                return Err(DasApiError::ValidationError(
                    "`json_uri` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.collections.is_some() {
                return Err(DasApiError::ValidationError(
                    "`collections` is not supported for this `token_type`".to_string(),
                ));
            }

            if saq.name.is_some() {
                return Err(DasApiError::ValidationError(
                    "`name` is not supported for this `token_type`".to_string(),
                ));
            }
        }
    }
    // Validate created_at filter
    if let Some(created_at) = &saq.created_at {
        if created_at.before.is_none() && created_at.after.is_none() {
            return Err(DasApiError::ValidationError(
                "Must provide either `before` or `after` for `created_at` filter".to_string(),
            ));
        }
        if let (Some(before), Some(after)) = (&created_at.before, &created_at.after) {
            if before < after {
                return Err(DasApiError::ValidationError(
                    "`before` must be greater than `after` in `created_at` filter".to_string(),
                ));
            }
        }

        // created_at filter is not supported when specifying creators because it would require
        // a join and be too slow.
        if saq.creator_address.is_some() {
            return Err(DasApiError::ValidationError(
                "`created_at` filter is not supported when specifying creators".to_string(),
            ));
        }

        if let Some(TokenType::Fungible | TokenType::All) = saq.token_type.clone() {
            return Err(DasApiError::ValidationError(
                "`created_at` filter is not supported for fungibles".to_string(),
            ));
        }
    }

    if saq.tree.is_some() {
        // Without a tree grouping or owner address filter, we risk a full table scan
        if saq.owner_address.is_none() && saq.grouping.is_none() {
            return Err(DasApiError::ValidationError(
                "Must provide either `owner_address` or `grouping` with `tree` filter".to_string(),
            ));
        }
    }

    Ok(())
}

pub struct RequestValidator {
    db_urls: Vec<String>,
    collection_list: Vec<String>,
    creators_list: Vec<String>,
    authority_list: Vec<String>,
}

impl RequestValidator {
    pub fn from_config(config: Config) -> RequestValidator {
        let db_urls = match config.database_urls {
            Some(list) => list.split(',').map(String::from).collect(),
            None => vec![],
        };
        let collection_list = match config.collection_list {
            Some(list) => list.split(',').map(String::from).collect(),
            None => vec![],
        };
        let creators_list = match config.creators_list {
            Some(list) => list.split(',').map(String::from).collect(),
            None => vec![],
        };
        let authority_list = match config.authority_list {
            Some(list) => list.split(',').map(String::from).collect(),
            None => vec![],
        };
        RequestValidator {
            db_urls,
            collection_list,
            creators_list,
            authority_list,
        }
    }

    /// Ensure valid options are provided. Grand total is not allowed for large groups for performance reasons.
    pub fn validate_options(
        &self,
        key: &String,
        value: &String,
        options: &Option<Options>,
    ) -> Result<(), DasApiError> {
        if let Some(opts) = options {
            if opts.show_grand_total && self.is_large_group(key, value) {
                return Err(DasApiError::ValidationError(
                    format!(
                        "Grand total is disabled for large {} ({}) for performance reasons. Please contact us if you require this feature.",
                        Self::pluralize_key(key),
                        value
                    )
                ));
            }
        }
        Ok(())
    }

    /// Ensure that the provided search key/value pair is not hitting a large group.
    /// Large groups (collections/creators/authorities) can cause performance issues for certain requests so we need to block them.
    fn is_large_group(&self, search_key: &String, search_value: &String) -> bool {
        match search_key.as_str() {
            "collection" => self.collection_list.contains(search_value),
            "creators" => self.creators_list.contains(search_value),
            "authority" => self.authority_list.contains(search_value),
            _ => false,
        }
    }

    fn pluralize_key(search_key: &String) -> &str {
        match search_key.as_str() {
            "collection" => "collections",
            "creators" => "creators",
            "authority" => "authorities",
            // Should never happen. Leave as-is.
            _ => search_key,
        }
    }

    pub fn get_database_urls(&self) -> Vec<String> {
        self.db_urls.clone()
    }
}
