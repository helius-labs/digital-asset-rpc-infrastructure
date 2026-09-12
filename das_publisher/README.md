# DAS Ingester

The DAS ingester:

1. Pulls transaction and account updates from a gRPC connection. It uses an RPC client as fallback.
2. Pushes the data to Redis queues.
3. Stores the last processed slot in PostgreSQL.
4. Auto-heals if there is any issues by continuing from the last processed slot.

# Running Locally

Setup Redis and PostgreSQL locally. See [Building DAS locally](https://www.notion.so/helius-api/Building-DAS-locally-8ea99fef76124123862383c0c1bb7b00#d42ae94967db4eb88eb8de4f755b3611) for instructions.

Then run:

```
export ENV=local
cargo run
```

# Using GPRC

By default, the ingest only uses the RPC client. To use the gRPC client, set the `INGESTER_GRPC_URL` and `INGESTER_GRPC_AUTH_HEADER` environment variables.

# Configuring a start slot

The ingester can be configured to start from a specific slot using the `start_slot` argument. If no start slot is provided, the ingester will start from the last indexed slot in the database. If no last indexed slot is found, the ingester will start from the current slot in the network.
