use program_backfill::backfill_program_accounts;
use nft_ingester::config::init_logger;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "program_backfill",
    about = "Backfill program accounts to Redis for DAS ingestion",
    long_about = "Fetches all accounts owned by a specified program using paginated RPC calls \
                  and sends them to Redis backfill stream for DAS processing."
)]
#[command(next_line_help = true)]
struct Args {
    /// Program ID to fetch accounts for (e.g., "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
    #[arg(long, required = true)]
    program_id: String,

    /// RPC URL to fetch accounts from (must be Helius mainnet RPC)
    #[arg(long, required = true)]
    rpc_url: String,

    /// Redis URL for account backfill stream
    #[arg(long, required = true)]
    redis_url: String,
}

#[tokio::main]
async fn main() {
    init_logger();

    let args = Args::parse();

    backfill_program_accounts(
        args.rpc_url,
        args.redis_url,
        args.program_id,
    )
    .await;
}
