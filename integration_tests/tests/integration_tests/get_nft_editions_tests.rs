use function_name::named;

use das_api::api::{self, ApiContract};

use itertools::Itertools;

use serial_test::serial;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_get_nft_editions_for_mint() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let seeds = seed_edition_nfts([
        "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL",
        "GJvFDcBWf6aDncd1TBzx2ou1rgLFYaMBdbYLBa9oTAEw",
        "9yQecKKYSHxez7fFjJkUvkz42TLmkoXzhyZxEf2pw8pz",
        "7AeRUkukNCpWFtxK2QBZr1PymzPde6qtQYND6CajrE2B",
        "5bEXK3igzQHXchPz36bGGrvmVF39p8iB1NJVgA5hWH3h",
    ]);
    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
    {
        "mint": "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL"
    }
    "#,
            "get-master-edition-nfts",
        ),
        (
            r#"
    {
        "mint": "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL",
        "limit":2
    }
    "#,
            "get-edition-limit",
        ),
        (
            r#"
    {
        "mint": "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL",
        "page": 2,
        "limit":2
    }
    "#,
            "get-edition-with-page",
        ),
    ] {
        let request: api::GetNftEditions = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_nft_editions(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_nft_editions_for_null_max_supply() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    let seeds = seed_edition_nfts(["5dFNU4rmxQGowe4Qm7Bqf2od2kSTfKmqmH9VrF6TzfHR"]);
    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [(
        r#"
    {
        "id": "5dFNU4rmxQGowe4Qm7Bqf2od2kSTfKmqmH9VrF6TzfHR"
    }
    "#,
        "get-edition-nft-null",
    )] {
        let request: api::GetAsset = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_asset(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}
