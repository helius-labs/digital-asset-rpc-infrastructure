// We are copying this code from the latest version spl-token-22 because
// it's incredibly difficult to bump the version of spl-token-22 due to dependency hell.

use solana_sdk::program_error::ProgramError;
use spl_pod::bytemuck::{pod_from_bytes, pod_get_packed_len};
use spl_token_2022::{
    error::TokenError,
    extension::{
        BaseState, BaseStateWithExtensions, Extension, ExtensionType, Length, StateWithExtensions,
    },
    state::Mint,
};
use spl_type_length_value::variable_len_pack::VariableLenPack;

pub fn get_variable_len_extension<V: Extension + VariableLenPack>(
    state: &StateWithExtensions<Mint>,
) -> Result<V, ProgramError> {
    let data = get_extension_bytes::<Mint, V>(state.get_tlv_data())?;
    V::unpack_from_slice(data)
}

fn get_extension_bytes<S: BaseState, V: Extension>(tlv_data: &[u8]) -> Result<&[u8], ProgramError> {
    if V::TYPE.get_account_type() != S::ACCOUNT_TYPE {
        return Err(ProgramError::InvalidAccountData);
    }
    let TlvIndices {
        type_start: _,
        length_start,
        value_start,
    } = get_extension_indices::<V>(tlv_data, false)?;
    // get_extension_indices has checked that tlv_data is long enough to include
    // these indices
    let length = pod_from_bytes::<Length>(&tlv_data[length_start..value_start])
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let value_end = value_start.saturating_add(usize::from(*length));
    if tlv_data.len() < value_end {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(&tlv_data[value_start..value_end])
}
/// Helper struct for returning the indices of the type, length, and value in
/// a TLV entry
#[derive(Debug)]
struct TlvIndices {
    pub type_start: usize,
    pub length_start: usize,
    pub value_start: usize,
}

fn get_extension_indices<V: Extension>(
    tlv_data: &[u8],
    init: bool,
) -> Result<TlvIndices, ProgramError> {
    let mut start_index = 0;
    while start_index < tlv_data.len() {
        let tlv_indices = get_tlv_indices(start_index);
        if tlv_data.len() < tlv_indices.value_start {
            return Err(ProgramError::InvalidAccountData);
        }
        let extension_type = u16::from_le_bytes(
            tlv_data[tlv_indices.type_start..tlv_indices.length_start]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        );
        if extension_type == u16::from(V::TYPE) {
            // found an instance of the extension that we're initializing, return!
            return Ok(tlv_indices);
        // got to an empty spot, init here, or error if we're searching, since
        // nothing is written after an Uninitialized spot
        } else if extension_type == u16::from(ExtensionType::Uninitialized) {
            if init {
                return Ok(tlv_indices);
            } else {
                return Err(TokenError::ExtensionNotFound.into());
            }
        } else {
            let length = pod_from_bytes::<Length>(
                &tlv_data[tlv_indices.length_start..tlv_indices.value_start],
            ).map_err(|_| ProgramError::InvalidAccountData)?;
            let value_end_index = tlv_indices.value_start.saturating_add(usize::from(*length));
            start_index = value_end_index;
        }
    }
    Err(ProgramError::InvalidAccountData)
}

/// Helper function to get the current TlvIndices from the current spot
fn get_tlv_indices(type_start: usize) -> TlvIndices {
    let length_start = type_start.saturating_add(size_of::<ExtensionType>());
    let value_start = length_start.saturating_add(pod_get_packed_len::<Length>());
    TlvIndices {
        type_start,
        length_start,
        value_start,
    }
}
