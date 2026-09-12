use crate::dao::owners;
use crate::dao::sea_orm_active_enums::EditionAccountType;
use crate::dao::sea_orm_active_enums::SpecificationVersions;
use crate::dao::AssetMetadata;
use crate::dao::FullAsset;
use crate::dao::PageOptions;
use crate::dao::Pagination;
use crate::dao::{asset, asset_creators};
use crate::rpc::filter::TokenSortBy;
use crate::rpc::filter::TokenSortDirection;
use crate::rpc::filter::TokenSorting;
use crate::rpc::filter::{AssetSortBy, AssetSortDirection, AssetSorting};
use crate::rpc::options::Options;
use crate::rpc::response::OwnerList;
use crate::rpc::response::TokenAccountsList;
use crate::rpc::response::{AssetError, AssetList, TransactionSignatureList};
use crate::rpc::MplCoreInfo;
use crate::rpc::Owner;
use crate::rpc::SystemInfo;
use crate::rpc::TokenAccount;
use crate::rpc::{
    Asset as RpcAsset, Compression, Content, Creator, File, Group, Interface, MetadataMap,
    Ownership, Royalty, Supply, Uses,
};
use chrono::Utc;
use jsonpath_lib::JsonPathError;
use log::warn;
use mime_guess::Mime;

use sea_orm::DbErr;
use serde_json::Map;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use url::Url;

use once_cell::sync::Lazy;
use serde_json;

// https://github.com/solana-labs/token-list
pub static LEGACY_TOKEN_IMAGES: Lazy<HashMap<String, String>> = Lazy::new(|| {
    let json_data_bytes = include_bytes!("../legacy_token_metadata/address_to_logo_uri.json");
    let json_data_str = std::str::from_utf8(json_data_bytes).unwrap();
    serde_json::from_str(&json_data_str).unwrap()
});

pub fn to_uri(uri: String) -> Option<Url> {
    Url::parse(&*uri).ok()
}

pub fn get_mime(url: Url) -> Option<Mime> {
    mime_guess::from_path(Path::new(url.path())).first()
}

pub fn get_mime_type_from_uri(uri: String) -> String {
    let default_mime_type = "image/png".to_string();
    to_uri(uri)
        .and_then(get_mime)
        .map_or(default_mime_type, |m| m.to_string())
}

pub fn file_from_str(str: String) -> File {
    let mime = get_mime_type_from_uri(str.clone());
    File {
        uri: Some(str),
        cdn_uri: None,
        mime: Some(mime),
        quality: None,
        contexts: None,
    }
}

pub fn build_asset_response(
    last_indexed_slot: u64,
    assets: Vec<FullAsset>,
    limit: u64,
    grand_total: Option<u64>,
    pagination: &Pagination,
    options: &Options,
) -> AssetList {
    let total = assets.len() as u32;
    let (page, before, after, cursor) = match pagination {
        Pagination::Keyset { before, after } => {
            let bef = before.clone().and_then(|x| String::from_utf8(x).ok());
            let aft = after.clone().and_then(|x| String::from_utf8(x).ok());
            (None, bef, aft, None)
        }
        Pagination::Page { page } => (Some(*page), None, None, None),
        Pagination::Cursor(_) => {
            if let Some(last_asset) = assets.last() {
                let cursor_str = bs58::encode(&last_asset.asset.id.clone()).into_string();
                (None, None, None, Some(cursor_str))
            } else {
                (None, None, None, None)
            }
        }
    };

    let (items, errors) = asset_list_to_rpc(assets, options);
    AssetList {
        last_indexed_slot,
        grand_total,
        total,
        limit: limit as u32,
        page: page.map(|x| x as u32),
        before,
        after,
        items,
        errors,
        cursor,
        nativeBalance: None,
    }
}

