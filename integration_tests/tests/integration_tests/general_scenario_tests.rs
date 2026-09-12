use digital_asset_types::dao::scopes::asset::{find_associated_token_address, TokenType};
use digital_asset_types::rpc::response::NativeBalance;
use function_name::named;
use nft_ingester::tasks::price::update_sol_token_price;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::str::FromStr;
use std::sync::Arc;

use das_api::api::{self, ApiContract};
use digital_asset_types::dao::{asset_creators, price};
use digital_asset_types::rpc::filter::{AssetSortBy, AssetSortDirection, AssetSorting};
use digital_asset_types::rpc::options::{Options, SearchAssetsOptions};
use digital_asset_types::rpc::NotFilter;
use itertools::Itertools;
use migration::sea_orm::{ConnectionTrait, EntityTrait};
use mpl_token_metadata::accounts::Metadata;

use mpl_token_metadata::types::Creator;
use nft_ingester::program_transformers::account_closure::{parse_account_type, AccountType};
use sea_orm::{ActiveModelTrait, DbBackend, QueryTrait, Set};
use serial_test::serial;
use solana_sdk::pubkey::Pubkey;
use spl_token::ID;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_asset_parsing() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("843gdpsTE4DoJz3ZoBsEjAqT8UgAcyF5YojygGgGZE1f").unwrap();
    index_nft(&setup, mint).await;
    let request = api::GetAsset {
        id: mint.to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_nft_burns() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let mint: Pubkey = Pubkey::try_from("843gdpsTE4DoJz3ZoBsEjAqT8UgAcyF5YojygGgGZE1f").unwrap();
    let nft_accounts = get_nft_accounts(&setup, mint).await;
    index_nft_accounts(&setup, nft_accounts).await;

    match parse_account_type(&setup.db, nft_accounts.token.to_bytes().to_vec())
        .await
        .unwrap()
    {
        AccountType::TokenAccount(_) => {}
        other => panic!("Unexpected account type: {:?}", other),
    }

    match parse_account_type(&setup.db, nft_accounts.metadata.to_bytes().to_vec())
        .await
        .unwrap()
    {
        AccountType::NFT(_) => {}
        other => panic!("Unexpected account type: {:?}", other),
    }

    match parse_account_type(&setup.db, Pubkey::new_unique().to_bytes().to_vec())
        .await
        .unwrap()
    {
        AccountType::Unknown => {}
        other => panic!("Unexpected account type: {:?}", other),
    }

    index_account_burn(&setup, Metadata::find_pda(&mint).0, get_max_slot()).await;
    let request = api::GetAsset {
        id: mint.to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_asset_parsing_all_account_order_permutations() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let mint: Pubkey = Pubkey::try_from("843gdpsTE4DoJz3ZoBsEjAqT8UgAcyF5YojygGgGZE1f").unwrap();
    let nft_accounts = get_nft_accounts(&setup, mint).await;

    let accounts = [nft_accounts.mint, nft_accounts.metadata, nft_accounts.token];
    let account_permutations: Vec<Vec<&Pubkey>> = accounts
        .iter()
        .permutations(accounts.len())
        .collect::<Vec<_>>();

    for account_order in account_permutations {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        for account in account_order {
            index_account(&setup, *account).await;
        }
        let request = api::GetAsset {
            id: mint.to_string(),
            ..api::GetAsset::default()
        };
        let response = setup.das_api.get_asset(request).await.unwrap();
        insta::assert_json_snapshot!(name.clone(), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_creators_reordering() {
    // This test covers a failure scenario we found in production where an NFT changed
    // the positions of its creators, leading to conflict errors in the DB.
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let asset_id = "ANt9HygtvFmFJ1UcAHFLnM62JJWjk8fujMzjGfpKBfzk";
    let asset_pubkey = Pubkey::from_str(asset_id).unwrap();
    apply_migrations_and_delete_data(setup.db.clone()).await;

    // Insert the original creators
    let original_creators = vec![
        Creator {
            address: Pubkey::from_str("8Jhy62JeG4rgPu4Q2tn3Q3eZ8XUZmHhYDKpVJkQ8RFhe").unwrap(),
            verified: true,
            share: 0,
        },
        Creator {
            address: Pubkey::from_str("9sJ3GKyTpBaNJ9CVFV6DecV556G1jU9L32kJASxzWsQA").unwrap(),
            verified: false,
            share: 10,
        },
        Creator {
            address: Pubkey::from_str("yX9uyojU5uwBnDUJg5wX1n7T4w7KyU6r9brszXX2yKa").unwrap(),
            verified: false,
            share: 10,
        },
        Creator {
            address: Pubkey::from_str("BDaobvsTU8Eu3R4sx1vLufKiToaZL3MDTHxPgHgvGWC7").unwrap(),
            verified: false,
            share: 10,
        },
        Creator {
            address: Pubkey::from_str("F9xfmpggwgqH7ASZzNre8TxZztZwCogPBE8aQCNBLkBn").unwrap(),
            verified: false,
            share: 70,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, c)| asset_creators::ActiveModel {
        asset_id: Set(asset_pubkey.to_bytes().to_vec()),
        position: Set(i as i16),
        creator: Set(c.address.to_bytes().to_vec()),
        share: Set(c.share as i32),
        verified: Set(c.verified),
        slot_updated: Set(Some(0)),
        seq: Set(Some(0)),
        ..Default::default()
    })
    .collect::<Vec<_>>();
    setup
        .db
        .execute(asset_creators::Entity::insert_many(original_creators).build(DbBackend::Postgres))
        .await
        .unwrap();

    // Index the current NFT.
    index_nft(&setup, asset_pubkey).await;

    // Verify creators
    let request = api::GetAsset {
        id: asset_id.to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_feat_grand_total() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    let seeds = seed_txns([
        "2ozXvc1C9yECahVv5HDXQVei7Uvjeus32z1kngqs2gywxmtRKg8GsWHrEdcFCnT7LrbStNMKF9rV2wwXEvHMe9vd",
        "2DCfkrQibbTy98cmhnaHb1yURfYBEVkmPXhuq4uBHNqNXZYQLev45wdBji2KNJ7L1jzbTBnk8i9J6rAjtCJ1NXRr",
        "5EQqsX6kfjDxLDwXu4zjAvazdXTWB4iZw5kxyMpEZXf14u93Lw4ck6KWA13H5CWZby65WwvR9TZPX86rWzGKEaPd",
    ]);

    for events in seeds.iter().permutations(seeds.len()).collect::<Vec<_>>() {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        index_seed_events(&setup, events).await;
        let request = api::SearchAssets {
            grouping: Some((
                "collection".to_string(),
                "7yvu7Ut65f9mfUyk9HQL7EEqzNfyyp2nbBuEjvY4fyqZ".to_string(),
            )),
            sort_by: Some(AssetSorting {
                sort_by: AssetSortBy::Updated,
                sort_direction: Some(AssetSortDirection::Asc),
            }),

            limit: Some(1),
            page: Some(1),
            options: Option::Some(SearchAssetsOptions {
                show_grand_total: true,
                ..SearchAssetsOptions::default()
            }),
            ..api::SearchAssets::default()
        };
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(setup.name.clone(), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_feat_search_assets_collections() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let seeds = seed_nfts([
        "JEJcxTTqXqzQj1QnaGTSVDrTyQhX5tyfE72yMtiHd1sc",
        "JEH83CTocEdB51sTR2jG357UXZzH2fZokLdgEEKTHkLo",
        "JEFgYVM4zRYpyV95aTd5dkGMHHRaERcLbrCa4629YRsW",
        "JDzmRZNoRw8ANxNkF7yj8ZSJ2FzQvEqw64WFaGyL1iyD",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;
    for (request, individual_test_name) in [
        (
            api::SearchAssets {
                owner_address: Some("4zdNGgAtFsW1cQgHqkiWyRsxaAgxrSRRynnuunxzjxue".to_string()),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "no_collection",
        ),
        (
            api::SearchAssets {
                owner_address: Some("4zdNGgAtFsW1cQgHqkiWyRsxaAgxrSRRynnuunxzjxue".to_string()),
                collections: Some(vec![
                    "3wsSCebzay39pghBbwzdaRytPCHbAmb33VDMP3p8XF4i".to_string()
                ]),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "single_collection",
        ),
        (
            api::SearchAssets {
                owner_address: Some("4zdNGgAtFsW1cQgHqkiWyRsxaAgxrSRRynnuunxzjxue".to_string()),
                collections: Some(vec![
                    "3wsSCebzay39pghBbwzdaRytPCHbAmb33VDMP3p8XF4i".to_string(),
                    "CWPJqBML6768gHpffCF6Q2mcJYWRb3kYBAmFCq5WQjPk".to_string(),
                ]),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "two_collections",
        ),
        (
            api::SearchAssets {
                owner_address: Some("4zdNGgAtFsW1cQgHqkiWyRsxaAgxrSRRynnuunxzjxue".to_string()),
                collections: Some(vec![
                    "3wsSCebzay39pghBbwzdaRytPCHbAmb33VDMP3p8XF4i".to_string(),
                    "CWPJqBML6768gHpffCF6Q2mcJYWRb3kYBAmFCq5WQjPk".to_string(),
                    "6rFsCBs19xSaBRGCwTFy877gXHwkHozNa1sQ4f9jYwNy".to_string(),
                ]),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "three_collections",
        ),
    ] {
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_feat_search_assets_negation() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    let seeds = seed_txns([
        "3JwBbH1wmcn3Z42kdDBLmXT2QWnmDnGWSz9sYC9NjRoypBLzqa76BArvk7CPbBXUCa92WqKn5RzFX7N5Hfr6RVkc",
        "2LF96xsNxVi2jic94yyugQCUMdLeDJmaZ6DPaE95qKBYyrxxHGQhVJEHtUHMyUp8vDP5n333S5tSQzfiSMsfyaGd",
    ]);

    for events in seeds.iter().permutations(seeds.len()).collect::<Vec<_>>() {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        index_seed_events(&setup, events).await;
        for (request, individual_test_name) in [
            (
                api::SearchAssets {
                    grouping: Some((
                        "collection".to_string(),
                        "CLXKiX3tyaenUK5GXiNUgUFRTfp4YPqYGXSLXUxQ7G8A".to_string(),
                    )),
                    limit: Some(1),
                    page: Some(1),
                    options: Option::Some(SearchAssetsOptions {
                        show_grand_total: true,
                        ..SearchAssetsOptions::default()
                    }),
                    not: Some(NotFilter {
                        owners: Some(vec![
                            "CNFTteLrtNChJfqPtPxn8NSJJi9JebtvPbL4egUCLr8v".to_string()
                        ]),
                        ..NotFilter::default()
                    }),
                    sort_by: Some(AssetSorting {
                        sort_by: AssetSortBy::None,
                        sort_direction: None,
                    }),
                    ..api::SearchAssets::default()
                },
                "with_negation",
            ),
            (
                api::SearchAssets {
                    grouping: Some((
                        "collection".to_string(),
                        "CLXKiX3tyaenUK5GXiNUgUFRTfp4YPqYGXSLXUxQ7G8A".to_string(),
                    )),
                    limit: Some(1),
                    page: Some(1),
                    options: Option::Some(SearchAssetsOptions {
                        show_grand_total: true,
                        ..SearchAssetsOptions::default()
                    }),
                    sort_by: Some(AssetSorting {
                        sort_by: AssetSortBy::Updated,
                        sort_direction: Some(AssetSortDirection::Asc),
                    }),
                    ..api::SearchAssets::default()
                },
                "without_negation",
            ),
        ] {
            let response = setup.das_api.search_assets(request).await.unwrap();
            insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
        }
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_feat_show_unverified_collections() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let seeds = seed_txns([
        "3pt4TGGrz8BfXESR1Kj1eTqXv1eZPQWoXrTSsqEocS9YdWuV2LVN4sfv7ejynwWWCVAE4KVE2WcGascKuLc6cn2P",
    ]);

    for events in seeds.iter().permutations(seeds.len()).collect::<Vec<_>>() {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        index_seed_events(&setup, events).await;
        let request: api::GetAssetsByOwner = serde_json::from_str(
            r#"{
            "ownerAddress": "HZLGz18PCxD1bZgarBaQL7FGPswyLv65hzMn6Uj179WC",
            "displayOptions": {
                "showUnverifiedCollections": true
            },
            "page": 1,
            "limit": 10
        }"#,
        )
        .unwrap();
        let response = setup.das_api.get_assets_by_owner(request).await.unwrap();
        insta::assert_json_snapshot!(setup.name.clone(), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_pricing_incorporation() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let owner1_address = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";
    let owner2_address = "FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5";

    let fungibles = [
        ("jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL", "JITO"),
        ("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R", "RAYDIUM"),
    ];

    let mut seeds = vec![];

    for (i, (fungible, symbol)) in fungibles.iter().enumerate() {
        let price = i + 2; // Arbitrary price
        let price_model = price::ActiveModel {
            mint: Set(Pubkey::from_str(fungible).unwrap().to_bytes().to_vec()),
            price: Set(Some(price as f32)),
            symbol: Set(Some(symbol.to_string())),
        };
        price_model.insert(setup.db.as_ref()).await.unwrap();
    }

    for (fungible, _symbol) in fungibles.into_iter() {
        seeds.push(seed_account(fungible));
    }

    for address in [owner1_address, owner2_address].into_iter() {
        for (fungible, _symbol) in fungibles.into_iter() {
            let token_account = find_associated_token_address(
                Pubkey::try_from(address).unwrap(),
                Pubkey::try_from(fungible).unwrap(),
                Some(ID),
            )
            .unwrap();
            seeds.push(seed_account(&token_account.to_string()));
        }
    }

    let nft_seeds = [
        // NFT for owner1
        seed_nft("NVguVfNBPoDUncBjCPJvnwvAAVPRcJRC42JXZRFNZ2Q"),
        // Mints 94WpxCmtoutLHy43Pdz185SNyVqybc1wVAWsXEJ78dUk for owner1
        seed_txn("TwCEgDgp56v4tXJiWCuW5dMjvXLrwMnsF5k7PpyvAUDhc6wSrb55rsBh9jJHKvpqpCKdWSbv8LRuCfjPYppesAY"),
        // Mints 2179Qt55o2G67RWXhKuJ2FfK8GQmEBQsoUKjBPf5xRsC for owner2
        seed_txn("5xd55Y9LmgcN4Ddhdw9ACsafhUxq4XzsdoCDxHZVuj1VZX36Jkd2qRHV2hERX7pBmEkeVXFeLKBHio6D2kP4yXgB"),
    ];

    let seeds = seeds
        .iter()
        .chain(nft_seeds.iter())
        .map(|s| s.clone())
        .collect::<Vec<_>>();

    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    // We specifically test show zero balance to regression test a bug that caused an outage.
    // More info here: https://helius-api.slack.com/archives/C04PLR7UD2P/p1707807088882549
    let show_zero_balance_options = [true, false];
    let token_type_options = [
        TokenType::Fungible,
        TokenType::NonFungible,
        TokenType::All,
        TokenType::CompressedNft,
        TokenType::RegularNft,
    ];
    for show_zero_balance in show_zero_balance_options.iter() {
        for token_type in token_type_options.iter() {
            let request = api::SearchAssets {
                owner_address: Some(owner1_address.to_string()),
                options: Some(SearchAssetsOptions {
                    show_zero_balance: *show_zero_balance,
                    ..Default::default()
                }),
                token_type: Some(token_type.clone()),
                ..api::SearchAssets::default()
            };
            let response = setup.das_api.search_assets(request).await.unwrap();
            insta::assert_json_snapshot!(
                format!(
                    "{}-show-zero-balance-{}-{}",
                    setup.name.clone(),
                    show_zero_balance,
                    serde_json::to_string(&token_type).unwrap()
                ),
                response
            );
        }
    }
}

#[tokio::test]
#[serial]
async fn test_search_assets_show_fungible_noop() {
    let request: Result<api::SearchAssets, serde_json::Error> = serde_json::from_str(
        r#"{
        "ownerAddress": "11111111111111111111111111111111",
        "options": {
            "showFungible": true
        }
    }"#,
    );

    assert!(request.is_err());
}

fn round_to_log_100(value: f64) -> f64 {
    let log_value = value.log(100.0);
    let rounded_log_value = log_value.round();
    let result = 100.0_f64.powf(rounded_log_value);
    result
}

fn smooth_native_balance(balance: NativeBalance) -> NativeBalance {
    NativeBalance {
        lamports: round_to_log_100(balance.lamports as f64).round() as u64,
        price_per_sol: round_to_log_100(balance.price_per_sol),
        total_price: round_to_log_100(balance.total_price),
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_search_assets_show_native_balance() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    //this is a test
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let nft_seeds = seed_nfts(["843gdpsTE4DoJz3ZoBsEjAqT8UgAcyF5YojygGgGZE1f"]);
    index_seed_events(&setup, nft_seeds.iter().collect_vec()).await;

    update_sol_token_price(setup.db.clone(), 120.0)
        .await
        .unwrap();

    for (request, name) in [
        (
            r#"{
        "ownerAddress": "BzbdvwEkQKeghTY53aZxTYjUienhdbkNVkgrLV6cErke",
        "options": {
            "showNativeBalance": true
        }
    }"#,
            "show_native_balance",
        ),
        (
            r#"{
        "ownerAddress": "BzbdvwEkQKeghTY53aZxTYjUienhdbkNVkgrLV6cErke",
        "options": {
            "showNativeBalance": false
        }
    }"#,
            "show_native_balance_false",
        ),
        (
            r#"{
        "ownerAddress": "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWN",
        "options": {
            "showNativeBalance": true
        }
    }"#,
            "show_native_balance_true_invalid_owner",
        ),
    ] {
        let request_serde: api::SearchAssets = serde_json::from_str(request).unwrap();
        let mut response = setup.das_api.search_assets(request_serde).await.unwrap();

        response.nativeBalance = response.nativeBalance.map(smooth_native_balance);

        insta::assert_json_snapshot!(format!("{}-{}", name, setup.name.clone()), response);

        let request_serde: api::GetAssetsByOwner = serde_json::from_str(request).unwrap();
        let mut response = setup
            .das_api
            .get_assets_by_owner(request_serde)
            .await
            .unwrap();
        response.nativeBalance = response.nativeBalance.map(smooth_native_balance);
        insta::assert_json_snapshot!(format!("{}-{}", name, setup.name.clone()), response);
    }

    let mut das_api = setup.das_api;
    das_api.rpc_client = Arc::new(RpcClient::new("https://api.invalid.solana.com".to_string()));
    let request = api::SearchAssets {
        owner_address: Some("BzbdvwEkQKeghTY53aZxTYjUienhdbkNVkgrLV6cErke".to_string()),
        options: Some(SearchAssetsOptions {
            show_native_balance: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = das_api.search_assets(request).await.unwrap();
    insta::assert_json_snapshot!(
        format!("{}-{}", "invalid_rpc", setup.name.clone()),
        response
    );
    let request = api::GetAssetsByOwner {
        owner_address: "BzbdvwEkQKeghTY53aZxTYjUienhdbkNVkgrLV6cErke".to_string(),
        options: Some(Options {
            show_native_balance: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = das_api.get_assets_by_owner(request).await.unwrap();
    insta::assert_json_snapshot!(
        format!("{}-{}", "invalid_rpc", setup.name.clone()),
        response
    );
}
