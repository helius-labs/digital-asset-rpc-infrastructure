/// Conversion functions from yellowstone gRPC proto types to Solana SDK types.
///
/// These were previously provided by `yellowstone_grpc_proto::convert_from` (behind the
/// `convert` feature), which was removed in yellowstone-grpc-proto v12. The functions
/// here are adapted from the v9 implementation to support the v12 proto types.
use solana_account_decoder::parse_token::UiTokenAmount;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_message::v0::{LoadedAddresses, Message as MessageV0, MessageAddressTableLookup};
use solana_message::v1::{Message as MessageV1, TransactionConfig};
use solana_message::{Message, MessageHeader, VersionedMessage};
use solana_sdk::account::Account;
use solana_sdk::hash::{Hash, HASH_BYTES};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_context::transaction::TransactionReturnData;
use solana_transaction_error::TransactionError;
use solana_transaction_status::{
    InnerInstruction, InnerInstructions, Reward, RewardType, TransactionStatusMeta,
    TransactionTokenBalance, TransactionWithStatusMeta, VersionedTransactionWithStatusMeta,
};
use yellowstone_grpc_proto::geyser::{
    SubscribeUpdateAccountInfo, SubscribeUpdateTransactionInfo,
};
use yellowstone_grpc_proto::prelude as proto;

type CreateResult<T> = Result<T, &'static str>;

pub fn create_account(
    mut account: SubscribeUpdateAccountInfo,
) -> CreateResult<(Pubkey, Account)> {
    let pubkey = create_pubkey(&account.pubkey)?;
    let account_data = std::mem::take(&mut account.data);
    let account = Account {
        lamports: account.lamports,
        data: account_data,
        owner: create_pubkey(&account.owner)?,
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    };
    Ok((pubkey, account))
}

pub fn create_tx_with_meta(
    tx: SubscribeUpdateTransactionInfo,
) -> CreateResult<TransactionWithStatusMeta> {
    let meta = tx.meta.ok_or("failed to get transaction meta")?;
    let tx = tx
        .transaction
        .ok_or("failed to get transaction transaction")?;

    Ok(TransactionWithStatusMeta::Complete(
        VersionedTransactionWithStatusMeta {
            transaction: create_tx_versioned(tx)?,
            meta: create_tx_meta(meta)?,
        },
    ))
}

fn create_tx_versioned(tx: proto::Transaction) -> CreateResult<solana_sdk::transaction::VersionedTransaction> {
    let mut signatures = Vec::with_capacity(tx.signatures.len());
    for signature in tx.signatures {
        signatures.push(match Signature::try_from(signature.as_slice()) {
            Ok(signature) => signature,
            Err(_error) => return Err("failed to parse Signature"),
        });
    }

    Ok(solana_sdk::transaction::VersionedTransaction {
        signatures,
        message: create_message(tx.message.ok_or("failed to get message")?)?,
    })
}

fn create_message(message: proto::Message) -> CreateResult<VersionedMessage> {
    let header = message.header.ok_or("failed to get MessageHeader")?;
    let header = MessageHeader {
        num_required_signatures: header
            .num_required_signatures
            .try_into()
            .map_err(|_| "failed to parse num_required_signatures")?,
        num_readonly_signed_accounts: header
            .num_readonly_signed_accounts
            .try_into()
            .map_err(|_| "failed to parse num_readonly_signed_accounts")?,
        num_readonly_unsigned_accounts: header
            .num_readonly_unsigned_accounts
            .try_into()
            .map_err(|_| "failed to parse num_readonly_unsigned_accounts")?,
    };

    if message.recent_blockhash.len() != HASH_BYTES {
        return Err("failed to parse hash");
    }

    // The proto sets `config` only for v1 messages (SIMD-0385); it is absent
    // for legacy/v0 (requires yellowstone-grpc-proto >= 12.6 server-side).
    let config = match message.config {
        // A v1 message cannot have address table lookups. If the stream ever
        // sends both, convert as v0 below (preserving all keys) instead of
        // erroring, because the gRPC caller unwraps conversion errors.
        Some(_) if !message.address_table_lookups.is_empty() => {
            log::error!("v1-flagged gRPC message has address table lookups; converting as v0");
            None
        }
        config => config,
    };
    Ok(if let Some(config) = config {
        VersionedMessage::V1(MessageV1 {
            header,
            config: TransactionConfig {
                priority_fee: config.priority_fee,
                compute_unit_limit: config.compute_unit_limit,
                loaded_accounts_data_size_limit: config.loaded_accounts_data_size_limit,
                heap_size: config.heap_size,
            },
            lifetime_specifier: Hash::new_from_array(
                <[u8; HASH_BYTES]>::try_from(message.recent_blockhash.as_slice()).unwrap(),
            ),
            account_keys: create_pubkey_vec(message.account_keys)?,
            instructions: create_message_instructions(message.instructions)?,
        })
    } else if message.versioned {
        let mut address_table_lookups = Vec::with_capacity(message.address_table_lookups.len());
        for table in message.address_table_lookups {
            address_table_lookups.push(MessageAddressTableLookup {
                account_key: Pubkey::try_from(table.account_key.as_slice())
                    .map_err(|_| "failed to parse Pubkey")?,
                writable_indexes: table.writable_indexes,
                readonly_indexes: table.readonly_indexes,
            });
        }

        VersionedMessage::V0(MessageV0 {
            header,
            account_keys: create_pubkey_vec(message.account_keys)?,
            recent_blockhash: Hash::new_from_array(
                <[u8; HASH_BYTES]>::try_from(message.recent_blockhash.as_slice()).unwrap(),
            ),
            instructions: create_message_instructions(message.instructions)?,
            address_table_lookups,
        })
    } else {
        VersionedMessage::Legacy(Message {
            header,
            account_keys: create_pubkey_vec(message.account_keys)?,
            recent_blockhash: Hash::new_from_array(
                <[u8; HASH_BYTES]>::try_from(message.recent_blockhash.as_slice()).unwrap(),
            ),
            instructions: create_message_instructions(message.instructions)?,
        })
    })
}

