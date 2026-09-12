use log::warn;

use crate::{
    error::BlockbusterError,
    instruction::InstructionBundle,
    program_handler::{ParseResult, ProgramParser},
};

use crate::{program_handler::NotUsed, programs::ProgramParseResult};
use borsh::de::BorshDeserialize;
use mpl_bubblegum::{
    get_instruction_type,
    instructions::{
        MintV2InstructionArgs, UnverifyCreatorInstructionArgs, UnverifyCreatorV2InstructionArgs,
        UpdateMetadataInstructionArgs, UpdateMetadataV2InstructionArgs,
        VerifyCreatorInstructionArgs, VerifyCreatorV2InstructionArgs,
    },
    types::{BubblegumEventType, MetadataArgs, UpdateArgs},
};
pub use mpl_bubblegum::{types::LeafSchema, InstructionName, LeafSchemaEvent, ID};
use mpl_bubblegum::ID as BUBBLEGUM_ID;
use plerkle_serialization::AccountInfo;
use solana_sdk::pubkey::Pubkey;

const SPL_NOOP_ID: [u8; 32] = solana_sdk::pubkey!("noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV").to_bytes();

/// Deserialize Borsh instruction data, tolerating trailing bytes to match on-chain Anchor behavior.
#[inline]
fn deserialize_ix_data<T: BorshDeserialize>(data: &[u8]) -> std::io::Result<T> {
    T::deserialize(&mut &data[..])
}

#[derive(Eq, PartialEq)]
pub enum Payload {
    Unknown,
    Mint {
        args: MetadataArgs,
        authority: [u8; 32],
        tree_id: [u8; 32],
    },
    MintV1 {
        args: MetadataArgs,
        authority: [u8; 32],
        tree_id: [u8; 32],
    },
    Decompress {
        args: MetadataArgs,
    },
    CancelRedeem {
        root: [u8; 32],
    },
    CreatorVerification {
        metadata: MetadataArgs,
        creator: Pubkey,
        verify: bool,
    },
    CollectionVerification {
        collection: Pubkey,
        verify: bool,
    },
    UpdateMetadata {
        current_metadata: MetadataArgs,
        update_args: UpdateArgs,
        tree_id: [u8; 32],
    },
}
//TODO add more of the parsing here to minimize program transformer code
pub struct BubblegumInstruction {
    pub instruction: InstructionName,
    pub tree_update: Option<mpl_account_compression::events::ChangeLogEventV1>,
    pub leaf_update: Option<LeafSchemaEvent>,
    pub payload: Option<Payload>,
}

impl BubblegumInstruction {
    pub fn new(ix: InstructionName) -> Self {
        BubblegumInstruction {
            instruction: ix,
            tree_update: None,
            leaf_update: None,
            payload: None,
        }
    }
}

impl ParseResult for BubblegumInstruction {
    fn result_type(&self) -> ProgramParseResult<'_> {
        ProgramParseResult::Bubblegum(self)
    }
    fn result(&self) -> &Self
    where
        Self: Sized,
    {
        self
    }
}

pub struct BubblegumParser;

impl ProgramParser for BubblegumParser {
    fn key(&self) -> Pubkey {
        Pubkey::new_from_array(BUBBLEGUM_ID.to_bytes())
    }

    fn key_match(&self, key: &Pubkey) -> bool {
        key.to_bytes() == BUBBLEGUM_ID.to_bytes()
    }
    fn handles_account_updates(&self) -> bool {
        false
    }

