use bytemuck::Zeroable;
use serde::{Deserialize, Serialize};
use spl_pod::{
    optional_keys::{OptionalNonZeroElGamalPubkey, OptionalNonZeroPubkey},
    primitives::{PodBool, PodI64, PodU16, PodU64},
};
use solana_zk_sdk_pod::encryption::{
    auth_encryption::PodAeCiphertext,
    elgamal::{PodElGamalCiphertext, PodElGamalPubkey},
};

use spl_token_2022::extension::{
    confidential_transfer::{ConfidentialTransferAccount, ConfidentialTransferMint},
    confidential_transfer_fee::{ConfidentialTransferFeeAmount, ConfidentialTransferFeeConfig},
    cpi_guard::CpiGuard,
    default_account_state::DefaultAccountState,
    group_member_pointer::GroupMemberPointer,
    group_pointer::GroupPointer,
    immutable_owner::ImmutableOwner,
    interest_bearing_mint::InterestBearingConfig,
    memo_transfer::MemoTransfer,
    metadata_pointer::MetadataPointer,
    mint_close_authority::MintCloseAuthority,
    pausable::PausableConfig,
    permanent_delegate::PermanentDelegate,
    scaled_ui_amount::{PodF64, ScaledUiAmountConfig},
    transfer_fee::{TransferFee, TransferFeeAmount, TransferFeeConfig},
    transfer_hook::TransferHook,
};
use std::fmt;

use spl_token_group_interface::state::{TokenGroup, TokenGroupMember};
use spl_token_metadata_interface::state::TokenMetadata;

const AE_CIPHERTEXT_LEN: usize = 36;
const UNIT_LEN: usize = 32;
const RISTRETTO_POINT_LEN: usize = UNIT_LEN;
pub(crate) const DECRYPT_HANDLE_LEN: usize = RISTRETTO_POINT_LEN;
pub(crate) const PEDERSEN_COMMITMENT_LEN: usize = RISTRETTO_POINT_LEN;
const ELGAMAL_PUBKEY_LEN: usize = RISTRETTO_POINT_LEN;
const ELGAMAL_CIPHERTEXT_LEN: usize = PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN;
type PodAccountState = u8;
pub type UnixTimestamp = PodI64;
pub type EncryptedBalance = ShadowElGamalCiphertext;
pub type DecryptableBalance = ShadowAeCiphertext;
pub type EncryptedWithheldAmount = ShadowElGamalCiphertext;

use serde::{
    de::{self, SeqAccess, Visitor},
    Deserializer, Serializer,
};

/// Bs58 encoded public key string. Used for storing Pubkeys in a human readable format.
/// Ideally we'd store them as is in the DB and later convert them to bs58 for display on the API.
/// But,
/// - We currently store them in DB as JSONB.
/// - `Pubkey` serializes to an u8 vector, unlike sth like `OptionalNonZeroElGamalPubkey` which serializes to a string.
///    So `Pubkey` is stored as a u8 vector in the DB.
/// - `Pubkey` doesn't implement something like `schemars::JsonSchema` so we can't convert them back to the rust struct either.
type PublicKeyString = String;

struct ShadowAeCiphertextVisitor;

struct ShadowElGamalCiphertextVisitor;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowAeCiphertext(pub [u8; AE_CIPHERTEXT_LEN]);

#[derive(Clone, Copy, Debug, PartialEq, Zeroable)]
pub struct ShadowElGamalCiphertext(pub [u8; ELGAMAL_CIPHERTEXT_LEN]);