fn create_message_instructions(
    ixs: Vec<proto::CompiledInstruction>,
) -> CreateResult<Vec<CompiledInstruction>> {
    ixs.into_iter().map(create_message_instruction).collect()
}

fn create_message_instruction(
    ix: proto::CompiledInstruction,
) -> CreateResult<CompiledInstruction> {
    Ok(CompiledInstruction {
        program_id_index: ix
            .program_id_index
            .try_into()
            .map_err(|_| "failed to decode CompiledInstruction.program_id_index)")?,
        accounts: ix.accounts,
        data: ix.data,
    })
}

fn create_tx_meta(
    meta: proto::TransactionStatusMeta,
) -> CreateResult<TransactionStatusMeta> {
    let meta_status = match create_tx_error(meta.err.as_ref())? {
        Some(err) => Err(err),
        None => Ok(()),
    };
    let meta_rewards = meta
        .rewards
        .into_iter()
        .map(create_reward)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TransactionStatusMeta {
        status: meta_status,
        fee: meta.fee,
        pre_balances: meta.pre_balances,
        post_balances: meta.post_balances,
        inner_instructions: Some(create_meta_inner_instructions(meta.inner_instructions)?),
        log_messages: Some(meta.log_messages),
        pre_token_balances: Some(create_token_balances(meta.pre_token_balances)?),
        post_token_balances: Some(create_token_balances(meta.post_token_balances)?),
        rewards: Some(meta_rewards),
        loaded_addresses: create_loaded_addresses(
            meta.loaded_writable_addresses,
            meta.loaded_readonly_addresses,
        )?,
        return_data: if meta.return_data_none {
            None
        } else {
            let data = meta.return_data.ok_or("failed to get return_data")?;
            Some(TransactionReturnData {
                program_id: Pubkey::try_from(data.program_id.as_slice())
                    .map_err(|_| "failed to parse program_id")?,
                data: data.data,
            })
        },
        compute_units_consumed: meta.compute_units_consumed,
        cost_units: meta.cost_units,
    })
}

fn create_tx_error(
    err: Option<&proto::TransactionError>,
) -> CreateResult<Option<TransactionError>> {
    err.map(|err| bincode::deserialize::<TransactionError>(&err.err))
        .transpose()
        .map_err(|_| "failed to decode TransactionError")
}

fn create_meta_inner_instructions(
    ixs: Vec<proto::InnerInstructions>,
) -> CreateResult<Vec<InnerInstructions>> {
    ixs.into_iter().map(create_meta_inner_instruction).collect()
}

fn create_meta_inner_instruction(
    ix: proto::InnerInstructions,
) -> CreateResult<InnerInstructions> {
    let mut instructions = vec![];
    for inner_ix in ix.instructions {
        instructions.push(InnerInstruction {
            instruction: CompiledInstruction {
                program_id_index: inner_ix
                    .program_id_index
                    .try_into()
                    .map_err(|_| "failed to decode CompiledInstruction.program_id_index)")?,
                accounts: inner_ix.accounts,
                data: inner_ix.data,
            },
            stack_height: inner_ix.stack_height,
        });
    }
    Ok(InnerInstructions {
        index: ix
            .index
            .try_into()
            .map_err(|_| "failed to decode InnerInstructions.index")?,
        instructions,
    })
}

