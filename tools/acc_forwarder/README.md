# Account Forwarder

Account Forwarder is a tool designed to replay account changes, primarily originating from geyser. It forwards serialized account information to a specified Redis endpoint.

## Usage

### Send a single account

To forward information about a single account, run the following command:

`cargo run -- --redis-url <REDIS_URL> --rpc-url <RPC_URL> single --account <ACCOUNT>`

### Send mint, metadata, and owned token account

To forward mint, metadata, and owned token account information for a specific mint, use the following commands.

Locally:

```
cargo run -- --redis-url redis://localhost --rpc-url $RPC_URL mint --mint Aeg1zJKqECmspy5h9xMhp6VvtSjzW2acgBp2n4YjePkX
```

Dev/Prod:
`cargo run -- --redis-url $REDIS_URL --rpc-url $RPC_URL mint --mint t8nGUrFQozLtgiqnc5Pu8yiodbrJCaFyE3CGeubAvky`

### Process accounts from a file

To forward account information for multiple accounts listed in a file, execute the following command:

`cargo run -- --redis-url <REDIS_URL> --rpc-url <RPC_URL> scenario --scenario-file <FILENAME>`

Replace <REDIS_URL>, <RPC_URL>, <ACCOUNT>, and <FILENAME> with the appropriate values for your use case.

### Re-index a regular NFT collection

```
cargo run -- --redis-url $REDIS_URL --rpc-url $RPC_URL collection-v2 \
    --collection BoYFrUK84ZtkAvoJMw2S2q1mKqKM7sHyvZ3yeemdWpP5 \
    --db-url $DB_URL
```

### Run for a fungible token

Pushes a token account that holds JitoSol. Owned by `5aZZ4duJUKiMsJN9vRsoAn4SDX7agvKu7Q3QdFWRfWze`. Useful for sanity checking token ownership indexing.

```
cargo run -- --redis-url redis://localhost:6379 --rpc-url $RPC_URL single --account H7iLu4DPFpzEx1AGN8BCN7Qg966YFndt781p6ukhgki9
cargo run -- --redis-url redis://localhost:6379 --rpc-url $RPC_URL single --account J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn
```

Matrica issue reproduction (FungibleAsset w/ Metadata)

```
cargo run -- --redis-url redis://localhost:6379 --rpc-url $RPC_URL single --account ByJk5D4y7urDRx4Qw95Sv9bLLdgoVcBsdyTFsgSdK29V
cargo run -- --redis-url redis://localhost:6379 --rpc-url $RPC_URL single --account AjMN1WMFusdZnJmHnY61mY6tzg3Eow4miVhJ1pgjcftj
cargo run -- --redis-url redis://localhost:6379 --rpc-url $RPC_URL single --account AmCBTCcz5YE3J6x6bcfgNxQvJgoWKFfKriRd1aTqWtpR
```