#[derive(Clone, Copy, Debug, Default, Zeroable, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowElGamalPubkey(pub [u8; ELGAMAL_PUBKEY_LEN]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowCpiGuard {
    pub lock_cpi: PodBool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowDefaultAccountState {
    pub state: PodAccountState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowImmutableOwner;

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowInterestBearingConfig {
    pub rate_authority: OptionalNonZeroPubkey,
    pub initialization_timestamp: i64,
    pub pre_update_average_rate: i16,
    pub last_update_timestamp: i64,
    pub current_rate: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowMemoTransfer {
    /// Require transfers into this account to be accompanied by a memo
    pub require_incoming_transfer_memos: PodBool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowMetadataPointer {
    pub authority: OptionalNonZeroPubkey,
    pub metadata_address: OptionalNonZeroPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowGroupMemberPointer {
    pub authority: OptionalNonZeroPubkey,
    pub member_address: OptionalNonZeroPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowGroupPointer {
    /// Authority that can set the group address
    pub authority: OptionalNonZeroPubkey,
    /// Account address that holds the group
    pub group_address: OptionalNonZeroPubkey,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadowTokenGroup {
    /// The authority that can sign to update the group
    pub update_authority: OptionalNonZeroPubkey,
    /// The associated mint, used to counter spoofing to be sure that group
    /// belongs to a particular mint
    pub mint: PublicKeyString,
    /// The current number of group members
    pub size: PodU64,
    /// The maximum number of group members
    pub max_size: PodU64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadowTokenGroupMember {
    /// The associated mint, used to counter spoofing to be sure that member
    /// belongs to a particular mint
    pub mint: PublicKeyString,
    /// The pubkey of the `TokenGroup`
    pub group: PublicKeyString,
    /// The member number
    pub member_number: PodU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowMintCloseAuthority {
    pub close_authority: OptionalNonZeroPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct NonTransferableAccount;

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowPermanentDelegate {
    pub delegate: OptionalNonZeroPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowTransferFee {
    pub epoch: PodU64,
    pub maximum_fee: PodU64,
    pub transfer_fee_basis_points: PodU16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowTransferHook {
    pub authority: OptionalNonZeroPubkey,
    pub program_id: OptionalNonZeroPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowPausableConfig {
    pub authority: OptionalNonZeroPubkey,
    pub paused: PodBool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowScaledUiAmountConfig {
    pub authority: OptionalNonZeroPubkey,
    pub multiplier: PodF64,
    pub new_multiplier_effective_timestamp: UnixTimestamp,
    pub new_multiplier: PodF64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowConfidentialTransferMint {
    pub authority: OptionalNonZeroPubkey,
    pub auto_approve_new_accounts: PodBool,
    pub auditor_elgamal_pubkey: OptionalNonZeroElGamalPubkey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadowConfidentialTransferAccount {
    pub approved: PodBool,
    pub elgamal_pubkey: ShadowElGamalPubkey,
    pub pending_balance_lo: EncryptedBalance,
    pub pending_balance_hi: EncryptedBalance,
    pub available_balance: EncryptedBalance,
    pub decryptable_available_balance: DecryptableBalance,
    pub allow_confidential_credits: PodBool,
    pub allow_non_confidential_credits: PodBool,
    pub pending_balance_credit_counter: PodU64,
    pub maximum_pending_balance_credit_counter: PodU64,
    pub expected_pending_balance_credit_counter: PodU64,
    pub actual_pending_balance_credit_counter: PodU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowConfidentialTransferFeeConfig {
    pub authority: OptionalNonZeroPubkey,
    pub withdraw_withheld_authority_elgamal_pubkey: ShadowElGamalPubkey,
    pub harvest_to_mint_enabled: PodBool,
    pub withheld_amount: EncryptedWithheldAmount,
}

pub struct ShadowConfidentialTransferFeeAmount {
    pub withheld_amount: EncryptedWithheldAmount,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowTransferFeeAmount {
    pub withheld_amount: PodU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Zeroable, Serialize, Deserialize)]
pub struct ShadowTransferFeeConfig {
    pub transfer_fee_config_authority: OptionalNonZeroPubkey,
    pub withdraw_withheld_authority: OptionalNonZeroPubkey,
    pub withheld_amount: PodU64,
    pub older_transfer_fee: ShadowTransferFee,
    pub newer_transfer_fee: ShadowTransferFee,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadowMetadata {
    pub update_authority: OptionalNonZeroPubkey,
    pub mint: PublicKeyString,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub additional_metadata: Vec<(String, String)>,
}

impl ShadowMetadata {
    pub fn sanitize(&mut self) {
        self.name = self.name.trim().replace('\0', "").to_string();
        self.symbol = self.symbol.trim().replace('\0', "").to_string();
        self.uri = self.uri.trim().replace('\0', "").to_string();
    }
}

impl Serialize for ShadowAeCiphertext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Visitor<'de> for ShadowElGamalCiphertextVisitor {
    type Value = ShadowElGamalCiphertext;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a byte array of length ELGAMAL_CIPHERTEXT_LEN")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v.len() == ELGAMAL_CIPHERTEXT_LEN {
            let mut arr = [0u8; ELGAMAL_CIPHERTEXT_LEN];
            arr.copy_from_slice(v);
            Ok(ShadowElGamalCiphertext(arr))
        } else {
            Err(E::invalid_length(v.len(), &self))
        }
    }
}

impl<'de> Deserialize<'de> for ShadowElGamalCiphertext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(ShadowElGamalCiphertextVisitor)
    }
}

impl Default for ShadowElGamalCiphertext {
    fn default() -> Self {
        ShadowElGamalCiphertext([0u8; ELGAMAL_CIPHERTEXT_LEN])
    }
}

impl Default for ShadowAeCiphertext {
    fn default() -> Self {
        ShadowAeCiphertext([0u8; AE_CIPHERTEXT_LEN])
    }
}

impl<'de> Visitor<'de> for ShadowAeCiphertextVisitor {
    type Value = ShadowAeCiphertext;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a byte array of length AE_CIPHERTEXT_LEN")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut arr = [0u8; AE_CIPHERTEXT_LEN];
        for i in 0..AE_CIPHERTEXT_LEN {
            arr[i] = seq
                .next_element()?
                .ok_or(de::Error::invalid_length(i, &self))?;
        }
        Ok(ShadowAeCiphertext(arr))
    }
}

impl<'de> Deserialize<'de> for ShadowAeCiphertext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(AE_CIPHERTEXT_LEN, ShadowAeCiphertextVisitor)
    }
}

impl Serialize for ShadowElGamalCiphertext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl From<PodAeCiphertext> for ShadowAeCiphertext {
    fn from(original: PodAeCiphertext) -> Self {
        let bytes = bytemuck::bytes_of(&original);
        let mut arr = [0u8; AE_CIPHERTEXT_LEN];
        arr.copy_from_slice(&bytes[..AE_CIPHERTEXT_LEN]);
        ShadowAeCiphertext(arr)
    }
}

impl From<PodElGamalCiphertext> for ShadowElGamalCiphertext {
    fn from(original: PodElGamalCiphertext) -> Self {
        let bytes = bytemuck::bytes_of(&original);
        let mut arr = [0u8; ELGAMAL_CIPHERTEXT_LEN];
        arr.copy_from_slice(&bytes[..ELGAMAL_CIPHERTEXT_LEN]);
        ShadowElGamalCiphertext(arr)
    }
}

impl From<PodElGamalPubkey> for ShadowElGamalPubkey {
    fn from(original: PodElGamalPubkey) -> Self {
        let bytes = bytemuck::bytes_of(&original);
        let mut arr = [0u8; ELGAMAL_PUBKEY_LEN];
        arr.copy_from_slice(&bytes[..ELGAMAL_PUBKEY_LEN]);
        ShadowElGamalPubkey(arr)
    }
}

impl From<CpiGuard> for ShadowCpiGuard {
    fn from(original: CpiGuard) -> Self {
        ShadowCpiGuard {
            lock_cpi: original.lock_cpi,
        }
    }
}

impl From<DefaultAccountState> for ShadowDefaultAccountState {
    fn from(original: DefaultAccountState) -> Self {
        ShadowDefaultAccountState {
            state: original.state,
        }
    }
}

impl From<ImmutableOwner> for ShadowImmutableOwner {
    fn from(_: ImmutableOwner) -> Self {
        ShadowImmutableOwner
    }
}

impl From<ConfidentialTransferFeeAmount> for ShadowConfidentialTransferFeeAmount {
    fn from(original: ConfidentialTransferFeeAmount) -> Self {
        ShadowConfidentialTransferFeeAmount {
            withheld_amount: original.withheld_amount.into(),
        }
    }
}

impl From<TransferFeeAmount> for ShadowTransferFeeAmount {
    fn from(original: TransferFeeAmount) -> Self {
        ShadowTransferFeeAmount {
            withheld_amount: original.withheld_amount,
        }
    }
}

impl From<MemoTransfer> for ShadowMemoTransfer {
    fn from(original: MemoTransfer) -> Self {
        ShadowMemoTransfer {
            require_incoming_transfer_memos: original.require_incoming_transfer_memos,
        }
    }
}

// spl-token-2022 11.x changed optional pubkeys to `MaybeNull<Pubkey>`; both types
// encode `None` as all-zero, so `.get().unwrap_or_default()` converts losslessly.
impl From<MetadataPointer> for ShadowMetadataPointer {
    fn from(original: MetadataPointer) -> Self {
        ShadowMetadataPointer {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            metadata_address: OptionalNonZeroPubkey(
                original.metadata_address.get().unwrap_or_default(),
            ),
        }
    }
}

impl From<GroupPointer> for ShadowGroupPointer {
    fn from(original: GroupPointer) -> Self {
        ShadowGroupPointer {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            group_address: OptionalNonZeroPubkey(
                original.group_address.get().unwrap_or_default(),
            ),
        }
    }
}

impl From<TokenGroup> for ShadowTokenGroup {
    fn from(original: TokenGroup) -> Self {
        ShadowTokenGroup {
            update_authority: OptionalNonZeroPubkey(
                original.update_authority.get().unwrap_or_default(),
            ),
            mint: bs58::encode(original.mint).into_string(),
            size: original.size,
            max_size: original.max_size,
        }
    }
}

impl From<TokenGroupMember> for ShadowTokenGroupMember {
    fn from(original: TokenGroupMember) -> Self {
        ShadowTokenGroupMember {
            mint: bs58::encode(original.mint).into_string(),
            group: bs58::encode(original.group).into_string(),
            member_number: original.member_number,
        }
    }
}

impl From<GroupMemberPointer> for ShadowGroupMemberPointer {
    fn from(original: GroupMemberPointer) -> Self {
        ShadowGroupMemberPointer {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            member_address: OptionalNonZeroPubkey(
                original.member_address.get().unwrap_or_default(),
            ),
        }
    }
}

impl From<TransferFee> for ShadowTransferFee {
    fn from(original: TransferFee) -> Self {
        ShadowTransferFee {
            epoch: original.epoch,
            maximum_fee: original.maximum_fee,
            transfer_fee_basis_points: original.transfer_fee_basis_points,
        }
    }
}

impl From<TransferFeeConfig> for ShadowTransferFeeConfig {
    fn from(original: TransferFeeConfig) -> Self {
        ShadowTransferFeeConfig {
            transfer_fee_config_authority: OptionalNonZeroPubkey(
                original.transfer_fee_config_authority.get().unwrap_or_default(),
            ),
            withdraw_withheld_authority: OptionalNonZeroPubkey(
                original.withdraw_withheld_authority.get().unwrap_or_default(),
            ),
            withheld_amount: original.withheld_amount,
            older_transfer_fee: ShadowTransferFee::from(original.older_transfer_fee),
            newer_transfer_fee: ShadowTransferFee::from(original.newer_transfer_fee),
        }
    }
}

impl From<InterestBearingConfig> for ShadowInterestBearingConfig {
    fn from(original: InterestBearingConfig) -> Self {
        ShadowInterestBearingConfig {
            rate_authority: OptionalNonZeroPubkey(
                original.rate_authority.get().unwrap_or_default(),
            ),
            initialization_timestamp: i64::from(original.initialization_timestamp),
            pre_update_average_rate: i16::from(original.pre_update_average_rate),
            last_update_timestamp: i64::from(original.last_update_timestamp),
            current_rate: i16::from(original.current_rate),
        }
    }
}

impl From<MintCloseAuthority> for ShadowMintCloseAuthority {
    fn from(original: MintCloseAuthority) -> Self {
        ShadowMintCloseAuthority {
            close_authority: OptionalNonZeroPubkey(
                original.close_authority.get().unwrap_or_default(),
            ),
        }
    }
}

impl From<PermanentDelegate> for ShadowPermanentDelegate {
    fn from(original: PermanentDelegate) -> Self {
        ShadowPermanentDelegate {
            delegate: OptionalNonZeroPubkey(original.delegate.get().unwrap_or_default()),
        }
    }
}

impl From<TransferHook> for ShadowTransferHook {
    fn from(original: TransferHook) -> Self {
        ShadowTransferHook {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            program_id: OptionalNonZeroPubkey(original.program_id.get().unwrap_or_default()),
        }
    }
}

impl From<PausableConfig> for ShadowPausableConfig {
    fn from(original: PausableConfig) -> Self {
        ShadowPausableConfig {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            paused: original.paused,
        }
    }
}

impl From<ScaledUiAmountConfig> for ShadowScaledUiAmountConfig {
    fn from(original: ScaledUiAmountConfig) -> Self {
        ShadowScaledUiAmountConfig {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            multiplier: original.multiplier,
            new_multiplier_effective_timestamp: original.new_multiplier_effective_timestamp,
            new_multiplier: original.new_multiplier,
        }
    }
}

impl From<ConfidentialTransferMint> for ShadowConfidentialTransferMint {
    fn from(original: ConfidentialTransferMint) -> Self {
        ShadowConfidentialTransferMint {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            auto_approve_new_accounts: original.auto_approve_new_accounts,
            auditor_elgamal_pubkey: bytemuck::pod_read_unaligned(bytemuck::bytes_of(
                &original.auditor_elgamal_pubkey.get().unwrap_or_default(),
            )),
        }
    }
}

impl From<ConfidentialTransferAccount> for ShadowConfidentialTransferAccount {
    fn from(original: ConfidentialTransferAccount) -> Self {
        ShadowConfidentialTransferAccount {
            approved: original.approved,
            elgamal_pubkey: original.elgamal_pubkey.into(),
            pending_balance_lo: original.pending_balance_lo.into(),
            pending_balance_hi: original.pending_balance_hi.into(),
            available_balance: original.available_balance.into(),
            decryptable_available_balance: original.decryptable_available_balance.into(),
            allow_confidential_credits: original.allow_confidential_credits,
            allow_non_confidential_credits: original.allow_non_confidential_credits,
            pending_balance_credit_counter: original.pending_balance_credit_counter,
            maximum_pending_balance_credit_counter: original
                .maximum_pending_balance_credit_counter,
            expected_pending_balance_credit_counter: original
                .expected_pending_balance_credit_counter,
            actual_pending_balance_credit_counter: original
                .actual_pending_balance_credit_counter,
        }
    }
}

impl From<ConfidentialTransferFeeConfig> for ShadowConfidentialTransferFeeConfig {
    fn from(original: ConfidentialTransferFeeConfig) -> Self {
        ShadowConfidentialTransferFeeConfig {
            authority: OptionalNonZeroPubkey(original.authority.get().unwrap_or_default()),
            withdraw_withheld_authority_elgamal_pubkey: original
                .withdraw_withheld_authority_elgamal_pubkey.into(),
            harvest_to_mint_enabled: original.harvest_to_mint_enabled,
            withheld_amount: original.withheld_amount.into(),
        }
    }
}

impl From<TokenMetadata> for ShadowMetadata {
    fn from(original: TokenMetadata) -> Self {
        ShadowMetadata {
            update_authority: OptionalNonZeroPubkey(
                original.update_authority.get().unwrap_or_default(),
            ),
            mint: bs58::encode(original.mint).into_string(),
            name: original.name,
            symbol: original.symbol,
            uri: original.uri,
            additional_metadata: original.additional_metadata,
        }
    }
}
