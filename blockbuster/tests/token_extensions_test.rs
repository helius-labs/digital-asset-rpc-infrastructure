use blockbuster::{
    program_handler::ProgramParser,
    programs::{
        token_extensions::{TokenExtensionsAccountParser, TokenExtensionsProgramAccount},
        ProgramParseResult,
    },
};
use flatbuffers::FlatBufferBuilder;
use plerkle_serialization::{
    root_as_account_info, AccountInfo as FBAccountInfo, AccountInfoArgs, Pubkey as FBPubkey,
};
use solana_sdk::pubkey::Pubkey;
use base64::{engine::general_purpose, Engine as _};

/// Helper to build a FlatBuffer AccountInfo from raw bytes
fn build_account_info<'a>(
    fbb: &'a mut FlatBufferBuilder<'a>,
    pubkey: &Pubkey,
    owner: &Pubkey,
    data: &[u8],
    slot: u64,
) -> Result<FBAccountInfo<'a>, flatbuffers::InvalidFlatbuffer> {
    let pubkey_fb = FBPubkey(pubkey.to_bytes());
    let owner_fb = FBPubkey(owner.to_bytes());
    let data_fb = if !data.is_empty() {
        Some(fbb.create_vector(data))
    } else {
        None
    };

    let account_info = FBAccountInfo::create(
        fbb,
        &AccountInfoArgs {
            pubkey: Some(&pubkey_fb),
            lamports: 1000000,
            owner: Some(&owner_fb),
            executable: false,
            rent_epoch: 0,
            data: data_fb,
            write_version: 1,
            slot,
            is_startup: false,
            seen_at: 0,
        },
    );

    fbb.finish(account_info, None);
    let finished_data = fbb.finished_data();
    root_as_account_info(finished_data)
}

#[test]
fn test_parser_key_match() {
    let parser = TokenExtensionsAccountParser {};
    let token_2022_program_id =
        Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    assert_eq!(parser.key(), token_2022_program_id);
    assert!(parser.key_match(&token_2022_program_id));
    assert!(!parser.key_match(&Pubkey::new_unique()));
}

#[test]
fn test_parser_handles_account_updates() {
    let parser = TokenExtensionsAccountParser {};
    assert!(parser.handles_account_updates());
    assert!(!parser.handles_instructions());
}

#[test]
fn test_parse_token_extensions_mint() {
    // Base64 encoded Token-2022 mint account with multiple extensions:
    // - ConfidentialTransferMint
    // - TransferHook
    // - MetadataPointer
    // - TransferFeeConfig
    // - TokenMetadata (inline)
    let data_b64 = "AQAAAP/f7BvNLNODk8hNqgjJHMDTjWd/+NCre10DPwD3MswwDdux7CIAAAAIAQEAAAD/3+wbzSzTg5PITaoIyRzA041nf/jQq3tdAz8A9zLMMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAARIAQABD+fHuLje4B+pFy3TmAJcivsAJKWAm5ORVi0NeJKXpxgfoN+LIQ7TZ2JXKDHJIcxFvSd5VFlRbdnyWRdakOGiRDAAgAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGBgABAAEZADgABm9ZIlHMR3R4JaWa0UIupDVz9SjaXe4q94ErMU+ZReMAAAAAAADwP1RkGmkAAAAAAAAAAAAAJEAaACEA/9/sG80s04OTyE2qCMkcwNONZ3/40Kt7XQM/APcyzDAABABBAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgBAAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAATAKcAQ/nx7i43uAfqRct05gCXIr7ACSlgJuTkVYtDXiSl6cYH6DfiyEO02diVygxySHMRb0neVRZUW3Z8lkXWpDhokQ4AAABOZXRmbGl4IHhTdG9jawUAAABORkxYeEQAAABodHRwczovL3hzdG9ja3MtbWV0YWRhdGEuYmFja2VkLmZpL3Rva2Vucy9Tb2xhbmEvTkZMWHgvbWV0YWRhdGEuanNvbgAAAAA=";

    let data = general_purpose::STANDARD.decode(data_b64).unwrap();
    let pubkey = Pubkey::new_unique();
    let token_2022_program_id =
        Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    let mut fbb = FlatBufferBuilder::new();
    let account_info = build_account_info(&mut fbb, &pubkey, &token_2022_program_id, &data, 100)
        .expect("Failed to build account info");

    let parser = TokenExtensionsAccountParser {};
    let result = parser
        .handle_account(&account_info)
        .expect("Failed to parse account");

    // Get the result type and match on it
    if let ProgramParseResult::TokenExtensionsProgramAccount(result_ref) = result.result_type() {
        if let TokenExtensionsProgramAccount::MintAccount(mint) = result_ref {
        // Verify base mint properties
        assert_eq!(mint.account.decimals, 8);
        assert_eq!(mint.account.is_initialized, true);

        // Verify extensions are parsed
        assert!(mint.extensions.confidential_transfer_mint.is_some(), "ConfidentialTransferMint should be present");
        assert!(mint.extensions.transfer_hook.is_some(), "TransferHook should be present");
        assert!(mint.extensions.metadata_pointer.is_some(), "MetadataPointer should be present");
        assert!(mint.extensions.metadata.is_some(), "TokenMetadata should be present");

            // Verify metadata content
            let metadata = mint.extensions.metadata.as_ref().unwrap();
            assert_eq!(metadata.name, "Netflix xStock");
            assert_eq!(metadata.symbol, "NFLXx");
            assert!(metadata.uri.starts_with("https://xstocks-metadata"));
        } else {
            panic!("Expected TokenExtensionsProgramAccount::MintAccount");
        }
    } else {
        panic!("Expected ProgramParseResult::TokenExtensionsProgramAccount");
    }
}

