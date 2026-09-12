use mint_backfill::backfill_mints;
use nft_ingester::config::init_logger;

use clap::Parser;



#[derive(Parser)]
#[command(
    name = "mint_backfill",
    about = "Backfill specific Token-2022 mints to Redis for DAS ingestion",
    long_about = "Fetches specific mint accounts by their addresses and sends them to Redis \
                  backfill stream for DAS processing. Supports both single mints and arrays of mints."
)]
#[command(next_line_help = true)]
struct Args {
    /// Mint addresses to backfill (comma-separated or multiple --mint flags)
    /// Example: --mint MINT1,MINT2,MINT3 or --mint MINT1 --mint MINT2
    #[arg(long, required = true, value_delimiter = ',', num_args = 1..)]
    mint: Vec<String>,

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

    backfill_mints(args.rpc_url, args.redis_url, args.mint).await;
}
