//! SeaORM Entity. Generated by sea-orm-codegen 0.9.3

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "mutability")]
pub enum Mutability {
    #[sea_orm(string_value = "immutable")]
    Immutable,
    #[sea_orm(string_value = "mutable")]
    Mutable,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(
    rs_type = "String",
    db_type = "Enum",
    enum_name = "v1_account_attachments"
)]
pub enum V1AccountAttachments {
    #[sea_orm(string_value = "edition")]
    Edition,
    #[sea_orm(string_value = "edition_marker")]
    EditionMarker,
    #[sea_orm(string_value = "master_edition_v1")]
    MasterEditionV1,
    #[sea_orm(string_value = "master_edition_v2")]
    MasterEditionV2,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "task_status")]
pub enum TaskStatus {
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "running")]
    Running,
    #[sea_orm(string_value = "success")]
    Success,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(
    rs_type = "String",
    db_type = "Enum",
    enum_name = "royalty_target_type"
)]
pub enum RoyaltyTargetType {
    #[sea_orm(string_value = "creators")]
    Creators,
    #[sea_orm(string_value = "fanout")]
    Fanout,
    #[sea_orm(string_value = "single")]
    Single,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(
    rs_type = "String",
    db_type = "Enum",
    enum_name = "specification_asset_class"
)]
pub enum SpecificationAssetClass {
    #[sea_orm(string_value = "FUNGIBLE_ASSET")]
    FungibleAsset,
    #[sea_orm(string_value = "FUNGIBLE_TOKEN")]
    FungibleToken,
    #[sea_orm(string_value = "IDENTITY_NFT")]
    IdentityNft,
    #[sea_orm(string_value = "NFT")]
    Nft,
    #[sea_orm(string_value = "NON_TRANSFERABLE_NFT")]
    NonTransferableNft,
    #[sea_orm(string_value = "PRINT")]
    Print,
    #[sea_orm(string_value = "PRINTABLE_NFT")]
    PrintableNft,
    #[sea_orm(string_value = "PROGRAMMABLE_NFT")]
    ProgrammableNft,
    #[sea_orm(string_value = "TRANSFER_RESTRICTED_NFT")]
    TransferRestrictedNft,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "chain_mutability")]
pub enum ChainMutability {
    #[sea_orm(string_value = "immutable")]
    Immutable,
    #[sea_orm(string_value = "mutable")]
    Mutable,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(
    rs_type = "String",
    db_type = "Enum",
    enum_name = "specification_versions"
)]
pub enum SpecificationVersions {
    #[sea_orm(string_value = "unknown")]
    Unknown,
    #[sea_orm(string_value = "v0")]
    V0,
    #[sea_orm(string_value = "v1")]
    V1,
    #[sea_orm(string_value = "v2")]
    V2,
}
#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "owner_type")]
pub enum OwnerType {
    #[sea_orm(string_value = "single")]
    Single,
    #[sea_orm(string_value = "token")]
    Token,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "instruction")]
pub enum Instruction {
    #[sea_orm(string_value = "burn")]
    Burn,
    #[sea_orm(string_value = "cancel_redeem")]
    CancelRedeem,
    #[sea_orm(string_value = "compress")]
    Compress,
    #[sea_orm(string_value = "decompress_v1")]
    DecompressV1,
    #[sea_orm(string_value = "delegate")]
    Delegate,
    #[sea_orm(string_value = "mint_to_collection_v1")]
    MintToCollectionV1,
    #[sea_orm(string_value = "mint_v1")]
    MintV1,
    #[sea_orm(string_value = "redeem")]
    Redeem,
    #[sea_orm(string_value = "set_and_verify_collection")]
    SetAndVerifyCollection,
    #[sea_orm(string_value = "transfer")]
    Transfer,
    #[sea_orm(string_value = "unknown")]
    Unknown,
    #[sea_orm(string_value = "unverify_collection")]
    UnverifyCollection,
    #[sea_orm(string_value = "unverify_creator")]
    UnverifyCreator,
    #[sea_orm(string_value = "verify_collection")]
    VerifyCollection,
    #[sea_orm(string_value = "verify_creator")]
    VerifyCreator,
}
// Added manually for convenience.
impl Instruction {
    pub fn from_str(s: &str) -> Self {
        match s {
            "Burn" => Instruction::Burn,
            "CancelRedeem" => Instruction::CancelRedeem,
            "Compress" => Instruction::Compress,
            "DecompressV1" => Instruction::DecompressV1,
            "Delegate" => Instruction::Delegate,
            "MintToCollectionV1" => Instruction::MintToCollectionV1,
            "MintV1" => Instruction::MintV1,
            "Redeem" => Instruction::Redeem,
            "SetAndVerifyCollection" => Instruction::SetAndVerifyCollection,
            "Transfer" => Instruction::Transfer,
            "UnverifyCollection" => Instruction::UnverifyCollection,
            "UnverifyCreator" => Instruction::UnverifyCreator,
            "VerifyCollection" => Instruction::VerifyCollection,
            "VerifyCreator" => Instruction::VerifyCreator,
            _ => Instruction::Unknown,
        }
    }

    pub fn to_str(s: &Self) -> &str {
        match s {
            Instruction::Burn => "Burn",
            Instruction::CancelRedeem => "CancelReddem",
            Instruction::Compress => "Compress",
            Instruction::DecompressV1 => "DecompressV1",
            Instruction::Delegate => "Delegate",
            Instruction::MintToCollectionV1 => "MintToCollectionV1",
            Instruction::MintV1 => "MintV1",
            Instruction::Redeem => "Redeem",
            Instruction::SetAndVerifyCollection => "SetAndVerifyCollection",
            Instruction::Transfer => "Transfer",
            Instruction::UnverifyCollection => "UnverifyCollection",
            Instruction::UnverifyCreator => "UnverifyCreator",
            Instruction::VerifyCollection => "VerifyCollection",
            Instruction::VerifyCreator => "VerifyCreator",
            _ => "Unknown",
        }
    }
}
