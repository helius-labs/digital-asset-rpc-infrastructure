use {
    crate::{
        error::IngesterError,
        program_transformers::{
            asset_upserts::{
                download_task_warranted, guard_asset_creators_noop, guard_asset_data_v2_noop,
                guard_authorities_collections_noop, guard_offchain_insert_repair,
                settle_asset_creators_positions, upsert_assets_metadata_account_columns,
                upsert_assets_mint_account_columns, upsert_assets_token_account_columns,
                AssetMetadataAccountColumns, AssetMintAccountColumns, AssetTokenAccountColumns,
            },
            bubblegum::{upsert_creators_info_in_asset_raw, upsert_owner_for_core},
            utils::find_model_with_retry,
        },
        tasks::{DownloadMetadata, IntoTaskData, TaskData},
    },
    blockbuster::{
        mpl_core::{
            types::{
                ExternalPluginAdapterType, Plugin, PluginAuthority, PluginType, UpdateAuthority,
            },
            IndexableAsset,
        },
        programs::mpl_core_program::{mpl_core_id, MplCoreAccountData},
    },
    cadence_macros::statsd_count,
    chrono::Utc,
    common::metric,
    digital_asset_types::{
        dao::{
            asset, asset_creators, asset_data_v2, offchain_metadata,
            sea_orm_active_enums::{
                ChainMutability, Mutability, OwnerType, SpecificationAssetClass,
            },
            AuthorityInfo, CollectionsInfo, CreatorInfo,
        },
        json::ChainDataV1,
    },
    heck::ToSnakeCase,
    log::warn,
    plerkle_serialization::{AccountInfo, Pubkey as FBPubkey},
    sea_orm::{
        entity::{ActiveValue, EntityTrait},
        query::{JsonValue, QueryFilter, QueryTrait},
        sea_query::query::OnConflict,
        ColumnTrait, ConnectionTrait, DbBackend, Set, TransactionTrait,
    },
    serde_json::{value::Value, Map},
    solana_sdk::pubkey::Pubkey,
    std::iter::Iterator,
};

pub async fn burn_v1_asset<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    id: FBPubkey,
    slot: u64,
) -> Result<(), IngesterError> {
    let slot_i = slot as i64;
    let model = asset::ActiveModel {
        id: ActiveValue::Set(id.0.to_vec()),
        slot_updated: ActiveValue::Set(Some(slot_i)),
        burnt: ActiveValue::Set(true),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([asset::Column::SlotUpdated, asset::Column::Burnt])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated > asset.slot_updated",
        query.sql
    );
    conn.execute(query).await?;
    Ok(())
}

const RETRY_INTERVALS: &[u64] = &[0, 5, 10];

