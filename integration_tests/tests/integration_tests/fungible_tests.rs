use digital_asset_types::rpc::options::Options;
use function_name::named;

use das_api::api::{self, ApiContract};

use itertools::Itertools;

use serial_test::serial;

use solana_sdk::pubkey::Pubkey;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_get_metadata() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = vec![
        seed_account("E851Na3ojPNUuD1MCuA6ARYQ4ez2oXdE4wDp4UsRatcT"),
        seed_account("3zMRuGDYbDYFqbPcyuBBFX9m5vi2GyCBXgqCRjm7o3UG"),
    ];
    index_seed_events(&setup, seeds.iter().collect_vec()).await;
    let request: api::GetAsset = serde_json::from_str(
        r#"{
        "id": "E851Na3ojPNUuD1MCuA6ARYQ4ez2oXdE4wDp4UsRatcT",
        "displayOptions": {
            "showFungible": true
        }
    }"#,
    )
    .unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(setup.name.clone(), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_get_owner() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;
    let seeds = seed_token_mints([
        "EbxeiHpnTkoBznU3F8CLHmMy8gLwrhun7EL3mP9fEPHi",
        "HwVsAvTjGNgynbGmDyAESS7HkLmJPuaHgHbDQ1MRoPf5",
        "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "displayOptions": {
                "showFungible": true
            }
        }
        "#,
            "show-fungible-true",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "displayOptions": {
                "showFungible": false
            }
        }
        "#,
            "show-fungible-false",
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

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "all"
        }
        "#,
            "search-assets-all",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "supplyMint": "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5",
            "tokenType": "all"
        }
        "#,
            "search-assets-all-with-supply-mint",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "fungible"
        }
        "#,
            "search-assets-fungibles",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "supplyMint": "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5",
            "tokenType": "fungible"
        }
        "#,
            "search-assets-fungible-with-supply-mint",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "regularNft"
        }
        "#,
            "search-assets-regular-nft",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "regularNft",
            "supplyMint": "HwVsAvTjGNgynbGmDyAESS7HkLmJPuaHgHbDQ1MRoPf5"
        }
        "#,
            "search-assets-regular-nft-with-supply-mint",
        ),
        (
            r#"
        {
            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "nonFungible"
        }
        "#,
            "search-assets-non-fungible",
        ),
        (
            r#"
        {

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "all",
            "limit": 2
        }
        "#,
            "search-assets-all-with-limit",
        ),
        (
            r#"
        {

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "all",
            "cursor": "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5"
        }
        "#,
            "search-assets-all-with-cursor-1",
        ),
        (
            r#"
        {

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "all",
            "cursor": "EbxeiHpnTkoBznU3F8CLHmMy8gLwrhun7EL3mP9fEPHi"
        }
        "#,
            "search-assets-all-with-cursor-2",
        ),
        (
            r#"
        {

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "fungible",
            "cursor": "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5"
        }
        "#,
            "search-assets-fungible-with-cursor-1",
        ),
        (
            r#"
        {

            "ownerAddress": "B9fcrRtNZK1j29sfPhrdrcbNwkrNcEGWNu7XFApt56ww",
            "tokenType": "fungible",
            "cursor": "EbxeiHpnTkoBznU3F8CLHmMy8gLwrhun7EL3mP9fEPHi"
        }
        "#,
            "search-assets-fungible-with-cursor-2",
        ),
    ] {
        let request: api::SearchAssets = serde_json::from_str(request).unwrap();
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
    let request: api::GetAsset = serde_json::from_str(
        r#"
    {
        "id": "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5",
        "displayOptions": {
            "showFungible": true
        }
    }"#,
    )
    .unwrap();
    let response = setup.das_api.get_asset(request).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "get-asset"), response);

    let request: api::GetAssets = serde_json::from_str(
        r#"
    {
        "ids": [
            "HwVsAvTjGNgynbGmDyAESS7HkLmJPuaHgHbDQ1MRoPf5",
            "Hp9aaQWpBmr4fc257KG19pSfitPGahTHpH9fEXMFwDN5",
            "EbxeiHpnTkoBznU3F8CLHmMy8gLwrhun7EL3mP9fEPHi",
            "8ojQuBwfC7xwfL6kvcdSJzMr71NZ8eE349LnWj657jQL"
        ],
        "displayOptions": {
            "showFungible": true
        }
    }"#,
    )
    .unwrap();
    let response = setup.das_api.get_assets(request).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-{}", name, "get-asset-batch"), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_get_owners_by_asset() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = seed_accounts([
        "HyZvk5wjmnSezhgfRcGRfuHeNtKTUB7XJg1h3YYbq2op",
        "Fz9dRWdCRZx1eZot2NhNFwSiiPQWSpZ2Kh3Ttskr5cju",
        "2GrsiVviNvpZ5mGm1fdTtx5FiewCWZ61RMnpH95Nzqkg",
        "6DkecwKUKBvCc11sF1eF5vqQ4DkRTwHbWtRYG5jHWgRK",
        "6kBS8ANHZExmW7zuBvAG1ofddK8h291orBUMycwo1J7N",
        "BKBNadcwHGN125VxdjAn2WqYDkzm6BbWKhrsAAGKmW9v",
        "RmCcDa1gaBjbfRJsi4mYnpoMg9yXp3PKMHCqSUUFFEJ",
        "6oM2Pst62qPdUtpv5bNPKDSLHdS68P79ujwJujUeCnU7",
        "J8B9nMeMYeuzntAeK5fLztRL8UBn1GzbSKs6bWi1E3JF",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "asset": "xxxxa1sKNGwFtw2kFn8XauW9xq8hBZ5kVtcSesTT9fW"
        }
        "#,
            "base",
        ),
        (
            r#"
        {
            "asset": "xxxxa1sKNGwFtw2kFn8XauW9xq8hBZ5kVtcSesTT9fW",
            "displayOptions": {
                "showZeroBalance": false
            }
        }
        "#,
            "show-zero-balance-false",
        ),
        (
            r#"
            {
                "asset": "xxxxa1sKNGwFtw2kFn8XauW9xq8hBZ5kVtcSesTT9fW",
                "displayOptions": {
                    "showZeroBalance": true
                }
            }
            "#,
            "show-zero-balance-true",
        ),
        (
            r#"
            {
                "page": 2,
                "limit": 2,
                "asset": "xxxxa1sKNGwFtw2kFn8XauW9xq8hBZ5kVtcSesTT9fW",
                "displayOptions": {
                    "showZeroBalance": true
                }
            }
            "#,
            "show-zero-balance-true-and-paging",
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
async fn test_fungible_get_token_accounts_by_mint() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = seed_accounts([
        "A35gLorchY5NLkceVC2Vhu881h6hYbpyjMTRQFoJAmKu",
        "A7hHRV3gksmQUvvbPx1phXvapVBGgGeAnfQ1zASX3zup",
        "GWdWkH9dvpRTrgEibJsPX2gkRFFHyJJDN9dQXTur3qq1",
        "6TXyfVhR91BSSgQqtYSt2TcBDi7knPdCbjebwn3KuTvX",
        "93Jv6HSpwHMwiwSQHG24SJ3c3PcMjgvJpK27dR62GLmK",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"
        }
        "#,
            "only-owner",
        ),
        (
            r#"
        {
            "owner": "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1",
            "mint": "So11111111111111111111111111111111111111112"
        }
        "#,
            "owner-and-mint",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "limit": 1
        }
        "#,
            "cursor-mint-1",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "cursor": "A35gLorchY5NLkceVC2Vhu881h6hYbpyjMTRQFoJAmKu",
            "limit": 2
        }
        "#,
            "cursor-mint-2",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "cursor": "A7hHRV3gksmQUvvbPx1phXvapVBGgGeAnfQ1zASX3zup",
            "limit": 1
        }
        "#,
            "cursor-mint-3",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "after": "A35gLorchY5NLkceVC2Vhu881h6hYbpyjMTRQFoJAmKu"
        }
        "#,
            "after-token",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "before": "GWdWkH9dvpRTrgEibJsPX2gkRFFHyJJDN9dQXTur3qq1"
        }
        "#,
            "before-token",
        ),
        (
            r#"
        {
            "mint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
            "after": "A35gLorchY5NLkceVC2Vhu881h6hYbpyjMTRQFoJAmKu",
            "before": "GWdWkH9dvpRTrgEibJsPX2gkRFFHyJJDN9dQXTur3qq1"
        }
        "#,
            "before-after-token",
        ),
    ] {
        let request: api::GetTokenAccounts = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_token_accounts(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_get_token_accounts_having_extensions() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new_with_options(
        name.clone(),
        TestSetupOptions {
            network: Some(Network::Devnet),
        },
    )
    .await;

    let seeds = seed_accounts([
        "J8QBq4Er3wYdmM4Eeg4b5Z6TioygY8b5fHVkDNtdLve4",
        "4Ueynth2yBaDCEnn2oqh2xpwYYjxzngQ5arayotGDdC6",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [(
        r#"
        {
            "mint": "J8QBq4Er3wYdmM4Eeg4b5Z6TioygY8b5fHVkDNtdLve4",
            "displayOptions": {
                "showZeroBalance": true
            }
        }
        "#,
        "mint-with-token-extension",
    )] {
        let request: api::GetTokenAccounts = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_token_accounts(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_get_token_accounts_by_owner() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = seed_accounts([
        "BBNurWySByzp8qEjFRoZExp2zh5hkrdLuU8t2qUAMHw1",
        "33JLPkn3u172De4xHV7WKzpeazzsFq514oDkxu9g6D4y",
        "2RvgU2YHSTW8XtjgYPKxbEqsgzBKY8iQecRk31kr35V2",
        "6A8TLiYURA18x7z3tJnwJoRVxhwBDe3ReZppr79KMPfz",
        "2ddkd1AbXXB2DQxbXnLmdMiRWKAaevYLJrhQ8NZrEWB5",
        "5Rb6miYHtCPovVTyiup3fH3ZymuVFTPfLXx9hxpNKwrt",
    ]);

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "owner": "A8UHfv6aHTVdQsYybK6NNbWsUwc3SXvNWKiWHFa3jZ8u"
        }
        "#,
            "owner-1",
        ),
        (
            r#"
        {
            "owner": "21sT5BUD82yVYs7fSnLZy1mWNDPEnsF1X3Brhqw93Uni"
        }
        "#,
            "owner-2",
        ),
        (
            r#"
        {
            "owner": "7LSb8VjDTVPTpcfBkpzzhjUvJ77mrShD1ZtuEmoxo48g"
        }
        "#,
            "owner-3",
        ),
    ] {
        let request: api::GetTokenAccounts = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_token_accounts(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_nft_mix() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = [
        seed_account("AjMN1WMFusdZnJmHnY61mY6tzg3Eow4miVhJ1pgjcftj"),
        seed_token_mint("DSejbjAmQFQtNk9vFqn6abTnPbSoENfkHCs6KU7j6swu"),
        seed_token_mint("ByJk5D4y7urDRx4Qw95Sv9bLLdgoVcBsdyTFsgSdK29V"),
    ];

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "4PjhNVPBizDhsfAZnkyVZCWkrNhwM1q4GdcbJHtKWKpB",
            "tokenType": "nonFungible"
        }
        "#,
            "only-non-fungible",
        ),
        (
            r#"
        {
            "ownerAddress": "4PjhNVPBizDhsfAZnkyVZCWkrNhwM1q4GdcbJHtKWKpB",
            "tokenType": "all"        }
        "#,
            "all",
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
async fn test_fungible_search_by_collection() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = [seed_nft("JC2EeoeueTxKAovZGJoMJjgN7UX9MC6fsKGhMCDEtw2H")];

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "tokenType": "fungible",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "base",
        ),
        (
            r#"
        {
            "tokenType": "fungible",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "grouping": ["collection", "HBgj24hUAAQs5ghzrrrM7T4WgZ7CQm3JiSUm3sUhTwGb"],
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "with-collection",
        ),
        (
            r#"
        {
            "tokenType": "fungible",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "grouping": ["collection", "F85YsPgCGP4PpYuieJgLFsfuxckQ45AfvY7vhfRQtq8w"],
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "with-collection-2",
        ),
        (
            r#"
        {
            "tokenType": "all",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "with-token-type-all",
        ),
        (
            r#"
        {
            "tokenType": "all",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "grouping": ["collection", "HBgj24hUAAQs5ghzrrrM7T4WgZ7CQm3JiSUm3sUhTwGb"],
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "with-token-type-all-and-collection",
        ),
        (
            r#"
        {
            "tokenType": "all",
            "ownerAddress": "GtBcUSWUi9h6XD6z84p66hGDJfoc3BvdWgoTpdj1Y3gL",
            "grouping": ["collection", "F85YsPgCGP4PpYuieJgLFsfuxckQ45AfvY7vhfRQtq8w"],
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "with-token-type-all-and-collection-2",
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
async fn test_fungible_search_owner() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds = [
        seed_token_mint("n54ZwXEcLnc3o7zK48nhrLV4KTU5wWD4iq7Gvdt5tik"),
        seed_token_mint_with_options("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", false),
        seed_account("GTBvbRXjYcs3qUn2o7fn1hgkF6pwLpdSb1WfWwnH4vpb"),
        seed_account("BdnMPvk1PN1uCG413pkJBoKabFu9Ywmz7g4pQVcN1aQB"),
        seed_account("4tCxy6YtPMNhgFmdUs6weRxugx1EXvJSvTN3WMDA95gw"),
    ];

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "GgLAbLNTNHzjzF4WJmYDaXEXVEiYFoho6PWazisXbLVr",
            "tokenType": "all"
        }
        "#,
            "all",
        ),
        (
            r#"
        {
            "ownerAddress": "GgLAbLNTNHzjzF4WJmYDaXEXVEiYFoho6PWazisXbLVr",
            "tokenType": "fungible"
        }
        "#,
            "only-fungible",
        ),
        (
            r#"
        {

            "ownerAddress": "AYfy57orvDk2i3XoLzvHSFbnjxas7Zp6ae9rP4TnoA2i",
            "tokenType": "fungible"
        }
        "#,
            "multiple-token-accounts",
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
async fn test_fungible_show_zero_balance() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = vec![
        seed_token_mint("CKfatsPMUf8SkiURsDXs7eK6GWb4Jsd6UDbs7twMCWxo"),
        seed_token_mint("G7rwEgk8KgQ4RUTnMy2W2i7dRDq4hXHD4CSp9PSmSbRW"),
        seed_account("3VUYGjYktCzNhDVymNb3Z1iHewtfPFRvdA53qSWuxdXy"),
        seed_account("hXP4faomdmra3hP6b96tVKSeCvfCigE4QCkMXnCY7Wz"),
    ];

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze"
        }
        "#,
            "search-assets-base",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "tokenType": "all"
        }
        "#,
            "search-assets-all",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "tokenType": "fungible"
        }
        "#,
            "search-assets-fungible",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "search-assets-show-zero-balance",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "tokenType": "all",
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "search-assets-all-show-zero-balance",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "tokenType": "fungible",
            "options": {
                "showZeroBalance": true
            }
        }
        "#,
            "search-assets-fungible-show-zero-balance",
        ),
    ] {
        let request: api::SearchAssets = serde_json::from_str(request).unwrap();
        let response = setup.das_api.search_assets(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }

    for (request, individual_test_name) in [
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze"
        }
        "#,
            "get-assets-by-owner",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "options": {
                "showFungible": true
            }
        }
        "#,
            "get-assets-by-owner-show-fungible",
        ),
        (
            r#"
        {
            "ownerAddress": "5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze",
            "options": {
                "showFungible": true,
                "showZeroBalance": true
            }
        }
        "#,
            "get-assets-by-owner-show-fungible-show-zero-balance",
        ),
    ] {
        let request: api::GetAssetsByOwner = serde_json::from_str(request).unwrap();
        let response = setup.das_api.get_assets_by_owner(request).await.unwrap();
        insta::assert_json_snapshot!(format!("{}-{}", name, individual_test_name), response);
    }
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_wsol() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;

    let seeds: Vec<SeedEvent> = vec![seed_token_mint(
        "So11111111111111111111111111111111111111112",
    )];

    apply_migrations_and_delete_data(setup.db.clone()).await;
    index_seed_events(&setup, seeds.iter().collect_vec()).await;

    let request = r#"
    {
        "ownerAddress": "GugU1tP7doLeTw9hQP51xRJyS8Da1fWxuiy2rVrnMD2m",
        "tokenType": "fungible"
    }
    "#;

    let request: api::SearchAssets = serde_json::from_str(request).unwrap();
    let response = setup.das_api.search_assets(request).await.unwrap();
    insta::assert_json_snapshot!(name, response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;
    let token_account: Pubkey =
        Pubkey::try_from("GTBvbRXjYcs3qUn2o7fn1hgkF6pwLpdSb1WfWwnH4vpb").unwrap();
    index_account(&setup, token_account).await;

    let request = api::SearchAssets {
        owner_address: Some("GgLAbLNTNHzjzF4WJmYDaXEXVEiYFoho6PWazisXbLVr".to_string()),
        token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::All),
        ..api::SearchAssets::default()
    };
    let response = setup.das_api.search_assets(request.clone()).await.unwrap();
    insta::assert_json_snapshot!(name.clone(), response);

    index_account_burn(&setup, token_account, get_max_slot()).await;
    let response = setup.das_api.search_assets(request).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-burn", name), response);
}