fn create_reward(reward: proto::Reward) -> CreateResult<Reward> {
    Ok(Reward {
        pubkey: reward.pubkey,
        lamports: reward.lamports,
        post_balance: reward.post_balance,
        reward_type: match proto::RewardType::try_from(reward.reward_type)
            .map_err(|_| "failed to parse reward_type")?
        {
            proto::RewardType::Unspecified => None,
            proto::RewardType::Fee => Some(RewardType::Fee),
            proto::RewardType::Rent => Some(RewardType::Rent),
            proto::RewardType::Staking => Some(RewardType::Staking),
            proto::RewardType::Voting => Some(RewardType::Voting),
            proto::RewardType::DeactivatedStake => Some(RewardType::DeactivatedStake),
        },
        commission: if reward.commission.is_empty() {
            None
        } else {
            Some(
                reward
                    .commission
                    .parse()
                    .map_err(|_| "failed to parse reward commission")?,
            )
        },
        commission_bps: if reward.commission_bps.is_empty() {
            None
        } else {
            Some(
                reward
                    .commission_bps
                    .parse()
                    .map_err(|_| "failed to parse reward commission_bps")?,
            )
        },
    })
}

fn create_token_balances(
    balances: Vec<proto::TokenBalance>,
) -> CreateResult<Vec<TransactionTokenBalance>> {
    let mut vec = Vec::with_capacity(balances.len());
    for balance in balances {
        let ui_amount = balance
            .ui_token_amount
            .ok_or("failed to get ui_token_amount")?;
        vec.push(TransactionTokenBalance {
            account_index: balance
                .account_index
                .try_into()
                .map_err(|_| "failed to parse account_index")?,
            mint: balance.mint,
            ui_token_amount: UiTokenAmount {
                ui_amount: Some(ui_amount.ui_amount),
                decimals: ui_amount
                    .decimals
                    .try_into()
                    .map_err(|_| "failed to parse decimals")?,
                amount: ui_amount.amount,
                ui_amount_string: ui_amount.ui_amount_string,
            },
            owner: balance.owner,
            program_id: balance.program_id,
        });
    }
    Ok(vec)
}

fn create_loaded_addresses(
    writable: Vec<Vec<u8>>,
    readonly: Vec<Vec<u8>>,
) -> CreateResult<LoadedAddresses> {
    Ok(LoadedAddresses {
        writable: create_pubkey_vec(writable)?,
        readonly: create_pubkey_vec(readonly)?,
    })
}

fn create_pubkey_vec(pubkeys: Vec<Vec<u8>>) -> CreateResult<Vec<Pubkey>> {
    pubkeys
        .iter()
        .map(|pubkey| create_pubkey(pubkey.as_slice()))
        .collect()
}

fn create_pubkey(pubkey: &[u8]) -> CreateResult<Pubkey> {
    Pubkey::try_from(pubkey).map_err(|_| "failed to parse Pubkey")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto_message(
        versioned: bool,
        config: Option<proto::TransactionConfig>,
    ) -> proto::Message {
        proto::Message {
            header: Some(proto::MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            }),
            account_keys: vec![
                Pubkey::new_unique().to_bytes().to_vec(),
                Pubkey::new_unique().to_bytes().to_vec(),
            ],
            recent_blockhash: vec![7; HASH_BYTES],
            instructions: vec![],
            versioned,
            address_table_lookups: vec![],
            config,
        }
    }

    #[test]
    fn message_with_config_is_v1() {
        let config = proto::TransactionConfig {
            priority_fee: Some(42),
            compute_unit_limit: Some(200_000),
            loaded_accounts_data_size_limit: None,
            heap_size: None,
        };
        let message = create_message(proto_message(true, Some(config))).unwrap();
        let VersionedMessage::V1(v1) = message else {
            panic!("expected V1, got {message:?}");
        };
        assert_eq!(v1.config.priority_fee, Some(42));
        assert_eq!(v1.config.compute_unit_limit, Some(200_000));
        assert_eq!(v1.account_keys.len(), 2);
    }

    #[test]
    fn message_without_config_keeps_v0_and_legacy() {
        assert!(matches!(
            create_message(proto_message(true, None)).unwrap(),
            VersionedMessage::V0(_)
        ));
        assert!(matches!(
            create_message(proto_message(false, None)).unwrap(),
            VersionedMessage::Legacy(_)
        ));
    }

    #[test]
    fn v1_with_address_table_lookups_falls_back_to_v0() {
        let mut message = proto_message(true, Some(proto::TransactionConfig::default()));
        message.address_table_lookups.push(proto::MessageAddressTableLookup {
            account_key: Pubkey::new_unique().to_bytes().to_vec(),
            writable_indexes: vec![0],
            readonly_indexes: vec![],
        });
        // Contradictory input (v1 cannot have lookups) must not error, because
        // the gRPC caller unwraps; it degrades to v0 with all keys intact.
        let message = create_message(message).unwrap();
        let VersionedMessage::V0(v0) = message else {
            panic!("expected V0 fallback, got {message:?}");
        };
        assert_eq!(v0.address_table_lookups.len(), 1);
    }
}
