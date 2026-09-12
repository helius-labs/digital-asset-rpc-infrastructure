use std::sync::Arc;

use async_stream::stream;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use grpc::get_grpc_stream_with_rpc_fallback;
use log::info;
use poller::get_block_poller_stream;
use solana_client::nonblocking::rpc_client::RpcClient;
use utils::DASBlock;

pub mod grpc;
mod grpc_convert;
pub mod poller;
pub mod utils;

pub struct GrpcConfig {
    pub url: String,
    pub auth_header: String,
}

pub fn load_block_stream(
    rpc_client: Arc<RpcClient>,
    grpc_config: Option<GrpcConfig>,
    last_indexed_slot: u64,
) -> impl Stream<Item = DASBlock> {
    stream! {
        match grpc_config {
            Some(grpc_config) => {
                info!("Using gPC with poller fallback");
                let stream = get_grpc_stream_with_rpc_fallback(
                    grpc_config.url,
                    grpc_config.auth_header,
                    rpc_client,
                    last_indexed_slot,
                );
                pin_mut!(stream);
                loop {
                    match stream.next().await {
                        Some(blocks) => yield blocks,
                        None => break,
                    }
                }
            }
            None => {
                info!("Using poller");
                let stream = get_block_poller_stream(rpc_client, last_indexed_slot);
                pin_mut!(stream);
                loop {
                    match stream.next().await {
                        Some(blocks) => yield blocks,
                        None => break,
                    }
                }
            }
        }
    }
}
