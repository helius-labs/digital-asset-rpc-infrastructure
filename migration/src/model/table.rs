use enum_iterator::Sequence;
use sea_orm_migration::prelude::*;

#[derive(Copy, Clone, Iden)]
pub enum AssetCreators {
    Table,
    Id,
    AssetId,
    Creator,
    Share,
    Verified,
    Seq,
}

#[derive(Copy, Clone, Iden)]
pub enum AssetAuthority {
    Table,
    Id,
    AssetId,
    Authority,
    SlotUpdated,
    Seq,
}

#[derive(Copy, Clone, Iden)]
pub enum AssetGrouping {
    Table,
    Id,
    GroupKey,
    GroupValue,
    Seq,
    SlotUpdated,
    Verified,
    GroupInfoSeq,
}

#[derive(Copy, Clone, Iden)]
pub enum BackfillItems {
    Table,
    Id,
    Tree,
    Seq,
    Slot,
    ForceChk,
    Backfilled,
    Failed,
    Locked,
}

#[derive(Copy, Clone, Iden)]
pub enum Asset {
    Table,
    Id,
    AltId,
    SpecificationVersion,
    SpecificationAssetClass,
    Owner,
    OwnerType,
    Delegate,
    Frozen,
    Supply,
    SupplyMint,
    Compressed,
    Compressible,
    Seq,
    TreeId,
    Leaf,
    Nonce,
    RoyaltyTargetType,
    RoyaltyTarget,
    RoyaltyAmount,
    AssetData,
    CreatedAt,
    Burnt,
    SlotUpdatedMetadataAccount,
    SlotUpdatedMintAccount,
    SlotUpdatedTokenAccount,
    SlotUpdatedCnftTransaction,
    DataHash,
    CreatorHash,
    OwnerDelegateSeq,
    WasDecompressed,
    LeafSeq,
    CreatorsInfo,
    CollectionsInfo,
    AuthorititiesInfo,
    MintExtensions,
    TokenExtensions,
    MetadataAccountId,
    BaseInfoSeq,
    MplCorePlugins,
    MplCoreUnknownPlugins,
    MplCoreCollectionNumMinted,
    MplCoreCollectionCurrentSize,
    MplCorePluginsJsonVersion,
    MplCoreExternalPlugins,
    MplCoreUnknownExternalPlugins,
    CollectionHash,
    AssetDataHash,
    BubblegumFlags,
    NonTransferable,
    IsAgent,
    AgentToken,
    AssetSigner,
    SlotUpdatedAgentRegistry,
}

#[derive(Copy, Clone, Iden)]
pub enum AssetData {
    Table,
    Id,
    ChainData,
    ChainDataMutability,
    RawName,
    RawSymbol,
    SlotUpdated,
}

#[derive(Copy, Clone, Iden)]
pub enum AssetDataV2 {
    Table,
    Id,
    MetadataUrl,
    ChainData,
    ChainDataMutability,
    RawName,
    RawSymbol,
    SlotUpdated,
    BaseInfoSeq,
}

#[derive(Copy, Clone, Iden)]
pub enum OffchainMetadata {
    Table,
    Id,
    MetadataUrl,
    Mutability,
    Metadata,
    CreatedAt,
    UpdatedAt,
    Reindex,
}

#[derive(Copy, Clone, Iden)]
pub enum Tasks {
    Table,
    TaskType,
    Data,
    Status,
    CreatedAt,
    LockedUntil,
    LockedBy,
    MaxAttempts,
    Attempts,
    Duration,
    Errors,
}

#[derive(Copy, Clone, Iden)]
pub enum TokenAccounts {
    Table,
    Pubkey,
    Mint,
    Amount,
    Owner,
    Frozen,
    CloseAuthority,
    Delegate,
    DelegatedAmount,
    SlotUpdated,
    TokenProgram,
}

#[derive(Copy, Clone, Iden)]
pub enum Tokens {
    Table,
    Mint,
    Supply,
    Decimals,
    TokenProgram,
    MintAuthority,
    FreezeAuthority,
    CloseAuthority,
    ExtensionData,
    SlotUpdated,
    Extensions,
}

#[derive(Copy, Clone, Iden)]
pub enum ClAudits {
    Table,
    Id,
    Tree,
    NodeIdx,
    LeafIdx,
    Seq,
    Level,
    Hash,
    CreatedAt,
    Tx,
}

#[derive(Copy, Clone, Iden)]
pub enum ClAuditsV2 {
    Table,
    Id,
    Tree,
    LeafIdx,
    Seq,
    CreatedAt,
    Tx,
    Instruction,
}

#[derive(Copy, Clone, Iden)]
pub enum Owners {
    Table,
    Id,
    Owner,
    Mint,
    TokenAccount,
    Delegate,
    SlotUpdated,
    OwnerDelegateSeq,
    CreatedAt,
}

#[derive(Iden, Debug, PartialEq, Sequence)]
pub enum EditionAccountType {
    Edition,
    EditionMarker,
    MasterEditionV1,
    MasterEditionV2,
    Unknown,
}

#[derive(Iden)]
pub enum Editions {
    EditionAccountType,
    Table,
    Id,
    Parent,
    EditionType,
    Data,
    SlotUpdated,
}

#[derive(Copy, Clone, Iden)]
pub enum Blocks {
    Table,
    Slot,
    ParentSlot,
    Blockhash,
    ParentBlockhash,
    BlockHeight,
    BlockTime,
}
