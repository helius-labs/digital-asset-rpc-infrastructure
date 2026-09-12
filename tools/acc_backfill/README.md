# Account Backfill

This tool fetches accounts from a Solana node and sends them to a Redis stream. It is used to backfill accounts into the system.

## Usage

```bash
cargo run -- --redis_url $REDIS_URL --rpc_url $RPC_URL --start_slot $START_SLOT --end_slot $END_SLOT
```

This sends accounts to the backfill stream. Increase the number of acc-backfill workers to run it. Track progress in the DAS dashboard.
