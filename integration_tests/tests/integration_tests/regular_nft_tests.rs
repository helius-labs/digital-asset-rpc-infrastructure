use function_name::named;

use das_api::api::{self, ApiContract};

use itertools::Itertools;

use serial_test::serial;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts(["CMVuYDS9nTeujfTPJb8ik7CRhAqZv4DfjfdamFLkJgxE"]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "id": "CMVuYDS9nTeujfTPJb8ik7CRhAqZv4DfjfdamFLkJgxE"
    }
    "#;

    let request: api::GetAsset = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset_with_cloudflare_ipfs() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts(["CTg3ZgYx79zrE1MteDVkmkcGniiFrK1hJ6yiabropump"]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "id": "CTg3ZgYx79zrE1MteDVkmkcGniiFrK1hJ6yiabropump"
    }
    "#;

    let request: api::GetAsset = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response, {
        ".token_info.supply" => "[supply]",
    });
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset_missing_image() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts(["3rHYaCNoe3gxLAehDDfzpMGRdtLhBLLtRgvNYXYPtZUU"]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "id": "3rHYaCNoe3gxLAehDDfzpMGRdtLhBLLtRgvNYXYPtZUU"
    }
    "#;

    let request: api::GetAsset = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response, {
        ".token_info.supply" => "[supply]",
    });
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset_batch() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts([
        "HTKAVZZrDdyecCxzm3WEkCsG1GUmiqKm73PvngfuYRNK",
        "2NqdYX6kJmMUoChnDXU2UrP9BsoPZivRw3uJG8iDhRRd",
        "5rEeYv8R25b8j6YTHJvYuCKEzq44UCw1Wx1Wx2VPPLz1",
        "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let seeds: Vec<SeedEvent> = vec![seed_account("Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX")];
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ids": ["HTKAVZZrDdyecCxzm3WEkCsG1GUmiqKm73PvngfuYRNK", "2NqdYX6kJmMUoChnDXU2UrP9BsoPZivRw3uJG8iDhRRd"]
        }
        "#,
            "only-2",
        ),
        (
            r#"
        {
            "ids": ["2NqdYX6kJmMUoChnDXU2UrP9BsoPZivRw3uJG8iDhRRd", "5rEeYv8R25b8j6YTHJvYuCKEzq44UCw1Wx1Wx2VPPLz1"]
        }
        "#,
            "only-2-different-2",
        ),
        (
            r#"
        {
            "ids": [
                "2NqdYX6kJmMUoChnDXU2UrP9BsoPZivRw3uJG8iDhRRd",
                "JECLQnbo2CCL8Ygn6vTFn7yeKn8qc7i51bAa9BCAJnWG",
                "5rEeYv8R25b8j6YTHJvYuCKEzq44UCw1Wx1Wx2VPPLz1"
            ]
        }
        "#,
            "2-and-a-missing-1",
        ),
        (
            r#"
        {
            "ids": [
                "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
            ]
        }
        "#,
            "just-1",
        ),
        (
            r#"
        {
            "ids": [
                "Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX"
            ]
        }
        "#,
            "with-mint-extension",
        ),
    ] {
        let request: api::GetAssets = serde_json::from_str(request).unwrap();
        let v2_response = setup.das_api.get_assets_v2(request.clone()).await.unwrap();
        let response = setup.das_api.get_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
        insta::assert_json_snapshot!(format!("{}-{}-v2", name, individual_test_name), v2_response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset_by_creator() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts([
        "SMBtHCCC6RYRutFEPb4gZqeBLUZbMNhRKaMKZZLHi7W",
        "Gk9A2rWYkoUkQdSqC5JLirpDoW4DHTUFxe2jJNhVMzFR",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "creatorAddress": "mdaoxg4DVGptU4WSpzGyVpK3zqsgn7Qzx5XNgWTcEA2",
        "displayOptions": {
            "showGrandTotal": true
        },
        "sortBy": {
            "sortBy": "created",
            "sortDirection": "desc"
        },
        "page": 1,
        "limit": 1
    }
    "#;

    let request: api::GetAssetsByCreator = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_assets_by_creator(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_asset_by_group() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts([
        "7jFuJ73mBPDdLMvCYxzrpFTD9FeDudRxdXGDALP5Cp2W",
        "BioVudBTjJnuDW22q62XPhGP87sVwZKcQ46MPSNz4gqi",
        "Fm9S3FL23z3ii3EBBv8ozqLninLvhWDYmcHcHaZy6nie",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "groupKey": "collection",
        "groupValue": "8Rt3Ayqth4DAiPnW9MDFi63TiQJHmohfTWLMQFHi4KZH",
        "sortBy": {
            "sortBy": "updated",
            "sortDirection": "asc"
        },
        "displayOptions": {
            "showGrandTotal": true
        },
        "page": 1,
        "limit": 1
    }
    "#;

    let request: api::GetAssetsByGroup = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_assets_by_group(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_owners_by_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_accounts([
        "GmNFwMYbh7frEPA3TMdZxi2bEta4huD1UZyGMFokfLBw",
        "5Jg6Jearnb4Rc5tjztcBfWgox9dMy7gWxJ9rRrtBmW8V",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "asset": "FA9cPjTQGsNP8ULUfxUpngZEZivYwAocw4aBoND4NTw"
        }
        "#,
            "base",
        ),
        (
            r#"
        {
            "asset": "FA9cPjTQGsNP8ULUfxUpngZEZivYwAocw4aBoND4NTw",
            "displayOptions": {
                "showZeroBalance": true
            }
        }
        "#,
            "show-zero-balance",
        ),
    ] {
        let request: api::SearchOwners = serde_json::from_str(request).unwrap();
        let response = setup.das_api.search_owners(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_search_assets() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts([
        "2PfAwPb2hdgsf7xCKyU2kAWUGKnkxYZLfg5SMf4YP1h2",
        "Dt3XDSAdXAJbHqvuycgCTHykKCC7tntMFGMmSvfBbpTL",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "ownerAddress": "6Cr66AabRYymhZgYQSfTCo6FVpH18wXrMZswAbcErpyX",
        "displayOptions": {
            "showGrandTotal": true
        },
        "page": 1,
        "limit": 2
    }
    "#;

    let request: api::SearchAssets = serde_json::from_str(request).unwrap();
    let response = setup.das_api.search_assets(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_collection_nft() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts(["J1S9H3QjnRtBbbuD4HjPV6RpRhwuk4zKbxsnCHuTgh9w"]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "id": "J1S9H3QjnRtBbbuD4HjPV6RpRhwuk4zKbxsnCHuTgh9w",
        "displayOptions": {
            "showUnverifiedCollections": true
        }
    }
    "#;

    let request: api::GetAsset = serde_json::from_str(request).unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_search_collection_nft() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts([
        "J1S9H3QjnRtBbbuD4HjPV6RpRhwuk4zKbxsnCHuTgh9w",
        "DL4pWLrfh2wXiovZLtbjeXDYMoo6zoa7wFCVJX8qUpxw",
        "8jJUWQJmQeEhj1bEMPgdDPmzConnPMyEe6bmmkLFRMQj",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
    {
        "collectionNft": true,
        "ownerAddress": "2RtGg6fsFiiF1EQzHqbd66AhW7R5bWeQGpTbv2UMkCdW",
        "displayOptions": {
            "showUnverifiedCollections": true
        }
    }
    "#,
            "search-collection-nft",
        ),
        (
            r#"
    {
        "collectionNft": false,
        "grouping": ["collection", "J1S9H3QjnRtBbbuD4HjPV6RpRhwuk4zKbxsnCHuTgh9w"],
        "displayOptions": {
            "showUnverifiedCollections": true
        }
    }
    "#,
            "search-collection-nft-false",
        ),
    ] {
        let request: api::SearchAssets = serde_json::from_str(request).unwrap();
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_reg_get_assets_same_nft_twice() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = seed_nfts(["EgGBxYtK2Esnasp9tPzqDyvEW46ZJ9KqeGHB6mdPvtWc"]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let ids = vec![
        "EgGBxYtK2Esnasp9tPzqDyvEW46ZJ9KqeGHB6mdPvtWc".to_string(),
        "EgGBxYtK2Esnasp9tPzqDyvEW46ZJ9KqeGHB6mdPvtWc".to_string(),
    ];
    let request: api::GetAssets = api::GetAssets {
        ids,
        ..api::GetAssets::default()
    };
    let response = setup.das_api.get_assets(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}
