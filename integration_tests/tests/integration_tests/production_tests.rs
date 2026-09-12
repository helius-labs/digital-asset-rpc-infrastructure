use digital_asset_types::rpc::{
    filter::{AssetSortBy, AssetSortDirection, AssetSorting},
    options::{Options, SearchAssetsOptions},
    Asset,
};
use function_name::named;

use das_api::api::{self, ApiContract};

use serial_test::serial;
use solana_sdk::bs58;

use super::common::*;

#[tokio::test]
#[serial]
#[ignore]
#[named]
async fn test_burns_bug() {
    let name = trim_test_name(function_name!());
    let das_config = das_api::config::Config {
        database_urls: Some(
            std::env::var("READONLY_DATABASE_URL")
                .expect("Expected READONLY_DATABASE_URL to be set."),
        ),
        server_port: 8000,
        ..das_api::config::Config::default()
    };
    let das_api = das_api::api::DasApi::from_config(das_config).await.unwrap();
    let request = api::SearchAssets {
        owner_address: Some("7LYZX8SHVZRyc1f4kH1BU2yM3MbKsAQ3LHXHtdVXrzwL".to_string()),
        token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),
        ..api::SearchAssets::default()
    };
    let response = das_api.search_assets(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[ignore]
#[named]
async fn test_faulty_pagination() {
    let name = trim_test_name(function_name!());

    let das_config = das_api::config::Config {
        database_urls: Some(
            std::env::var("READONLY_DATABASE_URL")
                .expect("Expected READONLY_DATABASE_URL to be set."),
        ),
        server_port: 8000,
        ..das_api::config::Config::default()
    };
    let das_api = das_api::api::DasApi::from_config(das_config).await.unwrap();
    let mut cursor = None;
    let mut all_assets = Vec::new();
    loop {
        let request = api::SearchAssets {
            owner_address: Some("7Y6feARE32RbDJGws99J9tGxSS3dfqY6iuK2kT4Lw8Ut".to_string()),
            token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),
            options: Some(SearchAssetsOptions {
                show_zero_balance: true,
                ..Default::default()
            }),
            sort_by: Some(AssetSorting {
                sort_by: AssetSortBy::Id,
                sort_direction: Some(AssetSortDirection::Asc),
            }),
            cursor: cursor.clone(),
            ..api::SearchAssets::default()
        };
        let response = das_api.search_assets(request.clone()).await.unwrap();
        insta::assert_json_snapshot!(name.clone(), response);
        all_assets.extend(response.items);
        println!("Num assets: {}", all_assets.len());
        cursor = response.cursor;
    }
}

#[tokio::test]
#[serial]
#[ignore]
#[named]
async fn test_u64_balance_issue() {
    let name = trim_test_name(function_name!());

    let das_config = das_api::config::Config {
        database_urls: Some(
            std::env::var("READONLY_DATABASE_URL")
                .expect("Expected READONLY_DATABASE_URL to be set."),
        ),
        server_port: 8000,
        ..das_api::config::Config::default()
    };
    let das_api = das_api::api::DasApi::from_config(das_config).await.unwrap();
    let request = api::GetAssetsByOwner {
        owner_address: "GQBzWUfDWbdnXMaQztxDELmhUKystypnANLkq6zJ6Wzn".to_string(),
        options: Some(Options {
            show_fungible: true,
            show_zero_balance: true,
            ..Default::default()
        }),
        ..api::GetAssetsByOwner::default()
    };
    let response = das_api.get_assets_by_owner(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[ignore]
#[named]
async fn test_attribute_string_escaping() {
    let name = trim_test_name(function_name!());

    let das_config = das_api::config::Config {
        database_urls: Some(
            std::env::var("READONLY_DATABASE_URL")
                .expect("Expected READONLY_DATABASE_URL to be set."),
        ),
        server_port: 8000,
        ..das_api::config::Config::default()
    };
    let das_api = das_api::api::DasApi::from_config(das_config).await.unwrap();
    let request = api::GetAsset {
        id: "G75Fuvdqnixj6XFbBHRSLFb6jTnvW421oXMhC6nQBLgP".to_string(),
        ..api::GetAsset::default()
    };
    let response = das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[ignore]
async fn test_duplicate_response() {
    setup_logging();
    let das_config = das_api::config::Config {
        database_urls: Some(
            std::env::var("READONLY_DATABASE_URL")
                .expect("Expected READONLY_DATABASE_URL to be set."),
        ),
        server_port: 8000,
        ..das_api::config::Config::default()
    };
    let das_api = das_api::api::DasApi::from_config(das_config).await.unwrap();

    let mut cursor = None;
    let mut assets: Vec<Asset> = Vec::new();

    loop {
        let request = api::GetAssetsByOwner {
            owner_address: "CtWERMNKhsMy2SX564sCPWVMd9LND8aa4jL3YJzdurbp".to_string(),
            options: Some(Options {
                show_fungible: true,
                show_grand_total: true,
                ..Default::default()
            }),
            cursor: cursor.clone(),
            ..api::GetAssetsByOwner::default()
        };
        let response: digital_asset_types::rpc::response::AssetList =
            das_api.get_assets_by_owner(request.clone()).await.unwrap();

        assets.extend(response.items.clone());
        cursor = response.cursor.clone();
        if response.items.len() < 1000 {
            break;
        }
    }
    let asset_ids = assets
        .iter()
        .map(|asset| bs58::decode(asset.id.clone()).into_vec().unwrap())
        .collect::<Vec<Vec<u8>>>();

    // Verify that asset ids are sorted in descending order
    for i in 1..asset_ids.len() {
        assert!(
            asset_ids[i - 1] > asset_ids[i],
            "{i} {:?} {:?} {:?} {:?}",
            assets[i - 1].id,
            assets[i].id,
            asset_ids[i - 1],
            asset_ids[i]
        );
    }
}