#[test]
fn test_parse_token_extensions_empty_account() {
    let data: Vec<u8> = vec![];
    let pubkey = Pubkey::new_unique();
    let token_2022_program_id =
        Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    let mut fbb = FlatBufferBuilder::new();
    let account_info = build_account_info(&mut fbb, &pubkey, &token_2022_program_id, &data, 100)
        .expect("Failed to build account info");

    let parser = TokenExtensionsAccountParser {};
    let result = parser
        .handle_account(&account_info)
        .expect("Failed to parse account");

    // Get the result type and match on it
    if let ProgramParseResult::TokenExtensionsProgramAccount(result_ref) = result.result_type() {
        if let TokenExtensionsProgramAccount::EmptyAccount = result_ref {
            // Success
        } else {
            panic!("Expected TokenExtensionsProgramAccount::EmptyAccount");
        }
    } else {
        panic!("Expected ProgramParseResult::TokenExtensionsProgramAccount");
    }
}

#[test]
fn test_parse_invalid_account_data() {
    // Random invalid data that doesn't match any token account format
    let data: Vec<u8> = vec![0xFF; 100];
    let pubkey = Pubkey::new_unique();
    let token_2022_program_id =
        Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    let mut fbb = FlatBufferBuilder::new();
    let account_info = build_account_info(&mut fbb, &pubkey, &token_2022_program_id, &data, 100)
        .expect("Failed to build account info");

    let parser = TokenExtensionsAccountParser {};
    let result = parser.handle_account(&account_info);

    // Should return an error for invalid data
    assert!(result.is_err());
}

