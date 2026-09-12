use {
    anyhow::Context,
    log::info,
    mpl_token_metadata::accounts::Metadata,
    plerkle_messenger::ACCOUNT_STREAM,
    plerkle_serialization::{
        serializer::serialize_account, solana_geyser_plugin_interface_shims::ReplicaAccountInfoV2,
    },
    solana_account_decoder::{UiAccount, UiAccountEncoding},
    solana_client::{
        nonblocking::rpc_client::RpcClient,
        rpc_config::RpcAccountInfoConfig,
        rpc_request::RpcRequest,
        rpc_response::{Response as RpcResponse, RpcTokenAccountBalance},
    },
    borsh::BorshDeserialize,
    solana_commitment_config::{CommitmentConfig, CommitmentLevel},
    solana_sdk::{account::Account, pubkey::Pubkey},
    std::{str::FromStr, sync::Arc},
    tokio::sync::Mutex,
    txn_forwarder::rpc_tx_with_retries,
};

/// fetch account from node and send it to redis
pub async fn fetch_and_send_account(
    pubkey: Pubkey,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
    ok_to_fail: bool,
) -> anyhow::Result<()> {
    let fetch_result = fetch_account(pubkey, client).await;
    let (account, slot) = match fetch_result {
        Ok((account, slot)) => (account, slot),
        Err(e) => {
            if ok_to_fail {
                return Ok(());
            } else {
                return Err(anyhow::anyhow!("Failed to fetch account: {:?}", e));
            }
        }
    };
    send_account(pubkey, account, slot, messenger).await
}

/// returns largest (NFT related) token account belonging to mint
pub async fn get_token_largest_account(client: &RpcClient, mint: Pubkey) -> anyhow::Result<Pubkey> {
    let response: RpcResponse<Vec<RpcTokenAccountBalance>> = rpc_tx_with_retries(
        client,
        RpcRequest::Custom {
            method: "getTokenLargestAccounts",
        },
        serde_json::json!([mint.to_string(),]),
        3,
        mint,
    )
    .await?;

    match response.value.first() {
        Some(account) => Pubkey::from_str(&account.address)
            .with_context(|| format!("failed to parse account for mint {mint}")),
        None => anyhow::bail!("no accounts for mint {mint}: burned nft?"),
    }
}

pub async fn get_token_largest_accounts(
    client: &RpcClient,
    mint: Pubkey,
) -> anyhow::Result<Vec<Pubkey>> {
    let response: RpcResponse<Vec<RpcTokenAccountBalance>> = rpc_tx_with_retries(
        client,
        RpcRequest::Custom {
            method: "getTokenLargestAccounts",
        },
        serde_json::json!([mint.to_string(),]),
        3,
        mint,
    )
    .await?;

    let pubkeys: Vec<Pubkey> = response
        .value
        .iter()
        .map(|account| Pubkey::from_str(&account.address))
        .collect::<Result<_, _>>()
        .with_context(|| format!("failed to parse accounts for mint {mint}"))?;

    if pubkeys.is_empty() {
        anyhow::bail!("no accounts for mint {mint}: burned nft?");
    }

    Ok(pubkeys)
}

/// fetch account and slot with retries
pub async fn fetch_account(pubkey: Pubkey, client: &RpcClient) -> anyhow::Result<(Account, u64)> {
    const CONFIG: RpcAccountInfoConfig = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64Zstd),
        commitment: Some(CommitmentConfig {
            commitment: CommitmentLevel::Finalized,
        }),
        data_slice: None,
        min_context_slot: None,
    };

    let response: RpcResponse<Option<UiAccount>> = rpc_tx_with_retries(
        client,
        RpcRequest::GetAccountInfo,
        serde_json::json!([pubkey.to_string(), CONFIG]),
        3,
        pubkey,
    )
    .await
    .with_context(|| format!("failed to get account {pubkey}"))?;

    let account: Account = response
        .value
        .ok_or_else(|| anyhow::anyhow!("failed to get account {pubkey}"))
        .and_then(|ui_account| {
            decode_ui_account(ui_account)
                .ok_or_else(|| anyhow::anyhow!("failed to parse account {pubkey}"))
        })?;

    Ok((account, response.context.slot))
}

fn decode_ui_account(ui_account: UiAccount) -> Option<Account> {
    Some(Account {
        lamports: ui_account.lamports,
        data: ui_account.data.decode()?,
        owner: Pubkey::from_str(&ui_account.owner).ok()?,
        executable: ui_account.executable,
        rent_epoch: ui_account.rent_epoch,
    })
}

/// send account data to redis
pub async fn send_account(
    pubkey: Pubkey,
    account: Account,
    slot: u64,
    messenger: &Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
) -> anyhow::Result<()> {
    let fbb = flatbuffers::FlatBufferBuilder::new();

    let account_info = ReplicaAccountInfoV2 {
        pubkey: &pubkey.to_bytes(),
        lamports: account.lamports,
        owner: &account.owner.to_bytes(),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
        data: &account.data,
        write_version: 0,
        txn_signature: None,
    };
    let is_startup = false;

    let fbb = serialize_account(fbb, &account_info, slot, is_startup);
    let bytes = fbb.finished_data();

    messenger.lock().await.send(ACCOUNT_STREAM, bytes).await?;
    info!("sent account {} to stream", pubkey);

    Ok(())
}

/// fetch metadata account and send mint account to redis
pub async fn fetch_metadata_and_send_accounts(
    pubkey: Pubkey,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
) -> anyhow::Result<()> {
    let (account, _slot) = fetch_account(pubkey, client).await?;
    let metadata: Metadata = Metadata::deserialize(&mut &account.data[..])
        .with_context(|| anyhow::anyhow!("failed to parse data for metadata account {pubkey}"))?;

    info!("Fetching token largest accounts: {:?}", metadata.mint);
    let token_account = get_token_largest_account(client, metadata.mint).await?;

    for pubkey in &[metadata.mint, pubkey, token_account] {
        fetch_and_send_account(*pubkey, client, messenger, false).await?;
    }
    Ok(())
}