    fn handles_instructions(&self) -> bool {
        true
    }
    fn handle_account(
        &self,
        _account_info: &AccountInfo,
    ) -> Result<Box<dyn ParseResult + 'static>, BlockbusterError> {
        Ok(Box::new(NotUsed::new()))
    }

    fn handle_instruction(
        &self,
        bundle: &InstructionBundle,
    ) -> Result<Box<dyn ParseResult + 'static>, BlockbusterError> {
        let InstructionBundle {
            txn_id,
            instruction,
            inner_ix,
            keys,
            ..
        } = bundle;
        let outer_ix_data = match instruction {
            Some(compiled_ix) if compiled_ix.data().is_some() => {
                let data = compiled_ix.data().unwrap();
                data.iter().collect::<Vec<_>>()
            }
            _ => {
                return Err(BlockbusterError::DeserializationError);
            }
        };
        let ix_type = get_instruction_type(&outer_ix_data);
        let mut b_inst = BubblegumInstruction::new(ix_type);
        if let Some(ixs) = inner_ix {
            for ix in ixs {
                if ix.0 .0 == SPL_NOOP_ID || ix.0 .0 == mpl_noop::id().to_bytes() {
                    let cix = ix.1;
                    if let Some(inner_ix_data) = cix.data() {
                        let inner_ix_data = inner_ix_data.iter().collect::<Vec<_>>();
                        if !inner_ix_data.is_empty() {
                            use mpl_account_compression::events::{
                                AccountCompressionEvent::{self, ApplicationData, ChangeLog},
                                ApplicationDataEvent, ChangeLogEvent,
                            };

                            match AccountCompressionEvent::try_from_slice(&inner_ix_data) {
                                Ok(result) => match result {
                                    ChangeLog(changelog_event) => {
                                        let ChangeLogEvent::V1(changelog_event) = changelog_event;
                                        b_inst.tree_update = Some(changelog_event);
                                    }
                                    ApplicationData(app_data) => {
                                        let ApplicationDataEvent::V1(app_data) = app_data;
                                        let app_data = app_data.application_data;

                                        let event_type_byte = if !app_data.is_empty() {
                                            &app_data[0..1]
                                        } else {
                                            return Err(BlockbusterError::DeserializationError);
                                        };

                                        match BubblegumEventType::try_from_slice(event_type_byte)? {
                                            BubblegumEventType::Uninitialized => {
                                                return Err(
                                                    BlockbusterError::MissingBubblegumEventData,
                                                );
                                            }
                                            BubblegumEventType::LeafSchemaEvent => {
                                                b_inst.leaf_update = Some(
                                                    LeafSchemaEvent::try_from_slice(&app_data)?,
                                                );
                                            }
                                        }
                                    }
                                },
                                Err(e) => {
                                    warn!(
                                        "Error while deserializing txn {:?} with noop data: {:?}",
                                        txn_id, e
                                    );
                                }
                            }
                        }
                    } else {
                        return Err(BlockbusterError::InstructionParsingError);
                    }
                }
            }
        }

        if outer_ix_data.len() >= 8 {
            let ix_data = &outer_ix_data[8..];
            if !ix_data.is_empty() {
                match b_inst.instruction {
                    InstructionName::MintV1 => {
                        b_inst.payload = Some(build_mint_v1_payload(keys, ix_data, false)?);
                    }

                    InstructionName::MintToCollectionV1 => {
                        b_inst.payload = Some(build_mint_v1_payload(keys, ix_data, true)?);
                    }
                    InstructionName::DecompressV1 => {
                        let args: MetadataArgs = deserialize_ix_data(ix_data)?;
                        b_inst.payload = Some(Payload::Decompress { args });
                    }
                    InstructionName::CancelRedeem => {
                        let slice: [u8; 32] = ix_data
                            .try_into()
                            .map_err(|_e| BlockbusterError::InstructionParsingError)?;
                        b_inst.payload = Some(Payload::CancelRedeem { root: slice });
                    }
                    InstructionName::VerifyCreator => {
                        b_inst.payload =
                            Some(build_creator_verification_payload(keys, ix_data, true)?);
                    }
                    InstructionName::UnverifyCreator => {
                        b_inst.payload =
                            Some(build_creator_verification_payload(keys, ix_data, false)?);
                    }
                    InstructionName::VerifyCollection | InstructionName::SetAndVerifyCollection => {
                        b_inst.payload = Some(build_collection_verification_payload(keys, true)?);
                    }
                    InstructionName::UnverifyCollection => {
                        b_inst.payload = Some(build_collection_verification_payload(keys, false)?);
                    }
                    InstructionName::UpdateMetadata => {
                        b_inst.payload = Some(build_update_metadata_payload(keys, ix_data)?);
                    }
                    InstructionName::MintV2 => {
                        b_inst.payload = Some(build_mint_v2_payload(keys, ix_data)?);
                    }
                    InstructionName::SetCollectionV2 => {
                        b_inst.payload = Some(build_set_collection_v2_payload(keys)?);
                    }
                    InstructionName::VerifyCreatorV2 => {
                        b_inst.payload =
                            Some(build_creator_verification_v2_payload(keys, ix_data, true)?);
                    }
                    InstructionName::UnverifyCreatorV2 => {
                        b_inst.payload =
                            Some(build_creator_verification_v2_payload(keys, ix_data, false)?);
                    }
                    InstructionName::UpdateMetadataV2 => {
                        b_inst.payload = Some(build_update_metadata_v2_payload(keys, ix_data)?);
                    }
                    InstructionName::UpdateAssetDataV2 => {} // Not supported
                    _ => {}
                };
            }
        }

        Ok(Box::new(b_inst))
    }
}

