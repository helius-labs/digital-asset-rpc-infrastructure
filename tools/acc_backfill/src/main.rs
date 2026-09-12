use acc_backfill::send_das_accounts_in_account_range;
use nft_ingester::config::init_logger;

use clap::Parser;

#[derive(Parser)]
#[command(next_line_help = true)]
struct Args {
    #[arg(long)]
    redis_url: String,
    #[arg(long)]
    rpc_url: String,
    #[arg(long)]
    start_slot: u64,
    #[arg(long)]
    end_slot: u64,
}

#[tokio::main]
async fn main() {
    init_logger();

    let args = Args::parse();

    send_das_accounts_in_account_range(
        args.rpc_url.clone(),
        args.redis_url.clone(),
        args.start_slot,
        args.end_slot,
    )
    .await;
}