pub fn build_transaction_signatures_response(
    last_indexed_slot: u64,
    items: Vec<(String, String)>,
    limit: u64,
    pagination: &Pagination,
) -> TransactionSignatureList {
    let total = items.len() as u32;
    let (page, before, after) = match pagination {
        Pagination::Keyset { before, after } => {
            let bef = before.clone().and_then(|x| String::from_utf8(x).ok());
            let aft = after.clone().and_then(|x| String::from_utf8(x).ok());
            (None, bef, aft)
        }
        Pagination::Page { page } => (Some(*page), None, None),
        Pagination::Cursor { .. } => (None, None, None),
    };
    TransactionSignatureList {
        last_indexed_slot,
        total,
        limit: limit as u32,
        page: page.map(|x| x as u32),
        before,
        after,
        items,
    }
}

pub fn build_owner_response(last_indexed_slot: u64, owners: Vec<Owner>, limit: u64, pagination: &Pagination) -> OwnerList {
    let total = owners.len() as u32;
    let page = match pagination {
        Pagination::Page { page } => Some(*page as u32),
        _ => None,
    };
    OwnerList {
        last_indexed_slot,
        total,
        limit: limit as u32,
        page,
        owners,
    }
}

pub fn build_token_account_response(
    last_indexed_slot: u64,
    token_accounts: Vec<TokenAccount>,
    limit: u64,
    pagination: &Pagination,
) -> TokenAccountsList {
    let total = token_accounts.len() as u32;
    let (page, before, after, cursor) = match pagination {
        Pagination::Keyset { before, after } => {
            let bef = before.clone().and_then(|x| String::from_utf8(x).ok());
            let aft = after.clone().and_then(|x| String::from_utf8(x).ok());
            (None, bef, aft, None)
        }
        Pagination::Page { page } => (Some(*page as u32), None, None, None),
        Pagination::Cursor(_) => {
            if let Some(last_token) = token_accounts.last() {
                (None, None, None, Some(last_token.address.clone()))
            } else {
                (None, None, None, None)
            }
        }
    };
    TokenAccountsList {
        last_indexed_slot,
        total,
        limit: limit as u32,
        page,
        token_accounts,
        before,
        after,
        cursor,
    }
}

pub fn create_sorting(sorting: AssetSorting) -> (sea_orm::query::Order, Option<asset::Column>) {
    let sort_column = match sorting.sort_by {
        AssetSortBy::Id => Some(asset::Column::Id),
        AssetSortBy::Created => Some(asset::Column::CreatedAt),
        AssetSortBy::Updated => Some(asset::Column::SlotUpdated),
        AssetSortBy::RecentAction => Some(asset::Column::SlotUpdated),
        AssetSortBy::None => None,
    };
    let sort_direction = match sorting.sort_direction.unwrap_or_default() {
        AssetSortDirection::Desc => sea_orm::query::Order::Desc,
        AssetSortDirection::Asc => sea_orm::query::Order::Asc,
    };
    (sort_direction, sort_column)
}

pub fn create_owner_sorting(
    sorting: AssetSorting,
) -> (sea_orm::query::Order, Option<owners::Column>) {
    let sort_column = match sorting.sort_by {
        AssetSortBy::Id => Some(owners::Column::Mint),
        AssetSortBy::Created => Some(owners::Column::CreatedAt),
        AssetSortBy::Updated => Some(owners::Column::SlotUpdated),
        AssetSortBy::RecentAction => Some(owners::Column::SlotUpdated),
        AssetSortBy::None => None,
    };
    let sort_direction = match sorting.sort_direction.unwrap_or_default() {
        AssetSortDirection::Desc => sea_orm::query::Order::Desc,
        AssetSortDirection::Asc => sea_orm::query::Order::Asc,
    };
    (sort_direction, sort_column)
}

pub fn create_token_sorting(
    sorting: TokenSorting,
) -> (sea_orm::query::Order, Option<owners::Column>) {
    let sort_column = match sorting.sort_by {
        TokenSortBy::TokenAccount => Some(owners::Column::TokenAccount),
        TokenSortBy::None => None,
    };
    let sort_direction = match sorting.sort_direction.unwrap_or_default() {
        TokenSortDirection::Desc => sea_orm::query::Order::Desc,
        TokenSortDirection::Asc => sea_orm::query::Order::Asc,
    };
    (sort_direction, sort_column)
}

