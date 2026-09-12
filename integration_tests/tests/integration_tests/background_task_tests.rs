use function_name::named;
use nft_ingester::tasks::DownloadMetadataTask;
use std::time::Duration;

use serial_test::serial;

use super::common::*;

#[tokio::test]
#[serial]
#[named]
async fn test_metadata_downloader() {
    let name = trim_test_name(function_name!());
    let setup = TestSetup::new(name.clone()).await;
    apply_migrations_and_delete_data(setup.db.clone()).await;

    let uri = "https://wagmi-collectibles.s3.amazonaws.com/drops/trait_packs/crack-pack-005/blue-meta.json";
    let metadata = DownloadMetadataTask::request_metadata(
        uri.to_string(),
        Duration::from_secs(10),
        None,
        None,
        None,
    )
    .await;
    insta::assert_json_snapshot!(name.clone(), metadata.unwrap());
}
