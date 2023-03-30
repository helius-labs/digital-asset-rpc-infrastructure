use std::str::FromStr;

use clap::Parser;
use figment::{util::map, value::Value};

use plerkle_messenger::MessengerConfig;
use plerkle_serialization::{
    serializer::serialize_account, solana_geyser_plugin_interface_shims::ReplicaAccountInfoV2,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey};
#[derive(Parser)]
#[command(next_line_help = true)]
struct Cli {
    #[arg(long)]
    redis_url: String,
    #[arg(long)]
    rpc_url: String,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand, Clone)]
enum Action {
    Single {
        #[arg(long)]
        account: String,
    },
    Scenario {
        #[arg(long)]
        scenario_file: String,
    },
}
const STREAM: &str = "ACC";

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config_wrapper = Value::from(map! {
    "redis_connection_str" => cli.redis_url,
    "pipeline_size_bytes" => 1u128.to_string(),
     });
    let config = config_wrapper.into_dict().unwrap();
    let messenger_config = MessengerConfig {
        messenger_type: plerkle_messenger::MessengerType::Redis,
        connection_config: config,
    };
    let mut messenger = plerkle_messenger::select_messenger(messenger_config)
        .await
        .unwrap();
    messenger.add_stream(STREAM).await.unwrap();
    messenger.set_buffer_size(STREAM, 10000000000000000).await;

    let client = RpcClient::new(cli.rpc_url.clone());

    let cmd = cli.action;

    match cmd {
        Action::Single { account } => send_account(&account, &client, &mut messenger).await,
        Action::Scenario { scenario_file } => {
            let scenario = std::fs::read_to_string(scenario_file).unwrap();
            let scenario: Vec<String> = scenario.lines().map(|s| s.to_string()).collect();
            for account in scenario {
                send_account(&account, &client, &mut messenger).await;
            }
        }
    }
}
pub async fn send_account(
    account: &str,
    client: &RpcClient,
    messenger: &mut Box<dyn plerkle_messenger::Messenger>,
) {
    let account = Pubkey::from_str(account).expect("Failed to parse mint as pubkey");
    let account_data: Account = client
        .get_account_with_commitment(&account, CommitmentConfig::confirmed())
        .await
        .expect("Failed to get account")
        .value
        .expect("Account not found");

    send(account, account_data, messenger).await
}

pub async fn send(
    pubkey: Pubkey,
    account_data: Account,
    messenger: &mut Box<dyn plerkle_messenger::Messenger>,
) {
    let fbb = flatbuffers::FlatBufferBuilder::new();

    let account_info = ReplicaAccountInfoV2 {
        pubkey: &pubkey.to_bytes(),
        lamports: account_data.lamports,
        owner: &account_data.owner.to_bytes(),
        executable: account_data.executable,
        rent_epoch: account_data.rent_epoch,
        data: &account_data.data,
        write_version: 0,
        txn_signature: None,
    };
    let slot = 0;
    let is_startup = false;

    let fbb = serialize_account(fbb, &account_info, slot, is_startup);
    let bytes = fbb.finished_data();

    messenger.send(STREAM, bytes).await.unwrap();
    println!("Sent account to stream");
}