pub fn create_pagination(page_options: &PageOptions) -> Result<Pagination, DbErr> {
    if let Some(cursor) = &page_options.cursor {
        Ok(Pagination::Cursor(cursor.clone()))
    } else {
        match (
            page_options.before.as_ref(),
            page_options.after.as_ref(),
            page_options.page,
        ) {
            (_, _, None) => Ok(Pagination::Keyset {
                before: page_options.before.clone(),
                after: page_options.after.clone(),
            }),
            (None, None, Some(p)) => Ok(Pagination::Page { page: p }),
            _ => Err(DbErr::Custom("Invalid Pagination".to_string())),
        }
    }
}

pub fn track_top_level_file(
    file_map: &mut HashMap<String, File>,
    top_level_file: Option<&serde_json::Value>,
) {
    if top_level_file.is_some() {
        let img = top_level_file.and_then(|x| x.as_str());
        if let Some(img) = img {
            let entry = file_map.get(img);
            if entry.is_none() {
                file_map.insert(img.to_string(), file_from_str(img.to_string()));
            }
        }
    }
}

pub fn safe_select<'a>(
    selector: &mut impl FnMut(&str) -> Result<Vec<&'a Value>, JsonPathError>,
    expr: &str,
) -> Option<&'a Value> {
    selector(expr)
        .ok()
        .filter(|d| !Vec::is_empty(d))
        .as_mut()
        .and_then(|v| v.pop())
}

fn process_raw_fields(
    name: &Option<Vec<u8>>,
    symbol: &Option<Vec<u8>>,
) -> (Option<String>, Option<String>) {
    let name_result = name
        .as_ref()
        .and_then(|name| String::from_utf8(name.clone()).ok());
    let symbol_result = symbol
        .as_ref()
        .and_then(|symbol| String::from_utf8(symbol.clone()).ok());
    (name_result, symbol_result)
}

// https://github.com/solana-labs/token-list
pub fn add_legacy_token_datadata(content: Content, mint: &Vec<u8>, options: &Options) -> Content {
    let mint = bs58::encode(mint).into_string();
    let mut files = content.files.clone().unwrap_or(Vec::new());
    let mut links = content.links.clone().unwrap_or(HashMap::new());
    if files.len() > 0 || links.get("image").is_some() {
        content
    } else {
        if let Some(image) = LEGACY_TOKEN_IMAGES.get(&mint) {
            let file = file_from_str(image.to_string());
            files.push(file);
            links.insert(
                "image".to_string(),
                serde_json::Value::String(image.to_string()),
            );
        }
        enrich_files_with_cdn(&mut files, &options.cdn_prefix);
        Content {
            schema: content.schema,
            json_uri: content.json_uri,
            files: Some(files),
            metadata: content.metadata,
            links: Some(links),
            category: content.category,
        }
    }
}

