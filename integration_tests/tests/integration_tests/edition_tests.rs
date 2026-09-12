use function_name::named;

use das_api::api::{self, ApiContract};

use itertools::Itertools;

use serial_test::serial;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_get_nft_editions() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let seeds = seed_edition_nfts([
        "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL",
        "5bEXK3igzQHXchPz36bGGrvmVF39p8iB1NJVgA5hWH3h",
    ]);
    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;
    let seeds: Vec<SeedEvent> = seed_txns([
        "fZZkevu826c6CfoikvSo5GRtyQMc5xR8pQEXq3uCwqre7gpfm4X93ujanPkE5Fck8LNnQ3bzrDFBt8Sr7DotgUJ",
        "5fPo2qSdzxWFpKXm7vkv9sP9ik9FZyF9QCvxhCEjKhCviHod4ytFnWVdnnyYwd4vdgQpEG85oEvi3RAyv4w8CNbz",
    ]);
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
    {
        "id": "Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL"
    }
    "#,
            "get-master-edition",
        ),
        (
            r#"
    {
        "id": "5bEXK3igzQHXchPz36bGGrvmVF39p8iB1NJVgA5hWH3h"
    }
    "#,
            "get-edition",
        ),
    ] {
        let request: api::GetAsset = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_asset(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v"
        }
        "#,
            "get-master-edition-for-owner",
        ),
        (
            r#"
        {
            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD"
        }
        "#,
            "show-edition-for-owner",
        ),
    ] {
        let request: api::GetAssetsByOwner = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_assets_by_owner(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }

    for (request, individual_test_name) in [
        (
            r#"
        {

            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v",
            "tokenType": "all"
        }
        "#,
            "search-assets-all-for-master-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v",
            "tokenType": "regularNft"
        }
        "#,
            "search-assets-regular-for-master-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v",
            "tokenType": "nonFungible"
        }
        "#,
            "search-assets-non-fungible-for-master-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v",
            "tokenType": "fungible"
        }
        "#,
            "search-assets-fungible-for-master-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "DN5aWeXu4Tx4uBUG3zvqpmcRG7jaYgeH87SnViLiRd7v",
            "tokenType": "compressedNft"
        }
        "#,
            "search-assets-compressed-for-master-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD",
            "tokenType": "all"
        }
        "#,
            "search-assets-all-for-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD",
            "tokenType": "regularNft"
        }
        "#,
            "search-assets-regular-for-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD",
            "tokenType": "nonFungible"
        }
        "#,
            "search-assets-non-fungible-for-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD",
            "tokenType": "fungible"
        }
        "#,
            "search-assets-fungible-for-edition",
        ),
        (
            r#"
        {

            "ownerAddress": "3HxqsUguP6E7CNqjvpEAnJ8v86qbyJgWvN2idAKygLdD",
            "tokenType": "compressedNft"
        }
        "#,
            "search-assets-compressed-for-edition",
        ),
    ] {
        let request: api::SearchAssets = serde_json::from_str(request).unwrap();
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
    for (request, individual_test_name) in [(
        r#"
    {
        "creatorAddress": "232PpcrPc6Kz7geafvbRzt5HnHP4kX88yvzUCN69WXQC"
    }
    "#,
        "get-edition-by-creator",
    )] {
        let request: api::GetAssetsByCreator = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_assets_by_creator(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
    for (request, individual_test_name) in [(
        r#"
    {
        "ids": ["Ey2Qb8kLctbchQsMnhZs5DjY32To2QtPuXNwWvk4NosL", "5bEXK3igzQHXchPz36bGGrvmVF39p8iB1NJVgA5hWH3h"]
    }
    "#,
        "get-edition-by-batch",
    )] {
        let request: api::GetAssets = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}
