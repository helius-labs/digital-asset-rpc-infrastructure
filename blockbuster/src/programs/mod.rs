use self::account_closure::AccountClosureData;
use agent_registry::AgentRegistryAccount;
use bubblegum::BubblegumInstruction;
use mpl_core_program::MplCoreAccountState;
use token_account::TokenProgramAccount;
use token_extensions::TokenExtensionsProgramAccount;
use token_metadata::TokenMetadataAccountState;

pub mod account_closure;
pub mod agent_registry;
pub mod bubblegum;
pub mod mpl_core_program;
pub mod token_account;
pub mod token_extensions;
pub mod token_metadata;

pub enum ProgramParseResult<'a> {
    Bubblegum(&'a BubblegumInstruction),
    AgentRegistry(&'a AgentRegistryAccount),
    TokenMetadata(&'a TokenMetadataAccountState),
    TokenExtensionsMetadata(&'a TokenMetadataAccountState),
    TokenProgramAccount(&'a TokenProgramAccount),
    TokenExtensionsProgramAccount(&'a TokenExtensionsProgramAccount),
    AccountClosure(&'a AccountClosureData),
    MplCore(&'a MplCoreAccountState),
    Unknown,
}