// See Bubblegum documentation for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_creator-and-unverify_creator
fn build_creator_verification_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
    verify: bool,
) -> Result<Payload, BlockbusterError> {
    let metadata = if verify {
        deserialize_ix_data::<VerifyCreatorInstructionArgs>(ix_data)?.metadata
    } else {
        deserialize_ix_data::<UnverifyCreatorInstructionArgs>(ix_data)?.metadata
    };

    let creator = keys
        .get(5)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    Ok(Payload::CreatorVerification {
        metadata,
        creator: Pubkey::new_from_array(creator),
        verify,
    })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_collection-unverify_collection-and-set_and_verify_collection
// This uses the account.  The collection is only provided as an argument for `set_and_verify_collection`.
fn build_collection_verification_payload(
    keys: &[plerkle_serialization::Pubkey],
    verify: bool,
) -> Result<Payload, BlockbusterError> {
    let collection_raw = keys
        .get(8)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;
    let collection: Pubkey = Pubkey::try_from_slice(&collection_raw)?;
    Ok(Payload::CollectionVerification { collection, verify })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md
fn build_mint_v1_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
    set_verify: bool,
) -> Result<Payload, BlockbusterError> {
    let mut args: MetadataArgs = deserialize_ix_data(ix_data)?;
    if set_verify {
        if let Some(ref mut col) = args.collection {
            col.verified = true;
        }
    }

    let authority = keys
        .first()
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    let tree_id = keys
        .get(3)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    Ok(Payload::Mint {
        args,
        authority,
        tree_id,
    })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md
fn build_update_metadata_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
) -> Result<Payload, BlockbusterError> {
    let args: UpdateMetadataInstructionArgs = deserialize_ix_data(ix_data)?;

    let tree_id = keys
        .get(8)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    Ok(Payload::UpdateMetadata {
        current_metadata: args.current_metadata,
        update_args: args.update_args,
        tree_id,
    })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md
fn build_mint_v2_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
) -> Result<Payload, BlockbusterError> {
    let args: MintV2InstructionArgs = deserialize_ix_data(ix_data)?;

    let authority = keys
        .first()
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    let tree_id = keys
        .get(6)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;

    Ok(Payload::Mint {
        args: args.metadata.into(),
        authority,
        tree_id,
    })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_collection-unverify_collection-and-set_and_verify_collection
// This uses the account.  The collection is only provided as an argument for `set_and_verify_collection`.
fn build_set_collection_v2_payload(
    keys: &[plerkle_serialization::Pubkey],
) -> Result<Payload, BlockbusterError> {
    let collection_raw = keys
        .get(8)
        .ok_or(BlockbusterError::InstructionParsingError)?
        .0;
    let collection: Pubkey = Pubkey::try_from_slice(&collection_raw)?;

    Ok(Payload::CollectionVerification {
        collection,
        verify: true,
    })
}

// See Bubblegum documentation for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_creator-and-unverify_creator
fn build_creator_verification_v2_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
    verify: bool,
) -> Result<Payload, BlockbusterError> {
    let metadata = if verify {
        deserialize_ix_data::<VerifyCreatorV2InstructionArgs>(ix_data)?.metadata
    } else {
        deserialize_ix_data::<UnverifyCreatorV2InstructionArgs>(ix_data)?.metadata
    };

    let payer = *keys
        .get(1)
        .ok_or(BlockbusterError::InstructionParsingError)?;

    let creator = *keys
        .get(2)
        .ok_or(BlockbusterError::InstructionParsingError)?;

    // Creator is optional in V2, None being signfied by the program ID.
    // Creator defaults to the payer.
    let creator = if creator.0 == mpl_bubblegum::ID.to_bytes() {
        payer
    } else {
        creator
    };

    Ok(Payload::CreatorVerification {
        metadata: metadata.into(),
        creator: Pubkey::new_from_array(creator.0),
        verify,
    })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md
fn build_update_metadata_v2_payload(
    keys: &[plerkle_serialization::Pubkey],
    ix_data: &[u8],
) -> Result<Payload, BlockbusterError> {
    let args: UpdateMetadataV2InstructionArgs = deserialize_ix_data(ix_data)?;

    let tree_id = *keys
        .get(5)
        .ok_or(BlockbusterError::InstructionParsingError)?;

    Ok(Payload::UpdateMetadata {
        current_metadata: args.current_metadata.into(),
        update_args: args.update_args,
        tree_id: tree_id.0,
    })
}
