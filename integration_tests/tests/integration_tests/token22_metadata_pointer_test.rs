use function_name::named;
use das_api::api::{self, ApiContract};
use serial_test::serial;
use solana_sdk::pubkey::Pubkey;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_t22_metadata_pointer_metadata_first() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let mint: Pubkey = Pubkey::try_from("2a8JV3mV2V4QFZjv91VGmPHA3AD1erGNqmuHZFCsTxMB").unwrap();
    let metadata_account: Pubkey = Pubkey::try_from("GT22s89nU4iWFkNXj1Bw6uYhJJWDRPpShHt4Bk8f99Te").unwrap();

    index_account(&setup, metadata_account).await;
    index_account(&setup, mint).await;

    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    verify_asset(&setup, &name).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_metadata_pointer_mint_first() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let mint: Pubkey = Pubkey::try_from("2a8JV3mV2V4QFZjv91VGmPHA3AD1erGNqmuHZFCsTxMB").unwrap();
    let metadata_account: Pubkey = Pubkey::try_from("GT22s89nU4iWFkNXj1Bw6uYhJJWDRPpShHt4Bk8f99Te").unwrap();

    index_account(&setup, mint).await;
    index_account(&setup, metadata_account).await;

    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    verify_asset(&setup, &name).await;
}

async fn verify_asset(setup: &TestSetup, test_name: &str) {
    let request = api::GetAsset {
        id: "2a8JV3mV2V4QFZjv91VGmPHA3AD1erGNqmuHZFCsTxMB".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();

    assert_eq!(
        response.interface,
        digital_asset_types::rpc::Interface::V1NFT,
        "Token22 NFT with metadata_pointer should have V1_NFT interface, not Custom"
    );

    assert!(response.content.is_some(), "Content should be present");

    assert_eq!(
        response.ownership.ownership_model,
        digital_asset_types::rpc::OwnershipModel::Single,
        "NFT should have single ownership model"
    );
    assert!(
        !response.ownership.owner.is_empty(),
        "NFT should have an owner"
    );

    let owner_address = response.ownership.owner.clone();
    let search_request = api::SearchAssets {
        owner_address: Some(owner_address.clone()),
        ..api::SearchAssets::default()
    };
    let search_response = setup.das_api.search_assets(search_request).await.unwrap();

    assert!(search_response.total > 0, "searchAssets by owner should return results");

    let found = search_response.items.iter().any(|item| {
        item.id == "2a8JV3mV2V4QFZjv91VGmPHA3AD1erGNqmuHZFCsTxMB"
    });
    assert!(
        found,
        "searchAssets by owner {} should include asset 2a8JV3mV2V4QFZjv91VGmPHA3AD1erGNqmuHZFCsTxMB",
        owner_address
    );

    insta::assert_json_snapshot!(test_name, response);
}