pub async fn save_v1_asset<'a, T: ConnectionTrait + TransactionTrait>(
    account_update: &'a AccountInfo<'a>,
    conn: &T,
    id: FBPubkey,
    account_data: &MplCoreAccountData,
    slot: u64,
) -> Result<Option<TaskData>, IngesterError> {
    let mpl_program = account_update.owner().unwrap().0.to_vec();
    // Notes:
    // The address of the Core asset is used for Core Asset ID.  There are no token or mint accounts.
    // There are no `MasterEdition` or `Edition` accounts associated with Core assets.
    let id_vec = id.0.to_vec();

    // Note: This indexes Core Assets, Core Collections, and Core Groups.
    // A GroupV1's memberships live in `parent_groups`, not a Groups plugin.
    let (asset, parent_groups) = match account_data {
        MplCoreAccountData::Asset(indexable_asset) => (indexable_asset, None),
        MplCoreAccountData::Collection(indexable_asset) => (indexable_asset, None),
        MplCoreAccountData::Group {
            indexable_asset,
            group,
        } => (
            indexable_asset,
            Some(
                group
                    .parent_groups
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            ),
        ),
        _ => return Err(IngesterError::NotImplemented),
    };

    // If it is an `Address` type, use the value directly.  If it is a `Collection`, search for and
    // use the collection's authority.
    let update_authority = match asset.update_authority {
        UpdateAuthority::Address(address) => address.to_bytes().to_vec(),
        UpdateAuthority::Collection(address) => find_model_with_retry(
            conn,
            "mpl_core",
            &asset::Entity::find().filter(asset::Column::Id.eq(address.to_bytes().to_vec())),
            RETRY_INTERVALS,
        )
        .await?
        .map(|model| {
            model
                .authorities_info
                .as_ref()
                .and_then(|info| serde_json::from_value::<Vec<u8>>(info["authority"].clone()).ok())
                .unwrap_or_default()
        })
        .unwrap_or_default(),
        UpdateAuthority::None => Pubkey::default().to_bytes().to_vec(),
    };

    let slot_i = slot as i64;

    let txn = conn.begin().await?;

    let authority_info = AuthorityInfo {
        authority: update_authority.clone(),
        seq: 0,
        slot_updated: slot_i,
        scopes: None,
    };
    let mut asset_model = asset::ActiveModel {
        id: Set(id_vec.clone()),
        authorities_info: Set(Some(authority_info.into())),
        authority_address: Set(Some(update_authority.clone())),
        authority_scopes: Set(None),
        authority_seq: Set(Some(0)),
        authority_slot_updated: Set(Some(slot_i)),
        ..Default::default()
    };

    // Stored on collections_info so grouping and getAssetsByGroup can read it
    // without a dedicated column.
    let group_memberships = build_mpl_core_group_values(parent_groups, asset);

    let collections_info = if let UpdateAuthority::Collection(address) = asset.update_authority {
        Some(CollectionsInfo {
            collection_id: Some(address.to_string()),
            seq: None,
            slot_updated: slot_i,
            verified: true,
            collection_info_seq: None,
            groups: group_memberships,
            ..Default::default()
        })
    } else {
        Some(CollectionsInfo {
            collection_id: None,
            seq: None,
            slot_updated: slot_i,
            verified: false,
            collection_info_seq: None,
            groups: group_memberships,
            ..Default::default()
        })
    };

    asset_model.collections_info = Set(Some(collections_info.into()));

    let mut asset_query = asset::Entity::insert(asset_model)
        .on_conflict(OnConflict::columns([asset::Column::Id]))
        .build(DbBackend::Postgres);

    asset_query.sql = format!(
            "{} DO UPDATE SET
            authorities_info = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.authorities_info, '{{}}') ELSE asset.authorities_info END,
            authority_address = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_address ELSE asset.authority_address END,
            authority_slot_updated = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_slot_updated ELSE asset.authority_slot_updated END,
            authority_scopes = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_scopes ELSE asset.authority_scopes END,
            collections_info = CASE WHEN COALESCE((excluded.collections_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.collections_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.collections_info, '{{}}') ELSE asset.collections_info END
            WHERE asset.id = excluded.id",
            asset_query.sql
        );
    asset_query.sql = guard_authorities_collections_noop(asset_query.sql);

    txn.execute(asset_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    txn.commit().await?;

    //-----------------------
    // asset_data table
    //-----------------------

    let name = asset.name.clone().into_bytes();
    let uri = asset.uri.trim().replace('\0', "");

    // Notes:
    // There is no symbol for a Core asset.
    // Edition nonce hardcoded to `None`.
    // There is no primary sale concept for Core Assets, hardcoded to `false`.
    // Token standard is hardcoded to `None`.
    let mut chain_data = ChainDataV1 {
        name: asset.name.clone(),
        symbol: "".to_string(),
        edition_nonce: None,
        primary_sale_happened: false,
        token_standard: None,
        uses: None,
    };

    chain_data.sanitize();
    let chain_data_json = serde_json::to_value(chain_data)
        .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;

    // Note:
    // Mutability set based on core asset data having an update authority.
    // Individual plugins could have some or no authority giving them individual mutability status.
    let chain_mutability = match asset.update_authority {
        UpdateAuthority::None => ChainMutability::Immutable,
        _ => ChainMutability::Mutable,
    };

    let offchain_metadata_model = offchain_metadata::ActiveModel {
        metadata_url: Set(uri.clone()),
        metadata: Set(JsonValue::String("processing".to_string())),
        mutability: Set(Mutability::Mutable),
        reindex: Set(true),
        ..Default::default()
    };
    let mut offchain_metadata_query = offchain_metadata::Entity::insert(offchain_metadata_model)
        .on_conflict(
            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                .update_columns([offchain_metadata::Column::Reindex])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    offchain_metadata_query.sql = guard_offchain_insert_repair(offchain_metadata_query.sql);

    let asset_data_v2_model = asset_data_v2::ActiveModel {
        metadata_url: Set(uri.clone()),
        id: Set(id_vec.clone()),
        chain_mutability: Set(chain_mutability),
        chain_data: Set(chain_data_json),
        slot_updated: Set(slot_i),
        base_info_seq: Set(Some(0)),
        raw_name: Set(Some(name.to_vec())),
        raw_symbol: Set(None),
    };
    let mut asset_data_v2_query = asset_data_v2::Entity::insert(asset_data_v2_model)
        .on_conflict(
            OnConflict::columns([asset_data_v2::Column::Id])
                .update_columns([
                    asset_data_v2::Column::ChainMutability,
                    asset_data_v2::Column::ChainData,
                    asset_data_v2::Column::MetadataUrl,
                    asset_data_v2::Column::SlotUpdated,
                    asset_data_v2::Column::BaseInfoSeq,
                    asset_data_v2::Column::RawName,
                    asset_data_v2::Column::RawSymbol,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    asset_data_v2_query.sql = guard_asset_data_v2_noop(asset_data_v2_query.sql);

    let txn = conn.begin().await?;
    let offchain_res = txn
        .execute(offchain_metadata_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    let asset_data_res = txn
        .execute(asset_data_v2_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    //-----------------------
    // asset table
    //-----------------------

    let ownership_type = OwnerType::Single;
    let (owner, class) = match account_data {
        MplCoreAccountData::Asset(_) => (
            asset.owner.map(|owner| owner.to_bytes().to_vec()),
            SpecificationAssetClass::MplCoreAsset,
        ),
        MplCoreAccountData::Collection(_) => (
            Some(update_authority.clone()),
            SpecificationAssetClass::MplCoreCollection,
        ),
        MplCoreAccountData::Group { .. } => (
            Some(update_authority.clone()),
            SpecificationAssetClass::MplCoreGroup,
        ),
        _ => return Err(IngesterError::NotImplemented),
    };

    let is_asset = matches!(account_data, MplCoreAccountData::Asset(_));

    let is_agent = is_asset
        && asset
            .external_plugins
            .iter()
            .any(|ep| ep.r#type == ExternalPluginAdapterType::AgentIdentity);

    let asset_signer_bytes = if is_asset {
        let (pda, _) = Pubkey::find_program_address(
            &[b"mpl-core-execute".as_ref(), id.0.as_ref()],
            &mpl_core_id(),
        );
        Some(pda.to_bytes().to_vec())
    } else {
        None
    };

    // Get royalty amount and creators from `Royalties` plugin if available.
    let default_creators = Vec::new();
    let (royalty_amount, royalty_creators) = asset
        .plugins
        .get(&PluginType::Royalties)
        .and_then(|plugin_schema| {
            if let Plugin::Royalties(royalties) = &plugin_schema.data {
                Some((royalties.basis_points, &royalties.creators))
            } else {
                None
            }
        })
        .unwrap_or((0, &default_creators));

    // Serialize known plugins into JSON.
    let mut plugins_json = serde_json::to_value(&asset.plugins)
        .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;

    // Improve JSON output.
    remove_plugins_nesting(&mut plugins_json, "data");
    transform_plugins_authority(&mut plugins_json);
    convert_keys_to_snake_case(&mut plugins_json);

    // Serialize any unknown plugins into JSON.
    let unknown_plugins_json = if !asset.unknown_plugins.is_empty() {
        let mut unknown_plugins_json = serde_json::to_value(&asset.unknown_plugins)
            .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;

        // Improve JSON output.
        transform_plugins_authority(&mut unknown_plugins_json);
        convert_keys_to_snake_case(&mut unknown_plugins_json);

        Some(unknown_plugins_json)
    } else {
        None
    };

    // Serialize known external plugins into JSON.
    let mut external_plugins_json = serde_json::to_value(&asset.external_plugins)
        .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;

    // Improve JSON output.
    remove_plugins_nesting(&mut external_plugins_json, "adapter_config");
    transform_plugins_authority(&mut external_plugins_json);
    convert_keys_to_snake_case(&mut external_plugins_json);

    // Serialize any unknown external plugins into JSON.
    let unknown_external_plugins_json = if !asset.unknown_external_plugins.is_empty() {
        let mut unknown_external_plugins_json =
            serde_json::to_value(&asset.unknown_external_plugins)
                .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;

        // Improve JSON output.
        transform_plugins_authority(&mut unknown_external_plugins_json);
        convert_keys_to_snake_case(&mut unknown_external_plugins_json);

        Some(unknown_external_plugins_json)
    } else {
        None
    };

    upsert_assets_metadata_account_columns(
        AssetMetadataAccountColumns {
            mint: id_vec.clone(),
            metadata_account_id: id_vec.clone(),
            owner_type: ownership_type,
            specification_asset_class: Some(class),
            royalty_amount: royalty_amount as i32,
            asset_data: Some(id_vec.clone()),
            slot_updated_metadata_account: slot,
            mpl_core_plugins: Some(plugins_json),
            mpl_core_unknown_plugins: unknown_plugins_json,
            mpl_core_collection_num_minted: asset.num_minted.map(|val| val as i32),
            mpl_core_collection_current_size: asset.current_size.map(|val| val as i32),
            mpl_core_plugins_json_version: Some(1),
            mpl_core_external_plugins: Some(external_plugins_json),
            mpl_core_unknown_external_plugins: unknown_external_plugins_json,
            is_agent,
            asset_signer: asset_signer_bytes,
        },
        &txn,
    )
    .await?;

    let supply = 1;

    // Note: these need to be separate for Token Metadata but here could be one upsert.
    upsert_assets_mint_account_columns(
        AssetMintAccountColumns {
            mint: id_vec.clone(),
            supply_mint: None,
            supply,
            slot_updated_mint_account: slot,
        },
        &txn,
    )
    .await?;

    // Get transfer delegate from `TransferDelegate` plugin if available.
    let transfer_delegate =
        asset
            .plugins
            .get(&PluginType::TransferDelegate)
            .and_then(|plugin_schema| match &plugin_schema.authority {
                PluginAuthority::Owner => owner.clone(),
                PluginAuthority::UpdateAuthority => Some(update_authority.clone()),
                PluginAuthority::Address { address } => Some(address.to_bytes().to_vec()),
                PluginAuthority::None => None,
            });

    // Get frozen status from `FreezeDelegate` plugin if available.
    let frozen = asset
        .plugins
        .get(&PluginType::FreezeDelegate)
        .and_then(|plugin_schema| {
            if let Plugin::FreezeDelegate(freeze_delegate) = &plugin_schema.data {
                Some(freeze_delegate.frozen)
            } else {
                None
            }
        })
        .unwrap_or(false);

    // TODO: these upserts needed to be separate for Token Metadata but here could be one upsert.
    upsert_assets_token_account_columns(
        AssetTokenAccountColumns {
            mint: id_vec.clone(),
            owner: owner.clone(),
            frozen,
            // Note use transfer delegate for the existing delegate field.
            delegate: transfer_delegate.clone(),
            slot_updated_token_account: Some(slot_i),
            token_extensions: None,
        },
        &txn,
    )
    .await?;

    //----------------------
    // owners table
    //----------------------
    if let Some(owner_vec) = owner.clone() {
        upsert_owner_for_core(
            conn,
            id_vec.clone(),
            owner_vec,
            transfer_delegate.clone(),
            slot_i,
            frozen,
            mpl_program,
        )
        .await?;
    }

    //-----------------------
    // creators table
    //-----------------------

    let creators = royalty_creators
        .iter()
        .enumerate()
        .map(|(i, creator)| asset_creators::ActiveModel {
            asset_id: ActiveValue::Set(id_vec.clone()),
            position: ActiveValue::Set(i as i16),
            creator: ActiveValue::Set(creator.address.to_bytes().to_vec()),
            share: ActiveValue::Set(creator.percentage as i32),
            // Note all creators are verified for Core Assets.
            verified: ActiveValue::Set(true),
            slot_updated: ActiveValue::Set(Some(slot_i)),
            seq: ActiveValue::Set(Some(0)),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    let creator_count = creators.len() as i16;
    if !creators.is_empty() {
        let mut query = asset_creators::Entity::insert_many(creators)
            .on_conflict(
                OnConflict::columns([
                    asset_creators::Column::AssetId,
                    asset_creators::Column::Position,
                ])
                .update_columns([
                    asset_creators::Column::Creator,
                    asset_creators::Column::Share,
                    asset_creators::Column::Verified,
                    asset_creators::Column::Seq,
                    asset_creators::Column::SlotUpdated,
                ])
                .to_owned(),
            )
            .build(DbBackend::Postgres);
        query.sql = guard_asset_creators_noop(query.sql);
        txn.execute(query)
            .await
            .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
        // The guarded upsert may skip unchanged positions and never removes
        // dropped ones; settle both so the read path's max-slot staleness
        // filter sees exactly the incoming creator set.
        settle_asset_creators_positions(&txn, id_vec.clone(), slot_i, creator_count)
            .await
            .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

        // Upsert creators_info JSONB column for optimized queries.
        // Note: MPL Core creators use 'percentage' instead of 'share' and are always verified.
        let creator_infos: Vec<CreatorInfo> = royalty_creators
            .iter()
            .map(|c| CreatorInfo {
                creator: c.address.to_bytes().to_vec(),
                share: Some(c.percentage),
                verified: true,
            })
            .collect();
        upsert_creators_info_in_asset_raw(&txn, id_vec.clone(), creator_infos, slot_i, 0).await?;
    }

    // Commit the database transaction.
    txn.commit().await?;

    // Return early if there is no URI.
    if uri.is_empty() {
        warn!(
            "URI is empty for mint {}. Skipping background task.",
            bs58::encode(id_vec.clone()).into_string()
        );
        return Ok(None);
    }

    // An account touch that neither introduced a new URI nor changed stored
    // metadata is no evidence the off-chain document changed — skip the task.
    if !download_task_warranted(offchain_res.rows_affected(), asset_data_res.rows_affected()) {
        metric! {
            statsd_count!("ingester.bgtask.noop_metadata_skip", 1);
        }
        return Ok(None);
    }

    // Otherwise return with info for background downloading.
    let mut task = DownloadMetadata {
        asset_data_id: id_vec.clone(),
        uri,
        created_at: Some(Utc::now().naive_utc()),
    };
    task.sanitize();
    let t = task.into_task_data()?;
    Ok(Some(t))
}

// Group memberships as base58 addresses: parent_groups for a GroupV1, else the
// Groups plugin list.
fn build_mpl_core_group_values(
    parent_groups: Option<Vec<String>>,
    asset: &IndexableAsset,
) -> Vec<String> {
    if let Some(parent_groups) = parent_groups {
        return parent_groups;
    }

    asset
        .plugins
        .get(&PluginType::Groups)
        .and_then(|plugin| match &plugin.data {
            Plugin::Groups(groups) => Some(groups.groups.iter().map(ToString::to_string).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

// Modify the JSON structure to remove the `Plugin`` name and just display its data.
// For example, this will transform `FreezeDelegate` JSON from:
// "data":{"freeze_delegate":{"frozen":false}}}
// to:
// "data":{"frozen":false}
fn remove_plugins_nesting(plugins_json: &mut Value, nested_key: &str) {
    match plugins_json {
        Value::Object(plugins) => {
            // Handle the case where plugins_json is an object.
            for (_, plugin) in plugins.iter_mut() {
                remove_nesting_from_plugin(plugin, nested_key);
            }
        }
        Value::Array(plugins_array) => {
            // Handle the case where plugins_json is an array.
            for plugin in plugins_array.iter_mut() {
                remove_nesting_from_plugin(plugin, nested_key);
            }
        }
        _ => {}
    }
}

fn remove_nesting_from_plugin(plugin: &mut Value, nested_key: &str) {
    if let Some(Value::Object(nested_key)) = plugin.get_mut(nested_key) {
        // Extract the plugin data and remove it.
        if let Some((_, inner_plugin_data)) = nested_key.iter().next() {
            let inner_plugin_data_clone = inner_plugin_data.clone();
            // Clear the `nested_key` object.
            nested_key.clear();
            // Move the plugin data fields to the top level of `nested_key`.
            if let Value::Object(inner_plugin_data) = inner_plugin_data_clone {
                for (field_name, field_value) in inner_plugin_data.iter() {
                    nested_key.insert(field_name.clone(), field_value.clone());
                }
            }
        }
    }
}
// Modify the JSON for `PluginAuthority` to have consistent output no matter the enum type.
// For example, from:
// "authority":{"Address":{"address":"D7whDWAP5gN9x4Ff6T9MyQEkotyzmNWtfYhCEWjbUDBM"}}
// to:
// "authority":{"address":"4dGxsCAwSCopxjEYY7sFShFUkfKC6vzsNEXJDzFYYFXh","type":"Address"}
// and from:
// "authority":"UpdateAuthority"
// to:
// "authority":{"address":null,"type":"UpdateAuthority"}
fn transform_plugins_authority(plugins_json: &mut Value) {
    match plugins_json {
        Value::Object(plugins) => {
            // Transform plugins in an object
            for (_, plugin) in plugins.iter_mut() {
                if let Some(plugin_obj) = plugin.as_object_mut() {
                    transform_authority_in_object(plugin_obj);
                    transform_data_authority_in_object(plugin_obj);
                    transform_linked_app_data_parent_key_in_object(plugin_obj);
                }
            }
        }
        Value::Array(plugins_array) => {
            // Transform plugins in an array
            for plugin in plugins_array.iter_mut() {
                if let Some(plugin_obj) = plugin.as_object_mut() {
                    transform_authority_in_object(plugin_obj);
                    transform_data_authority_in_object(plugin_obj);
                    transform_linked_app_data_parent_key_in_object(plugin_obj);
                }
            }
        }
        _ => {}
    }
}

fn transform_authority_in_object(plugin: &mut Map<String, Value>) {
    if let Some(authority) = plugin.get_mut("authority") {
        transform_authority(authority);
    }
}

fn transform_data_authority_in_object(plugin: &mut Map<String, Value>) {
    if let Some(adapter_config) = plugin.get_mut("adapter_config") {
        if let Some(data_authority) = adapter_config
            .as_object_mut()
            .and_then(|o| o.get_mut("data_authority"))
        {
            transform_authority(data_authority);
        }
    }
}

fn transform_linked_app_data_parent_key_in_object(plugin: &mut Map<String, Value>) {
    if let Some(adapter_config) = plugin.get_mut("adapter_config") {
        if let Some(parent_key) = adapter_config
            .as_object_mut()
            .and_then(|o| o.get_mut("parent_key"))
        {
            if let Some(linked_app_data) = parent_key
                .as_object_mut()
                .and_then(|o| o.get_mut("LinkedAppData"))
            {
                transform_authority(linked_app_data);
            }
        }
    }
}

fn transform_authority(authority: &mut Value) {
    match authority {
        Value::Object(authority_obj) => {
            if let Some(authority_type) = authority_obj.keys().next().cloned() {
                // Replace the nested JSON objects with desired format.
                if let Some(Value::Object(pubkey_obj)) = authority_obj.remove(&authority_type) {
                    if let Some(address_value) = pubkey_obj.get("address") {
                        authority_obj.insert("type".to_string(), Value::from(authority_type));
                        authority_obj.insert("address".to_string(), address_value.clone());
                    }
                }
            }
        }
        Value::String(authority_type) => {
            // Handle the case where authority is a string.
            let mut authority_obj = Map::new();
            authority_obj.insert("type".to_string(), Value::String(authority_type.clone()));
            authority_obj.insert("address".to_string(), Value::Null);
            *authority = Value::Object(authority_obj);
        }
        _ => {}
    }
}

// Convert all keys to snake case.  Ignore values that aren't JSON objects themselves.
fn convert_keys_to_snake_case(plugins_json: &mut Value) {
    match plugins_json {
        Value::Object(obj) => {
            let keys = obj.keys().cloned().collect::<Vec<String>>();
            for key in keys {
                let snake_case_key = key.to_snake_case();
                if let Some(val) = obj.remove(&key) {
                    obj.insert(snake_case_key, val);
                }
            }
            for (_, val) in obj.iter_mut() {
                convert_keys_to_snake_case(val);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                convert_keys_to_snake_case(val);
            }
        }
        _ => {}
    }
}