pub fn v1_content_from_json(
    asset_data: &AssetMetadata,
    options: &Options,
) -> Result<Content, DbErr> {
    // todo -> move this to the bg worker for pre processing
    let json_uri = asset_data.metadata_url.clone();
    let metadata = &asset_data.metadata;
    let mut selector_fn = jsonpath_lib::selector(metadata);
    let mut chain_data_selector_fn = jsonpath_lib::selector(&asset_data.chain_data);
    let selector = &mut selector_fn;
    let chain_data_selector = &mut chain_data_selector_fn;
    let mut meta: MetadataMap = MetadataMap::new();
    if options.show_raw_data {
        let (name, symbol) = process_raw_fields(&asset_data.raw_name, &asset_data.raw_symbol);
        if let Some(name) = name {
            meta.set_item("name", name.into());
        }
        if let Some(symbol) = symbol {
            meta.set_item("symbol", symbol.into());
        }
    } else {
        let name = safe_select(chain_data_selector, "$.name");
        if let Some(name) = name {
            meta.set_item("name", name.clone());
        }
        let symbol = safe_select(chain_data_selector, "$.symbol");
        if let Some(symbol) = symbol {
            meta.set_item("symbol", symbol.clone());
        }
    }
    let token_standard = safe_select(chain_data_selector, "$.token_standard");
    if let Some(token_standard) = token_standard {
        meta.set_item("token_standard", token_standard.clone());
    }
    // The on-chain name is capped at 32 bytes; expose the untruncated off-chain
    // name alongside it. Unverified — sourced from creator-controlled JSON.
    let json_name = safe_select(selector, "$.name");
    if let Some(json_name) = json_name {
        meta.set_item("json_name", json_name.clone());
    }
    let desc = safe_select(selector, "$.description");
    if let Some(desc) = desc {
        meta.set_item("description", desc.clone());
    }
    let symbol = safe_select(selector, "$.attributes");
    if let Some(symbol) = symbol {
        match symbol {
            Value::String(s) => match serde_json::from_str(s) {
                // Handle the case where the attributes are a stringified JSON object.
                Ok(v) => {
                    meta.set_item("attributes", v);
                }
                Err(_) => {
                    meta.set_item("attributes", symbol.clone());
                }
            },
            _ => {
                meta.set_item("attributes", symbol.clone());
            }
        }
    }
    let mut links = HashMap::new();
    let link_fields = vec!["image", "animation_url", "external_url"];
    for f in link_fields {
        let l = safe_select(selector, format!("$.{}", f).as_str());
        if let Some(l) = l {
            links.insert(f.to_string(), l.to_owned());
        }
    }
    let category = safe_select(selector, "$.properties.category").cloned();
    let mut actual_files: HashMap<String, File> = HashMap::new();
    if let Some(files) = selector("$.properties.files[*]")
        .ok()
        .filter(|d| !Vec::is_empty(d))
    {
        for v in files.iter() {
            if v.is_object() {
                // Some assets don't follow the standard and specifiy 'url' instead of 'uri'
                let mut uri = v.get("uri");
                if uri.is_none() {
                    uri = v.get("url");
                }
                let mime_type = v.get("type");
                match (uri, mime_type) {
                    (Some(u), Some(m)) => {
                        if let Some(str_uri) = u.as_str() {
                            let file = if let Some(str_mime) = m.as_str() {
                                File {
                                    uri: Some(str_uri.to_string()),
                                    cdn_uri: None,
                                    mime: Some(str_mime.to_string()),
                                    quality: None,
                                    contexts: None,
                                }
                            } else {
                                warn!("Mime is not string: {:?}", m);
                                file_from_str(str_uri.to_string())
                            };
                            actual_files.insert(str_uri.to_string().clone(), file);
                        } else {
                            warn!("URI is not string: {:?}", u);
                        }
                    }
                    (Some(u), None) => {
                        let str_uri = serde_json::to_string(u).unwrap_or_else(|_| String::new());
                        actual_files.insert(str_uri.clone(), file_from_str(str_uri));
                    }
                    _ => {}
                }
            } else if v.is_string() {
                let str_uri = v.as_str().unwrap().to_string();
                actual_files.insert(str_uri.clone(), file_from_str(str_uri));
            }
        }
    }

    track_top_level_file(&mut actual_files, links.get("image"));
    track_top_level_file(&mut actual_files, links.get("animation_url"));

    let mut files: Vec<File> = actual_files.into_values().collect();

    // List the defined image file before the other files (if one exists).
    files.sort_by(|a, _: &File| match (a.uri.as_ref(), links.get("image")) {
        (Some(x), Some(y)) => {
            if x == y {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }
        _ => Ordering::Equal,
    });
    enrich_files_with_cdn(&mut files, &options.cdn_prefix);

    Ok(Content {
        schema: "https://schema.metaplex.com/nft1.0.json".to_string(),
        json_uri: replace_cloudfare_ipfs(&json_uri),
        files: Some(
            files
                .iter()
                .map(|file| {
                    let mut file = file.clone();
                    file.uri = file.uri.map(|uri| replace_cloudfare_ipfs(uri.as_str()));
                    file
                })
                .collect(),
        ),
        metadata: meta,
        links: Some(
            links
                .into_iter()
                .map(|(k, v)| {
                    let string = v.as_str();
                    match string {
                        Some(string) => {
                            (k, serde_json::Value::String(replace_cloudfare_ipfs(string)))
                        }
                        None => (k, v),
                    }
                })
                .collect(),
        ),
        category,
    })
}

// Cloudfare IPFS URIs are deprecated and should be replaced with ipfs.io
pub fn replace_cloudfare_ipfs(uri: &str) -> String {
    let new_uri = uri.replace("https://cloudflare-ipfs.com/ipfs/", "https://ipfs.io/ipfs/");
    new_uri.replace("https://cf-ipfs.com/ipfs/", "https://ipfs.io/ipfs/")
}

fn enrich_files_with_cdn(files: &mut Vec<File>, cdn_prefix: &Option<String>) {
    if let Some(cdn_prefix) = cdn_prefix {
        let cdn_options = ""; // Placeholder for potential future options

        files.iter_mut().for_each(|file| {
            if let (Some(uri), Some(mime)) = (&file.uri, &file.mime) {
                if mime.starts_with("image/") {
                    file.cdn_uri = Some(format!(
                        "{}/{}/{}",
                        cdn_prefix.trim_end_matches('/'),
                        cdn_options,
                        uri
                    ));
                }
            }
        });
    }
}

fn fix_missing_images(mut content: Content) -> Content {
    let mut links = content.links.clone().unwrap_or(HashMap::new());
    if content.json_uri.ends_with("png")
        || content.json_uri.ends_with("jpg")
        || content.json_uri.ends_with("jpeg")
    {
        if links.get("image").is_none() {
            links.insert(
                "image".to_string(),
                serde_json::Value::String(content.json_uri.clone()),
            );
        }
    }
    content.links = Some(links);
    content
}

pub fn get_content(
    asset: &asset::Model,
    data: &AssetMetadata,
    options: &Options,
) -> Result<Content, DbErr> {
    match asset.specification_version {
        Some(SpecificationVersions::V1) | Some(SpecificationVersions::V0) => {
            Ok(add_legacy_token_datadata(
                fix_missing_images(v1_content_from_json(data, options)?),
                &asset.id,
                options,
            ))
        }
        Some(_) => Err(DbErr::Custom("Version Not Implemented".to_string())),
        None => Err(DbErr::Custom("Specification version not found".to_string())),
    }
}

pub fn to_creators(creators: Vec<asset_creators::Model>) -> Vec<Creator> {
    creators
        .iter()
        .map(|a| Creator {
            address: bs58::encode(&a.creator).into_string(),
            share: a.share,
            verified: a.verified,
        })
        .collect()
}
pub fn filter_groups(groups: Vec<Group>, options: &Options) -> Result<Vec<Group>, DbErr> {
    let result: Vec<Group> = groups
        .iter()
        .filter_map(|model| {
            // Only show verification info if requested via display options.
            let verified = match options.show_unverified_collections {
                // Null verified indicates legacy data, meaning it is verified.
                true => Some(model.verified.unwrap_or(true)),
                false => None,
            };
            // Filter out items where group_value is None.
            model.group_value.clone().map(|group_value| Group {
                asset_id: model.asset_id.clone(),
                group_key: model.group_key.clone(),
                group_value: Some(group_value),
                verified,
                collection_metadata: None,
            })
        })
        .collect();
    Ok(result)
}

pub fn get_interface(asset: &asset::Model) -> Result<Interface, DbErr> {
    Ok(Interface::from((
        asset
            .specification_version
            .as_ref()
            .ok_or(DbErr::Custom("Specification version not found".to_string()))?,
        asset
            .specification_asset_class
            .as_ref()
            .ok_or(DbErr::Custom(
                "Specification asset class not found".to_string(),
            ))?,
        &(asset.supply as u64),
    )))
}

pub fn filter_non_null_fields(value: Option<&Value>) -> Option<Value> {
    match value {
        Some(Value::Null) => None,
        Some(Value::Object(map)) => {
            if map.values().all(|v| matches!(v, Value::Null)) {
                None
            } else {
                let filtered_map: Map<String, Value> = map
                    .into_iter()
                    .filter(|(_k, v)| !matches!(v, Value::Null))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                if filtered_map.is_empty() {
                    None
                } else {
                    Some(Value::Object(filtered_map))
                }
            }
        }
        _ => value.cloned(),
    }
}

//TODO -> impl custom error type
pub fn asset_to_rpc(
    last_indexed_slot: Option<u64>,
    asset: FullAsset,
    options: &Options,
) -> Result<RpcAsset, DbErr> {
    let FullAsset {
        asset,
        data,
        authorities,
        creators,
        groups,
        token_info,
        editions,
        group_definition,
    } = asset;
    let asset_id_str = bs58::encode(asset.clone().id).into_string();
    let rpc_creators = to_creators(creators);
    let rpc_groups = filter_groups(groups, options)?;
    // Hardcode interface if it's a BubblegumV2 asset that was indexed before the specific
    // interface was created.  We infer this by checking if the asset is compressed and has
    // a saved collection hash.
    let interface = if asset.compressed && asset.collection_hash.is_some() {
        Interface::MplBubblegumV2
    } else {
        get_interface(&asset)?
    };
    let content = get_content(&asset, &data, options)?;
    let mut chain_data_selector_fn = jsonpath_lib::selector(&data.chain_data);
    let chain_data_selector = &mut chain_data_selector_fn;
    let basis_points = safe_select(chain_data_selector, "$.primary_sale_happened")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let edition_nonce =
        safe_select(chain_data_selector, "$.edition_nonce").and_then(|v| v.as_u64());

    let mint_ext = filter_non_null_fields(asset.mint_extensions.as_ref());
    let mut supply = if let Some(edition_info) = editions {
        match EditionAccountType::from(edition_info.edition_type.as_str()) {
            EditionAccountType::Edition => Some(Supply {
                edition_nonce,
                edition_number: edition_info.edition,
                print_current_supply: edition_info.supply.unwrap_or(0),
                print_max_supply: edition_info.max_supply,
                master_edition_mint: edition_info.master_edition_mint,
            }),
            EditionAccountType::MasterEditionV1 | EditionAccountType::MasterEditionV2 => {
                Some(Supply {
                    edition_nonce,
                    print_current_supply: edition_info.supply.unwrap_or(0),
                    print_max_supply: edition_info.max_supply,
                    ..Default::default()
                })
            }
            _ => None,
        }
    } else {
        None
    };

    if supply.is_none() {
        supply = match interface {
            Interface::V1NFT | Interface::MplBubblegumV2 => Some(Supply {
                edition_nonce,
                print_current_supply: 0,
                print_max_supply: Some(0),
                ..Default::default()
            }),
            _ => None,
        };
    }

    let mpl_core_info = match interface {
        Interface::MplCoreAsset | Interface::MplCoreCollection | Interface::MplCoreGroup => {
            Some(MplCoreInfo {
                num_minted: asset.mpl_core_collection_num_minted,
                current_size: asset.mpl_core_collection_current_size,
                plugins_json_version: asset.mpl_core_plugins_json_version,
            })
        }
        _ => None,
    };

    Ok(RpcAsset {
        last_indexed_slot,
        interface: interface.clone(),
        id: asset_id_str,
        content: Some(content),
        authorities: Some(authorities),
        mutable: data.chain_mutability.into(),
        compression: Some(Compression {
            eligible: asset.compressible,
            compressed: asset.compressed,
            leaf_id: asset.nonce.unwrap_or(0 as i64),
            seq: asset.seq.unwrap_or(0 as i64),
            tree: asset
                .tree_id
                .map(|s| bs58::encode(s).into_string())
                .unwrap_or_default(),
            asset_hash: asset
                .leaf
                .map(|s| bs58::encode(s).into_string())
                .unwrap_or_default(),
            data_hash: asset
                .data_hash
                .map(|e| if asset.compressed { e.trim() } else { "" }.to_string())
                .unwrap_or_default(),
            creator_hash: asset
                .creator_hash
                .map(|e| if asset.compressed { e.trim() } else { "" }.to_string())
                .unwrap_or_default(),
            collection_hash: asset
                .collection_hash
                .map(|e| if asset.compressed { e.trim() } else { "" }.to_string()),
            asset_data_hash: asset
                .asset_data_hash
                .map(|e| if asset.compressed { e.trim() } else { "" }.to_string()),
            flags: asset.bubblegum_flags.and_then(|val| val.try_into().ok()),
        }),
        grouping: Some(rpc_groups),
        royalty: Some(Royalty {
            royalty_model: asset.royalty_target_type.into(),
            target: asset.royalty_target.map(|s| bs58::encode(s).into_string()),
            percent: (asset.royalty_amount as f64) * 0.0001,
            basis_points: asset.royalty_amount as u32,
            primary_sale_happened: basis_points,
            locked: false,
        }),
        creators: Some(rpc_creators),
        ownership: Ownership {
            frozen: asset.frozen,
            non_transferable: asset.non_transferable,
            delegated: asset.delegate.is_some(),
            delegate: asset.delegate.map(|s| bs58::encode(s).into_string()),
            ownership_model: asset.owner_type.into(),
            owner: asset
                .owner
                .map(|o| bs58::encode(o).into_string())
                .unwrap_or("".to_string()),
        },
        supply,
        uses: data.chain_data.get("uses").map(|u| Uses {
            use_method: u
                .get("use_method")
                .and_then(|s| s.as_str())
                .unwrap_or("Single")
                .to_string()
                .into(),
            total: u.get("total").and_then(|t| t.as_u64()).unwrap_or(0),
            remaining: u.get("remaining").and_then(|t| t.as_u64()).unwrap_or(0),
        }),
        burnt: asset.burnt,
        mint_extensions: mint_ext,
        token_info,
        group_definition,
        system: match options.show_system_metadata {
            true => Some(SystemInfo {
                created_at: asset.created_at.map(|dt| dt.with_timezone(&Utc)),
            }),
            false => None,
        },
        plugins: asset.mpl_core_plugins,
        unknown_plugins: asset.mpl_core_unknown_plugins,
        mpl_core_info,
        external_plugins: asset.mpl_core_external_plugins,
        unknown_external_plugins: asset.mpl_core_unknown_external_plugins,
        is_agent: match interface {
            Interface::MplCoreAsset | Interface::MplCoreCollection | Interface::MplCoreGroup => {
                Some(asset.is_agent)
            }
            _ => None,
        },
        agent_token: asset.agent_token.map(|t| bs58::encode(t).into_string()),
        asset_signer: asset.asset_signer.map(|s| bs58::encode(s).into_string()),
    })
}

pub fn asset_list_to_rpc(
    asset_list: Vec<FullAsset>,
    options: &Options,
) -> (Vec<RpcAsset>, Vec<AssetError>) {
    asset_list
        .into_iter()
        .fold((vec![], vec![]), |(mut assets, mut errors), asset| {
            let id = bs58::encode(asset.asset.id.clone()).into_string();
            match asset_to_rpc(None, asset, options) {
                Ok(rpc_asset) => assets.push(rpc_asset),
                Err(e) => errors.push(AssetError {
                    id,
                    error: e.to_string(),
                }),
            }
            (assets, errors)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_cloudfare_ipfs() {
        let test_cases = vec![
            (
                "https://cloudflare-ipfs.com/ipfs/QmTest123",
                "https://ipfs.io/ipfs/QmTest123",
            ),
            (
                "https://cf-ipfs.com/ipfs/QmAnotherTest456",
                "https://ipfs.io/ipfs/QmAnotherTest456",
            ),
            (
                "https://example.com/ipfs/QmNoChange789",
                "https://example.com/ipfs/QmNoChange789",
            ),
        ];

        for (input, expected) in test_cases {
            assert_eq!(replace_cloudfare_ipfs(input), expected);
        }
    }
}
