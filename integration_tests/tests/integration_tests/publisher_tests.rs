use das_publisher::fetchers::{
    grpc::get_grpc_block_stream,
    poller::{
        fetch_block_with_account_data, get_transaction_keys, is_bubblegum_transaction,
        BUBBLEGUM_PUBKEY, DAS_ACCOUNTS,
    },
};
use function_name::named;

use futures_util::pin_mut;
use serial_test::serial;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tokio_stream::StreamExt;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_get_transaction_keys() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let sig = Signature::from_str(
        "55tQCoLUtHyu4i6Dny6SMdq4dVD61nuuLxXvRLeeQqE6xdm66Ajm4so39MXcJ2VaTmCNDEFBpitzLkiFaF7rNtHi",
    )
    .unwrap();
    let txn = get_transaction(&setup.client, sig, 3).await.unwrap();
    let keys = get_transaction_keys(txn.transaction);
    assert!(keys.contains(&BUBBLEGUM_PUBKEY));
    let sig_2 = Signature::from_str(
        "53MLZzkgKkZz7kPBetuCbJ2SshxgDcf7qjGwjruVZVuVKZDRFqLKcZTvFFD8kRDoVihgx3iY59135AJrHB9oS6v6",
    )
    .unwrap();
    let txn_2 = get_transaction(&setup.client, sig_2, 3).await.unwrap();
    let keys_2 = get_transaction_keys(txn_2.transaction);
    assert!(!keys_2.contains(&BUBBLEGUM_PUBKEY));
}

#[tokio::test]
#[serial]
#[named]
#[ignore]
async fn test_expand_block() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let slot = 302174307;
    let client = Arc::new(setup.client);
    let das_block = fetch_block_with_account_data(client.clone(), slot)
        .await
        .unwrap();
    assert!(das_block.das_transactions.len() > 10);
    let mut i = 0;
    for account in das_block.das_accounts {
        let (pubkey, (account, _slot)) = account;
        match account {
            Some(_) => {
                client.get_account(&pubkey).await.unwrap();
            }
            None => {
                let res = client.get_account(&pubkey).await;
                if let Ok(_) = res {
                    panic!("Expected closed account: {:?}", pubkey);
                }
            }
        }
        i += 1;
        if i > 30 {
            break;
        }
    }
}

#[tokio::test]
#[serial]
#[named]
#[ignore]
async fn test_grpc_publisher() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    let client = Arc::new(setup.client);
    let grpc_url = std::env::var("GRPC_URL").unwrap();
    let auth_header = std::env::var("GRPC_AUTH_HEADER").unwrap();
    let stream = get_grpc_block_stream(grpc_url, auth_header, None);
    pin_mut!(stream);
    let block = stream.next().await.unwrap();
    assert!(block.das_accounts.len() > 0);
    // Print the number of accoutns matching each key
    let mut counts = HashMap::new();
    for account in block.das_accounts.clone() {
        let (pubkey, (account, _slot)) = account;
        if let Some(account) = account {
            let count = counts.entry(account.owner).or_insert(0);
            *count += 1;
            assert!(
                DAS_ACCOUNTS.contains(&account.owner)
                    || account.owner == solana_system_interface::program::id()
            );
        } else {
            let res = client.get_account(&pubkey).await;
            if let Ok(_) = res {
                panic!("Expected closed account: {:?}", pubkey);
            }
        }
    }
    let fetched_block = fetch_block_with_account_data(client.clone(), block.block_metadata.slot)
        .await
        .unwrap();

    let mut fetched_block_counts = HashMap::new();

    println!("Block slot: {:?}", block.block_metadata.slot);
    for owner in DAS_ACCOUNTS.iter() {
        let mut i: i32 = 0;
        println!("Owner: {:?}", owner);

        for account in block.das_accounts.iter() {
            let (pubkey, (account, _slot)) = account;
            if let Some(account) = account {
                if account.owner == *owner {
                    if !fetched_block.das_accounts.contains_key(&pubkey) {
                        println!("Missing account: {:?}", pubkey);
                        i += 1;
                        if i > 3 {
                            break;
                        }
                    }
                }
            }
        }
    };
    let mut i: i32 = 0;
    println!("Empty accounts");

    for account in block.das_accounts.iter() {
        let (pubkey, (account, _slot)) = account;
        if let Some(account) = account {
            if account.owner == solana_system_interface::program::id() {
                if !fetched_block.das_accounts.contains_key(&pubkey) {
                    println!("Missing account: {:?}", pubkey);
                    i += 1;
                    if i > 3 {
                        break;
                    }
                }
            }
        }
    }
    fetched_block_counts.insert(solana_system_interface::program::id(), 0);

    for account in fetched_block.das_accounts {
        let (_pubkey, (account, _slot)) = account;
        if let Some(account) = account {
            let count = fetched_block_counts.entry(account.owner).or_insert(0);
            *count += 1;
        } else {
            let owner = solana_system_interface::program::id();
            let count = fetched_block_counts.entry(owner).or_insert(0);
            *count += 1;
        }
    }
    #[derive(Debug)]
    #[allow(dead_code)]
    pub struct Stats {
        pub account_counts: HashMap<Pubkey, u64>,
        pub transaction_counts: usize,
    }
    let grpc_block_stats = Stats {
        account_counts: counts,
        transaction_counts: block.das_transactions.len(),
    };
    let poller_block_stats = Stats {
        account_counts: fetched_block_counts,
        transaction_counts: fetched_block.das_transactions.len(),
    };
    println!("{:?}", grpc_block_stats);
    println!("{:?}", poller_block_stats);

    assert!(block.das_transactions.len() > 0);
    for tx in block.das_transactions {
        assert!(is_bubblegum_transaction(tx.clone()));
        let meta = tx.meta.unwrap();
        assert!(meta.status.is_ok(), "Transaction status indicates failure");
    }
}
