use std::str::FromStr;

use digital_asset_types::rpc::options::Options;
use function_name::named;

use das_api::api::{self, ApiContract};

use serial_test::serial;
use solana_sdk::{pubkey::Pubkey, signature::Signature};

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_t22_native_metadata() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX").unwrap();
    index_account(&setup, mint).await;
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    let request = api::GetAsset {
        id: "Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_parsing_fix() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("XsqE9cRRpzxcGKDXj1BJ7Xmg4GRhZoyY1KpmGSxAWT2").unwrap();
    index_account(&setup, mint).await;
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    let request = api::GetAsset {
        id: "XsqE9cRRpzxcGKDXj1BJ7Xmg4GRhZoyY1KpmGSxAWT2".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response, {
        ".token_info.supply" => "[supply]",
    });
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_burn() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let mint: Pubkey = Pubkey::try_from("Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX").unwrap();
    index_account(&setup, mint).await;
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;
    index_account_burn(&setup, mint, get_max_slot()).await;
    let request = r#"
    {
        "id": "Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX"
    }
    "#;

    let request: api::GetAsset = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_native_metadata_2() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("FumKKEEuQj8ZHqJi7Pj7uVCmjpGN5iv4nZdEeqPTuRM1").unwrap();
    index_account(&setup, mint).await;
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    let request = api::GetAsset {
        id: "FumKKEEuQj8ZHqJi7Pj7uVCmjpGN5iv4nZdEeqPTuRM1".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_group_pointer() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("99dwsr1CWxKfcfHdBTjrMetw1sm2ec2sahBB2kryZHFx").unwrap();
    index_account(&setup, mint).await;

    let request = api::GetAsset {
        id: "99dwsr1CWxKfcfHdBTjrMetw1sm2ec2sahBB2kryZHFx".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_member_pointer() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("A1Ypyq4BHkS1R1DnYiRV9HxC4joWDNgFcmaCpMgJjppK").unwrap();
    index_account(&setup, mint).await;

    let request = api::GetAsset {
        id: "A1Ypyq4BHkS1R1DnYiRV9HxC4joWDNgFcmaCpMgJjppK".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_get_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let accounts = vec![
        "DVg4Y4ZEQ7EaiV2dwXZ21E7aWhizu6ZQ9ZA3jZd263Xb", // NFT mint
        "F51svAfxU5Zg6cE9LJ8xCcQayED2BvaYdUKEjLmJ5zPL", // NFT TA
        "6PHEpno6fuPCeraN3U1px3GxaN6FAqYx9C6Tz1axAd6K", // Fungible mint
        "9mnkrbd6T7nzRwp3pX6fFnwQDtYhF7TnkciodcbyghVg", // Fungible TA
    ];
    for account in accounts {
        let account: Pubkey = Pubkey::try_from(account).unwrap();
        index_account(&setup, account).await;
    }

    let request = api::GetAsset {
        id: "DVg4Y4ZEQ7EaiV2dwXZ21E7aWhizu6ZQ9ZA3jZd263Xb".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "nft"), response);

    let request = api::GetAsset {
        id: "6PHEpno6fuPCeraN3U1px3GxaN6FAqYx9C6Tz1axAd6K".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "fungible"), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_get_assets_by_authority() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let accounts = vec![
        "DVg4Y4ZEQ7EaiV2dwXZ21E7aWhizu6ZQ9ZA3jZd263Xb",
        "F51svAfxU5Zg6cE9LJ8xCcQayED2BvaYdUKEjLmJ5zPL",
    ];
    for account in accounts {
        let account: Pubkey = Pubkey::try_from(account).unwrap();
        index_account(&setup, account).await;
    }

    let request = api::GetAssetsByAuthority {
        authority_address: "92ivrruXpkUh7zoDBWL1HbVJX8Hd16x5p6xxmdLBFimF".to_string(), // metadata auth
        ..api::GetAssetsByAuthority::default()
    };
    let response = setup
        .das_api
        .get_assets_by_authority(request.clone())
        .await
        .unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "metadata"), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_get_assets_by_owner() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_transaction(
        &setup,
        Signature::from_str("595h6cA7UMgHkuNwRs8WHWyAaGeNx6YvDBG54BPqZ63w5sEfHrmstLFwHCmyZunbacGiicWcwDbmXkujoZqCDbMh").unwrap()
    )
    .await;
    let accounts = vec![
        "DVg4Y4ZEQ7EaiV2dwXZ21E7aWhizu6ZQ9ZA3jZd263Xb",
        "F51svAfxU5Zg6cE9LJ8xCcQayED2BvaYdUKEjLmJ5zPL",
        "6PHEpno6fuPCeraN3U1px3GxaN6FAqYx9C6Tz1axAd6K",
        "9mnkrbd6T7nzRwp3pX6fFnwQDtYhF7TnkciodcbyghVg",
        "So11111111111111111111111111111111111111112",
        "vFBy94ZGkNpr3svH2D2b9xwx3U2pLArzrpSqzHKSdDy",
    ];
    for account in accounts {
        let account: Pubkey = Pubkey::try_from(account).unwrap();
        index_account(&setup, account).await;
    }

    let request = api::GetAssetsByOwner {
        owner_address: "ExA7MKcvVcQU39paNjpCQb7QQcg6QBm9uQbicwpKZZ3b".to_string(),
        ..api::GetAssetsByOwner::default()
    };
    let response = setup
        .das_api
        .get_assets_by_owner(request.clone())
        .await
        .unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "nft"), response);

    let request = api::GetAssetsByOwner {
        owner_address: "2BPEQsnGFx3sdQrW3AjXpzxtjNMZy5kokh1QUjsPN38N".to_string(),
        options: Some(Options {
            show_fungible: false,
            ..Options::default()
        }),
        ..api::GetAssetsByOwner::default()
    };
    let response = setup
        .das_api
        .get_assets_by_owner(request.clone())
        .await
        .unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "show-fungible-false"), response);

    let request = api::GetAssetsByOwner {
        owner_address: "2BPEQsnGFx3sdQrW3AjXpzxtjNMZy5kokh1QUjsPN38N".to_string(),
        options: Some(Options {
            show_fungible: true,
            ..Options::default()
        }),
        ..api::GetAssetsByOwner::default()
    };
    let response = setup
        .das_api
        .get_assets_by_owner(request.clone())
        .await
        .unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "show-fungible-true"), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_marking_fungible_as_nfts_bug() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("HbxiDXQxBKMNJqDsTavQE7LVwrTR36wjV2EaYEqUw6qH").unwrap();
    index_account(&setup, mint).await;
    // We also incorrectly marked this token account as the owner. Fungibles should not have owners.
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    let request = api::GetAsset {
        id: "HbxiDXQxBKMNJqDsTavQE7LVwrTR36wjV2EaYEqUw6qH".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response, {
        ".token_info.supply" => "[supply]",
    });

    let request = api::SearchAssets {
        owner_address: Some("E8E6GvyCpbGu7YSFxfhTXGx6SW4VhzVmxWh3gbrgXZNd".to_string()),
        token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::Fungible),
        ..api::SearchAssets::default()
    };
    let response = setup.das_api.search_assets(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-search", name.clone()), response, {
        ".items[].token_info.supply" => "[supply]",
    });
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_search_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let accounts = vec![
        "DVg4Y4ZEQ7EaiV2dwXZ21E7aWhizu6ZQ9ZA3jZd263Xb", // NFT mint
        "F51svAfxU5Zg6cE9LJ8xCcQayED2BvaYdUKEjLmJ5zPL", // NFT TA
    ];
    for account in accounts {
        let account: Pubkey = Pubkey::try_from(account).unwrap();
        index_account(&setup, account).await;
    }

    let request = api::SearchAssets {
        owner_address: Some("ExA7MKcvVcQU39paNjpCQb7QQcg6QBm9uQbicwpKZZ3b".to_string()),
        ..api::SearchAssets::default()
    };
    let response = setup.das_api.search_assets(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_unsanitised_metadata() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("6XDmP7QwL3HkxW9dei82rHC5zw69ugucxc4GQ6uvmkCf").unwrap();
    index_account(&setup, mint).await;

    let request = api::GetAsset {
        id: "6XDmP7QwL3HkxW9dei82rHC5zw69ugucxc4GQ6uvmkCf".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_with_null_terminated_uri() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("AWXzZ1NV2qQqffafUsrkfT3QxHaUtS4iEx75MoE5YFLc").unwrap();
    index_account(&setup, mint).await;

    let request = api::GetAsset {
        id: "AWXzZ1NV2qQqffafUsrkfT3QxHaUtS4iEx75MoE5YFLc".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_t22_pausable_and_scaled_ui_amount() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    // This mint has 8 extensions including pausable_config and scaled_ui_amount_config
    let mint: Pubkey = Pubkey::try_from("XsEH7wWfJJu2ZT3UCFeVfALnVA6CP5ur7Ee11KmzVpL").unwrap();
    index_account(&setup, mint).await;
    let token_account = cached_fetch_largest_token_account_id(&setup.client, mint).await;
    index_account(&setup, token_account).await;

    let request = api::GetAsset {
        id: "XsEH7wWfJJu2ZT3UCFeVfALnVA6CP5ur7Ee11KmzVpL".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();

    // Verify that the response contains extension data
    assert!(response.mint_extensions.is_some(), "mint_extensions should be present");
    let mint_extensions = response.mint_extensions.as_ref().unwrap();

    // Verify pausable_config is present and not null
    assert!(
        mint_extensions.get("pausable_config").is_some(),
        "pausable_config should be present in mint_extensions"
    );
    let pausable_config = &mint_extensions["pausable_config"];
    assert!(
        !pausable_config.is_null(),
        "pausable_config should not be null"
    );

    // Verify scaled_ui_amount_config is present and not null
    assert!(
        mint_extensions.get("scaled_ui_amount_config").is_some(),
        "scaled_ui_amount_config should be present in mint_extensions"
    );
    let scaled_config = &mint_extensions["scaled_ui_amount_config"];
    assert!(
        !scaled_config.is_null(),
        "scaled_ui_amount_config should not be null"
    );

    // Verify all 8 expected extensions are present
    let expected_extensions = [
        "pausable_config",
        "scaled_ui_amount_config",
        "default_account_state",
        "permanent_delegate",
        "metadata_pointer",
        "transfer_hook",
        "confidential_transfer_mint",
        "metadata",
    ];

    for ext in expected_extensions.iter() {
        assert!(
            mint_extensions.get(*ext).is_some(),
            "Extension {} should be present",
            ext
        );
    }

    // Snapshot the full response for regression testing
    insta::assert_json_snapshot!(name.clone(), response, {
        ".token_info.supply" => "[supply]",
    });
}