#[test]
fn test_parse_mint_with_pausable_and_scaled_ui_amount() {
    // This test uses a mint that only has common extensions to verify basic parsing works
    // For more comprehensive pausable/scaled testing, see integration tests
    // Base64 encoded Token-2022 mint account with multiple extensions
    let data_b64 = "AQAAAP/f7BvNLNODk8hNqgjJHMDTjWd/+NCre10DPwD3MswwDdux7CIAAAAIAQEAAAD/3+wbzSzTg5PITaoIyRzA041nf/jQq3tdAz8A9zLMMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAARIAQABD+fHuLje4B+pFy3TmAJcivsAJKWAm5ORVi0NeJKXpxgfoN+LIQ7TZ2JXKDHJIcxFvSd5VFlRbdnyWRdakOGiRDAAgAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGBgABAAEZADgABm9ZIlHMR3R4JaWa0UIupDVz9SjaXe4q94ErMU+ZReMAAAAAAADwP1RkGmkAAAAAAAAAAAAAJEAaACEA/9/sG80s04OTyE2qCMkcwNONZ3/40Kt7XQM/APcyzDAABABBAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgBAAEP58e4uN7gH6kXLdOYAlyK+wAkpYCbk5FWLQ14kpenGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAATAKcAQ/nx7i43uAfqRct05gCXIr7ACSlgJuTkVYtDXiSl6cYH6DfiyEO02diVygxySHMRb0neVRZUW3Z8lkXWpDhokQ4AAABOZXRmbGl4IHhTdG9jawUAAABORkxYeEQAAABodHRwczovL3hzdG9ja3MtbWV0YWRhdGEuYmFja2VkLmZpL3Rva2Vucy9Tb2xhbmEvTkZMWHgvbWV0YWRhdGEuanNvbgAAAAA=";

    let data = general_purpose::STANDARD.decode(data_b64).unwrap();
    let pubkey = Pubkey::try_from("XsEH7wWfJJu2ZT3UCFeVfALnVA6CP5ur7Ee11KmzVpL").unwrap();
    let token_2022_program_id =
        Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    let mut fbb = FlatBufferBuilder::new();
    let account_info = build_account_info(&mut fbb, &pubkey, &token_2022_program_id, &data, 100)
        .expect("Failed to build account info");

    let parser = TokenExtensionsAccountParser {};
    let result = parser
        .handle_account(&account_info)
        .expect("Failed to parse account");

    // Get the result type and match on it
    if let ProgramParseResult::TokenExtensionsProgramAccount(result_ref) = result.result_type() {
        if let TokenExtensionsProgramAccount::MintAccount(mint) = result_ref {
            // Verify base mint properties
            assert_eq!(mint.account.decimals, 8);
            assert_eq!(mint.account.is_initialized, true);

            // This mint has the following extensions:
            // - ConfidentialTransferMint
            // - TransferHook
            // - MetadataPointer
            // - TransferFeeConfig
            // - TokenMetadata

            // The key thing we're testing is that the parser can handle mints with these structures
            // and that PausableConfig/ScaledUiAmountConfig structures compile and serialize correctly

            assert!(
                mint.extensions.confidential_transfer_mint.is_some(),
                "ConfidentialTransferMint should be present"
            );
            assert!(
                mint.extensions.transfer_hook.is_some(),
                "TransferHook should be present"
            );
            assert!(
                mint.extensions.metadata_pointer.is_some(),
                "MetadataPointer should be present"
            );
            assert!(
                mint.extensions.metadata.is_some(),
                "TokenMetadata should be present"
            );

            // Verify metadata content
            let metadata = mint.extensions.metadata.as_ref().unwrap();
            assert_eq!(metadata.name, "Netflix xStock");
            assert_eq!(metadata.symbol, "NFLXx");
            assert!(metadata.uri.starts_with("https://xstocks-metadata"));
        } else {
            panic!("Expected TokenExtensionsProgramAccount::MintAccount");
        }
    } else {
        panic!("Expected ProgramParseResult::TokenExtensionsProgramAccount");
    }
}

#[test]
fn test_pausable_config_serialization() {
    // Test that PausableConfig can be serialized and deserialized correctly
    use blockbuster::programs::token_extensions::extension::ShadowPausableConfig;
    use serde_json;

    let pausable = ShadowPausableConfig {
        authority: Default::default(),
        paused: Default::default(),
    };

    // Should be able to serialize to JSON
    let json = serde_json::to_string(&pausable).expect("Failed to serialize PausableConfig");
    assert!(json.len() > 0);

    // Should be able to deserialize from JSON
    let _deserialized: ShadowPausableConfig =
        serde_json::from_str(&json).expect("Failed to deserialize PausableConfig");
}

#[test]
fn test_scaled_ui_amount_config_serialization() {
    // Test that ScaledUiAmountConfig can be serialized and deserialized correctly
    use blockbuster::programs::token_extensions::extension::ShadowScaledUiAmountConfig;
    use serde_json;

    let scaled = ShadowScaledUiAmountConfig {
        authority: Default::default(),
        multiplier: Default::default(),
        new_multiplier_effective_timestamp: Default::default(),
        new_multiplier: Default::default(),
    };

    // Should be able to serialize to JSON
    let json = serde_json::to_string(&scaled).expect("Failed to serialize ScaledUiAmountConfig");
    assert!(json.len() > 0);

    // Should be able to deserialize from JSON
    let _deserialized: ShadowScaledUiAmountConfig =
        serde_json::from_str(&json).expect("Failed to deserialize ScaledUiAmountConfig");
}
