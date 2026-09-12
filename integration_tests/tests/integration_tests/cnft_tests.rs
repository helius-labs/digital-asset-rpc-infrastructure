use digital_asset_types::dao::{asset, cl_audits_v2, cl_items};
use function_name::named;
use std::str::FromStr;

use das_api::api::{self, ApiContract};

use digital_asset_types::rpc::options::{GetAssetOptions, Options};

use itertools::Itertools;

use sea_orm::{EntityTrait, Set, Update};
use serial_test::serial;

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

use super::common::*;

// TODO: Adjust this so that it can be run from anywhere.
// Do not move this test name or tests will break because the snapshot name and location will change.
pub async fn run_get_asset_scenario_test(
    setup: &TestSetup,
    asset_id: &str,
    seeds: Vec<SeedEvent>,
    order: Order,
) {
    let seed_permutations: Vec<Vec<&SeedEvent>> = match order {
        Order::AllPermutations => seeds.iter().permutations(seeds.len()).collect::<Vec<_>>(),
        Order::Forward => vec![seeds.iter().collect_vec()],
    };

    for events in seed_permutations {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        index_seed_events(setup, events).await;
        let request = api::GetAsset {
            id: asset_id.to_string(),
            ..api::GetAsset::default()
        };
        let response = setup.das_api.get_asset(request).await.unwrap();
        insta::assert_json_snapshot!(setup.name.clone(), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_name_trimming() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let asset_id = "6cbucQmGXAkfF2wu6P4KgHKUqDfp8e97wkm13NTFAx4p";

    let seeds: Vec<SeedEvent> = vec![seed_txn(
        "3kG9rUmLGPwVRnVLoUgzB1XJ39pKNBCWgWr8251vw3V63oJ1nEyMke9PVLTvsNXHkaKRbMSUVHrvX26JgcxCfqgo",
    )];

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::Forward).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_asset_decompress() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let asset_id = "Az9QTysJj1LW1F7zkYF21HgBj3FRpq3zpxTFdPnAJYm8";

    // Mint a compressed NFT and then decompress it. In production, we would receive account updates for the newly minted NFT.
    // This test guarentees consistent results if we index the events in different orders.
    let seeds: Vec<SeedEvent> = vec![
        // mint cNFT
        seed_txn("55tQCoLUtHyu4i6Dny6SMdq4dVD61nuuLxXvRLeeQqE6xdm66Ajm4so39MXcJ2VaTmCNDEFBpitzLkiFaF7rNtHi"),
        // redeem
        seed_txn("4FQRV38NSP6gDo8qDbTBfy8UDHUd6Lzu4GXbHtfvWbtCArkVcbGQwinZ7M61eCmPEF5L8xu4tLAXL7ozbh5scfRi"),
        // decompress
        seed_txn("3Ct9n9hv5PWEYbsrgDdUDqegzsnX2n5jYRxkq5YafFAueup8mTYmN4nHhNCaEwVyVAVqNssr4fizdg9wRavT7ydE"),
        // regular nft mint
        seed_nft("Az9QTysJj1LW1F7zkYF21HgBj3FRpq3zpxTFdPnAJYm8"),
    ];

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_scenario_mint_update_metadata() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    // Mint a compressed NFT and then update its metadata. Verify correct state regardless of order.
    let asset_id = "FLFoCw2RBbxiw9rbEeqPWJ5rasArD9kTCKWEJirTexsU";
    let seeds: Vec<SeedEvent> = vec![
        // mint cNFT
        seed_txn("2DP84v6Pi3e4v5i7KSvzmK4Ufbzof3TAiEqDbm9gg8jZpBRF9f1Cy6x54kvZoHPX9k1XfqbsG1FTv2KVP9fvNrN6"),
        // update metadata
        seed_txn("3bsL5zmLKvhN9Je4snTKxjFSpmXEEg2cvMHm2rCNgaEYkNXBqJTA4N7QmvBSWPiNUQPtzJSYzpQYX92NowV3L7vN"),
    ];

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_scenario_mint_update_metadata_remove_creators() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    // Mint a compressed NFT and then update its metadata to remove creators.
    // Creator removal inserts a placeholder creator to handle out-of-order updates.
    // This test explicitly verifies this behaviour.
    let asset_id = "Gi4fAXJdnWYrEPjQm3wnW9ctgG7zJjB67zHDQtRGRWyZ";
    let seeds: Vec<SeedEvent> = vec![
        // mint cNFT
        seed_txn("2qMQrXfRE7pdnjwobWeqDkEhsv6MYmv3JdgvNxTVaL1VrMCZ4JYkUnu7jiJb2etX3W9WyQgSxktUgn9skxCeqTo5"),
        // update metadata (no creators)
        seed_txn("41YW187sn6Z2dXfqz6zSbnPtQoE826cCSgTLnMLKa9rH1xrCqAXBQNwKnzjGc9wjU5RtMCqKhy2eMN2TjuYC8veB"),
    ];

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_scenario_update_metadata_bugfix() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Mainnet),
        },
    )
    .await;

    // Reproduces a bug we saw where an update metadata failed due to empty json URI.
    let asset_id = "5xg1NhPgYajc3LYzv5T5RT9vfxVP3s2uyT9zTb9azu1f";
    let seeds: Vec<SeedEvent> = vec![
        // mint cNFT
        seed_txn("34mJGDZXLDv2w9uudkoWHPqPFUKxikE2MRzBe2RLGuEZBTs7bgUmBQtq5bKtjmkN3tQcKvLS7XKXwXHAf5Xy4a3"),
        seed_txn("zVLGB5UX8fm2xF5XM57bMSYxd29HaJhRZtDmTCxVm1ZhQyauGVBAZgo4TdqkSPoN4oFXgVmm3TEXJHSLmjrQTE8"),
        seed_txn("5aCknZAHzGUnBAkDSqpsdMGGQDggporsL2kRZ9wdPQsZWbNSunk4ubZcqFuAn2P74C5FC8iqZQ2sJyMURHo3vBsd"),
        seed_txn("2pdMPBc2fPcL2xHGvo7uhRu9Js7RsYKtkr1iJ1rEJ4qbZdLZFZhx4r4JpC6fPVuEEZ8euR1gJzz4W7f4sPNsNPbt"),
        // this is a broken cNFT update metadata
        seed_txn("4zq2YNM5gc4ZQNqwNS1TU6gvaruhthzNeV7e4XvKDPL32E118TUAQe9ThLdFozTTnnpnhegeZxUUKKvnibxAj8GX"),
    ];

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::Forward).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_get_owners_by_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_transaction(
        &setup,
        Signature::from_str("33b4JpfJDf94jEph2gmYA6t6kgYGEPXbEyjRpiqhKeKPn6Aa4hEARYWH7qCotnNnZFtDfYyU9W9DfSiHrEDtNaxu").unwrap()
    )
    .await;

    let token_account: Pubkey =
        Pubkey::try_from("GTBvbRXjYcs3qUn2o7fn1hgkF6pwLpdSb1WfWwnH4vpb").unwrap();
    index_account(&setup, token_account).await;

    let request = api::SearchOwners {
        asset: "BGM1BzsPrqukDFUt5qC1EdqdZWRn3NLWj2JhwtnJSNqt".to_string(),
        options: Some(Options {
            show_zero_balance: false,
            ..Options::default()
        }),
        ..api::SearchOwners::default()
    };
    let response = setup.das_api.search_owners(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_owners_table() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let transactions = vec![
        "25djDqCTka7wEnNMRVwsqSsVHqQMknPReTUCmvF4XGD9bUD494jZ1FsPaPjbAK45TxpdVuF2RwVCK9Jq7oxZAtMB",
        "3UrxyfoJKH2jvVkzZZuCMXxtyaFUtgjhoQmirwhyiFjZXA8oM3QCBixCSBj9b53t5scvsm3qpuq5Qm4cGbNuwQP7",
        "4fzBjTaXmrrJReLLSYPzn1fhPfuiU2EU1hGUddtHV1B49pvRewGyyzvMMpssi7K4Y5ZYj5xS9DrJuxqJDZRMZqY1",
    ];
    for txn in transactions {
        index_transaction(&setup, Signature::from_str(txn).unwrap()).await;
    }
    for (request, individual_test_name) in [
        (
            api::SearchAssets {
                owner_address: Some("F3MdnVQkRSy56FSKroYawfMk1RJFo42Quzz8VTmFzPVz".to_string()),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "base",
        ),
        (
            api::SearchAssets {
                owner_address: Some("F3MdnVQkRSy56FSKroYawfMk1RJFo42Quzz8VTmFzPVz".to_string()),
                page: Some(1),
                limit: Some(5),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),
                ..api::SearchAssets::default()
            },
            "with_token_type_all",
        ),
        (
            api::SearchAssets {
                owner_address: Some("3jnP4utL1VvjNhkxstYJ5MNayZfK4qHjFBDHNKEBpXCH".to_string()),
                page: Some(1),
                limit: Some(5),
                ..api::SearchAssets::default()
            },
            "with_different_owner",
        ),
        (
            api::SearchAssets {
                owner_address: Some("3jnP4utL1VvjNhkxstYJ5MNayZfK4qHjFBDHNKEBpXCH".to_string()),
                page: Some(1),
                limit: Some(5),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),

                ..api::SearchAssets::default()
            },
            "with_different_owner_and_token_type_all",
        ),
    ] {
        let response = setup.das_api.search_assets(request.clone()).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_no_json_uri() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let seeds = vec![seed_txn(
        "4ASu45ELoTmvwhNqokGQrh2VH8p5zeUepYLbkcULMeXSCZJGrJa7ojgdVh5JUxBjAMF9Lrp55EgUUFPaPeWKejNQ",
    )];
    run_get_asset_scenario_test(
        &setup,
        "DFRJ4PwAze1mMQccRmdyc46yQpEVd4FPiwtAVgzGCs7g",
        seeds,
        Order::Forward,
    )
    .await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_delegate_transfer() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "77wWrvhgEkkQZQVA2hoka1JTsjG3w7BVzvcmqxDrVPWE";

    let seeds: Vec<SeedEvent> = seed_txns([
        "KNWsAYPo3mm1HuFxRyEwBBMUZ2hqTnFXjoPVFo7WxGTfmfRwz6K8eERc4dnJpHyuoDkAZu1czK55iB1SbtCsdW2",
        "3B1sASkuToCWuGFRG47axQDm1SpgLi8qDDGnRFeR7LB6oa5C3ZmkEuX98373gdMTBXED44FkwT227kBBAGSw7e8M",
        "5Q8TAMMkMTHEM2BHyD2fp2sVdYKByFeATzM2mHF6Xbbar33WaeuygPKGYCWiDEt3MZU1mUrq1ePnT9o4Pa318p8w",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_redeem_cancel_redeem() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "5WaPA7HLZKGg56bcKiroMXAzHmB1mdxK3QTeCDepLkiK";

    let seeds: Vec<SeedEvent> = seed_txns([
        "3uzWoVgLGVd9cGXaF3JW7znpWgKse3obCa2Vvdoe59kaziX84mEXTwecUoZ49PkJDjReRMSXksKzyfj7pf3ekAGR",
        "49bJ8U3cK9htmLvA1mhXXcjKdpV2YN5JQBrb3Quh7wxENz1BP9F8fE9CKsje41aMbZwzgomnkXirKx2Xpdvprtak",
        "32FpSe6r9jnFNjjvbx2PPQdZqs5KpMoF6yawiRW1F6ctu1kmx2B4sLDBGjsthVQtmnhaJVrqdtmUP893FwXCbqY5",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_redeem() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let asset_id = "Az9QTysJj1LW1F7zkYF21HgBj3FRpq3zpxTFdPnAJYm8";

    let seeds: Vec<SeedEvent> = seed_txns([
        "55tQCoLUtHyu4i6Dny6SMdq4dVD61nuuLxXvRLeeQqE6xdm66Ajm4so39MXcJ2VaTmCNDEFBpitzLkiFaF7rNtHi",
        "4FQRV38NSP6gDo8qDbTBfy8UDHUd6Lzu4GXbHtfvWbtCArkVcbGQwinZ7M61eCmPEF5L8xu4tLAXL7ozbh5scfRi",
        "3Ct9n9hv5PWEYbsrgDdUDqegzsnX2n5jYRxkq5YafFAueup8mTYmN4nHhNCaEwVyVAVqNssr4fizdg9wRavT7ydE",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_transfer_burn() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "8vw7tdLGE3FBjaetsJrZAarwsbc8UESsegiLyvWXxs5A";

    let seeds: Vec<SeedEvent> = seed_txns([
        "5coWPFty37s7haT3SVyMf6PkTaABEnhCRhfDjXeMNS58czHB5dCFPY6VrsZNwxBnqypmNic1LbLp1j5qjbdnZAc8",
        "k6jmJcurgBQ6F2bVa86Z1vGb7ievzxwRZ8GAqzFEG8HicDizxceYPUm1KTzWZ3QKtGgy1EuFWUGCRqBeKU9SAoJ",
        "KHNhLijkAMeKeKm6kpbk3go6q9uMF3zmfCoYSBgERe8qJDW8q5ANpnkyBuyVkychXCeWzRY8i5EtKfeGaDDU23w",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_transfer_noop() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let asset_id = "7myVr8fEG52mZ3jAwgz88iQRWsuzuVR2nfH8n2AXnBxE";

    let seeds: Vec<SeedEvent> = seed_txns([
        "4nKDSvw2kGpccZWLEPnfdP7J1SEexQFRP3xWc9NBtQ1qQeGu3bu5WnAdpcLbjQ4iyX6BQ5QGF69wevE8ZeeY5poA",
        "4URwUGBjbsF7UBUYdSC546tnBy7nD67txsso8D9CR9kGLtbbYh9NkGw15tEp16LLasmJX5VQR4Seh8gDjTrtdpoC",
        "5bNyZfmxLVP9cKc6GjvozExrSt4F1QFt4PP992pQwT8FFHdWsX3ZFNvwurfU2xpDYtQ7qAUxVahGCraXMevRH8p1",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_transfer_transfer() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "EcLv3bbLYr2iH5PVEuf9pJMRdDCvCqwSx3Srz6AeKjAe";

    let seeds: Vec<SeedEvent> = seed_txns([
        "5bq936UgGs4RnxM78iXp1PwVhr8sTYoEsHCWpr8QBFtc2YtS3ieYHcsPG46G2ikwrS3tXYnUK93PzseT52AR81RR",
        "5VC3Jqr5X1N8NB8zuSahHpayekLVozYkDiPjJLqU6H5M6fq9ExVLGYYKKCPbeksMPXTjy65sdEQGPzDWAYPs8QjP",
        "34xjcNf3rZFKz381hKpFLqxpojaDgXEpCqH5qcpTXLaJnDbtqRz35wiuMF1cAgvJGLzYYrwaMvCK1D7LxYsdpMU1",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_verify_collection_unverify_collection() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "BiHHJ1gKV4exTjPe7PE6aydgMVqRUzzz8aeWYCGhZJ4s";

    let seeds: Vec<SeedEvent> = seed_txns([
        "5uWXt8JAhuP2XQ2nYJTq8Ndp34fdG3vmJ7DJnb3bE6iyrZZ6jeuN9w5jZvKrduMDu4zKyQU7A3JtswhKxE3hjKBk",
        "4hQQsDKgDx5PpZR7nGvxKsLSvX4J7voaiJC3ag7dPuu4HY5kbvaqD2gyeHbdja1f22ypmzouRNpuo6sbyGDSSgya",
        "5k71fZRpRagY45ZYu13Q8C3Bmw6KFPBkRmbBx2NuYk7roVtvM8P16WouCZtnkhRCyKyQHSgHKyTY92t9aq2tyLdd",
    ]);

    let seed_permutations: Vec<Vec<&SeedEvent>> =
        seeds.iter().permutations(seeds.len()).collect::<Vec<_>>();

    for events in seed_permutations {
        apply_migrations_and_delete_data(setup.db.clone()).await;
        index_seed_events(&setup, events).await;
        let request = api::GetAsset {
            id: asset_id.to_string(),
            options: Some(GetAssetOptions {
                show_unverified_collections: true,
                ..GetAssetOptions::default()
            }),
            ..api::GetAsset::default()
        };

        let response = setup.das_api.get_asset(request).await.unwrap();
        insta::assert_json_snapshot!(setup.name.clone(), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_verify_creator() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "5rmTyghEuZhRTB77L3KqGMy6h5RpSNWNLj14avbxGNKB";

    let seeds: Vec<SeedEvent> = seed_txns([
        "37ts5SqpNazPTp26VfC4oeuXpXezKYkD9oarczPNaE8TUGG8msifnTYTBJiBZNBeAUGrNw85EEfwnR1t9SieKTdq",
        "4xrw5UwQSxxPzVxge6fbtmgLNsT2amaGrwpZFE95peRbnHGpxWtS2fF7whXW2xma4i2KDXdneztJZCAtgGZKTw11",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_verify_collection() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "2WjoMU1hBGXv8sKcxQDGnu1tgMduzdZEmEEGjh8MZYfC";

    let seeds: Vec<SeedEvent> = seed_txns([
        "63xhs5bXcuMR3uMACXWkkFMm7BJ9Thknh7WNMPzV8HJBNwpyxJTr98NrLFHnTZDHdSUFD42VFQx8rjSaGynWbaRs",
        "5ZKjPxm3WAZzuqqkCDjgKpm9b5XjB9cuvv68JvXxWThvJaJxcMJgpSbYs4gDA9dGJyeLzsgNtnS6oubANF1KbBmt",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}

#[tokio::test]
#[serial]
#[named]
async fn test_cnft_get_token_accounts() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let seed_events = [seed_txn(
        "55tQCoLUtHyu4i6Dny6SMdq4dVD61nuuLxXvRLeeQqE6xdm66Ajm4so39MXcJ2VaTmCNDEFBpitzLkiFaF7rNtHi",
    )];
    index_seed_events(&setup, seed_events.iter().collect_vec()).await;

    let request = api::GetTokenAccounts {
        page: Some(1),
        limit: Some(10),
        owner: Some("21sT5BUD82yVYs7fSnLZy1mWNDPEnsF1X3Brhqw93Uni".to_string()),
        ..api::GetTokenAccounts::default()
    };
    let response = setup
        .das_api
        .get_token_accounts(request.clone())
        .await
        .unwrap();

    insta::assert_json_snapshot!(name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_search_by_tree() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    // These transactions index two unrelated assets.
    let transactions = vec![
        "25djDqCTka7wEnNMRVwsqSsVHqQMknPReTUCmvF4XGD9bUD494jZ1FsPaPjbAK45TxpdVuF2RwVCK9Jq7oxZAtMB",
        "3UrxyfoJKH2jvVkzZZuCMXxtyaFUtgjhoQmirwhyiFjZXA8oM3QCBixCSBj9b53t5scvsm3qpuq5Qm4cGbNuwQP7",
        "4fzBjTaXmrrJReLLSYPzn1fhPfuiU2EU1hGUddtHV1B49pvRewGyyzvMMpssi7K4Y5ZYj5xS9DrJuxqJDZRMZqY1",
        "4ASu45ELoTmvwhNqokGQrh2VH8p5zeUepYLbkcULMeXSCZJGrJa7ojgdVh5JUxBjAMF9Lrp55EgUUFPaPeWKejNQ",
    ];
    for txn in transactions {
        index_transaction(&setup, Signature::from_str(txn).unwrap()).await;
    }
    let tree_id = "67B3xvadpVeBJ6MT5tQqBqntrBeLNjrWbcF6uEAH1EEi";
    let owner_id = "347kSWBNPm6Z3VFZZXAcKnT4T7v4WVQYw5b2xPJxp6zA";
    let collection_id = "GG6KuiAjG4hhg2AYdtjSoCG1QesgRFjTaok5MLxmAF3k";

    let asset_2_id = Pubkey::try_from("4k41XV45LUyX2iRSdw8bvwPmKF9y7SNLi9imUGWc3ADS").unwrap();

    // HACK: Updating the owner of the second asset to be the same as the first asset so that
    //       we can test the tree filter.
    Update::one(asset::ActiveModel {
        id: Set(asset_2_id.to_bytes().to_vec()),
        owner: Set(Some(
            Pubkey::try_from(owner_id).unwrap().to_bytes().to_vec(),
        )),
        ..Default::default()
    })
    .exec(setup.db.as_ref())
    .await
    .unwrap();

    for (request, individual_test_name) in [
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                ..api::SearchAssets::default()
            },
            "no_filters_other_than_tree",
        ),
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                owner_address: Some(owner_id.to_string()),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),
                ..api::SearchAssets::default()
            },
            "all_token_type",
        ),
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                owner_address: Some(owner_id.to_string()),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::Fungible),
                ..api::SearchAssets::default()
            },
            "fungible_token_type",
        ),
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                owner_address: Some(owner_id.to_string()),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::RegularNft),
                ..api::SearchAssets::default()
            },
            "regular_nft_token_type",
        ),
    ] {
        setup
            .das_api
            .search_assets(request.clone())
            .await
            .expect_err(&format!("{} should fail", individual_test_name));
    }

    for (request, individual_test_name) in [
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                owner_address: Some(owner_id.to_string()),
                ..api::SearchAssets::default()
            },
            "with_tree_id",
        ),
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                owner_address: Some(owner_id.to_string()),
                token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::CompressedNft),
                ..api::SearchAssets::default()
            },
            "with_tree_id_and_token_type_compressed_nft",
        ),
        (
            api::SearchAssets {
                tree: Some(tree_id.to_string()),
                grouping: Some(("collection".to_string(), collection_id.to_string())),
                ..api::SearchAssets::default()
            },
            "with_collection_grouping",
        ),
    ] {
        let response = setup.das_api.search_assets(request.clone()).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_do_not_index_scam_nft() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Mainnet),
        },
    )
    .await;

    let seeds: Vec<SeedEvent> = seed_txns([
        "317JHxreEWUsiSrKSwuRSMEgR79A1JutT8b4Fb8itiYJ9ZiHTHyFG33mFt9x536ap4HawYd4ZgWmBdCQCvfBdDaQ",
    ]);
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = api::GetAsset {
        id: "7YrKyJLzZrT5uukfT5BG95dmeG1DHqiaTv8mV19L9SNk".to_string(),
        ..api::GetAsset::default()
    };
    let response = setup.das_api.get_asset(request).await;
    // Assert that the response is an error
    assert!(response.is_err());

    // Query the cl items table to make sure that the proof is indexed
    let items = cl_items::Entity::find()
        .all(setup.db.as_ref())
        .await
        .unwrap();
    assert_ne!(items.len(), 0);
    let txn_history = cl_audits_v2::Entity::find()
        .all(setup.db.as_ref())
        .await
        .unwrap();
    assert_ne!(txn_history.len(), 0);
}

#[tokio::test]
#[serial]
#[named]
async fn test_mint_transfer_mpl_programs() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let asset_id = "ZzTjJVwo66cRyBB5zNWNhUWDdPB6TqzyXDcwjUnpSJC";

    let seeds: Vec<SeedEvent> = seed_txns([
        "3iJ6XzhUXxGQYEEUnfkbZGdrkgS2o9vXUpsXALet3Co6sFQ2h7J21J4dTgSka8qoKiUFUzrXZFHfkqss1VFivnAG",
        "4gV14HQBm8GCXjSTHEXjrhUNGmsBiyNdWY9hhCapH9cshmqbPKxn2kUU1XbajZ9j1Pxng95onzR6dx5bYqxQRh2a",
        "T571TWE76frw6mWxYoHDrTdxYq7hJSyCtVEG4qmemPPtsc1CCKdknn9rTMAVcdeukLfwB1G97LZLH8eHLvuByoA",
    ]);

    run_get_asset_scenario_test(&setup, asset_id, seeds, Order::AllPermutations).await;
}