#[tokio::test]
#[serial]
#[named]
async fn test_fungible_balance_overflow() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let mint: Pubkey = Pubkey::try_from("drakjG26NLi2QkHzsmok6ei79VDYWVGR3eG8USzxGHb").unwrap();
    let ta: Pubkey = Pubkey::try_from("7o2RFDQYTEHGn6c8Dd1oCmuHxej9Bib8Wbrp3UBoiR7e").unwrap();

    index_account(&setup, mint).await;
    index_account(&setup, ta).await;

    let req = api::SearchAssets {
        owner_address: Some("24Bx3re6nMwxyyD6DA6AWtxzxwNhEm5AhwcRBPJtHSrY".to_string()),
        token_type: Some(digital_asset_types::dao::scopes::asset::TokenType::Fungible),
        ..api::SearchAssets::default()
    };
    let response = setup.das_api.search_assets(req.clone()).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-search-assets", name).clone(), response);

    let req = api::GetAssetsByOwner {
        owner_address: "24Bx3re6nMwxyyD6DA6AWtxzxwNhEm5AhwcRBPJtHSrY".to_string(),
        options: Some(Options {
            show_fungible: true,
            ..Options::default()
        }),
        ..api::GetAssetsByOwner::default()
    };
    let response = setup
        .das_api
        .get_assets_by_owner(req.clone())
        .await
        .unwrap();
    insta::assert_json_snapshot!(format!("{}-get-assets-by-owner", name).clone(), response);

    let req = api::GetTokenAccounts {
        owner: Some("24Bx3re6nMwxyyD6DA6AWtxzxwNhEm5AhwcRBPJtHSrY".to_string()),
        ..api::GetTokenAccounts::default()
    };
    let response = setup.das_api.get_token_accounts(req.clone()).await.unwrap();
    insta::assert_json_snapshot!(format!("{}-get-token-accounts", name).clone(), response);
}
