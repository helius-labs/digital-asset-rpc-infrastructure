pub mod args;
pub mod subcommands;
pub mod utils;

use crate::args::Args;
use cadence::{StatsdClient, UdpMetricSink};
use cadence_macros::{is_global_default_set, set_global_default};
use clap::{value_parser, Arg, ArgAction, Command};
use log::info;
use nft_ingester::{
    config::{init_logger, setup_ingester_config, IngesterConfig},
    database::setup_database,
};
use sea_orm::SqlxPostgresConnector;
use std::{net::UdpSocket, path::PathBuf};

pub fn safe_metric<F: Fn()>(f: F) {
    if is_global_default_set() {
        f()
    }
}

#[macro_export]
macro_rules! metric {
    {$($block:stmt;)*} => {
        if is_global_default_set() {
            $(
                $block
            )*
        }
    };
}

pub fn setup_metrics(config: &IngesterConfig) {
    let uri = config.metrics_host.clone();
    let port = config.metrics_port;
    let env = config.env.clone().unwrap_or("dev".to_string());
    if let (Some(uri), Some(port)) = (uri, port) {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind to UDP socket");
        let host = (uri, port);
        let udp_sink = UdpMetricSink::from(host, socket).unwrap();
        let builder = StatsdClient::builder("bgtask_creator", udp_sink);
        let client = builder.with_tag("env", env).build();
        set_global_default(client);
    }
}

/**
 * The bgtask creator is intended to be use as a tool to handle assets that have not been indexed.
 * It will delete all the current bgtasks and create new ones for assets where the metadata is missing.
 *
 * Currently it will try every missing asset every run.
 */

#[tokio::main(flavor = "multi_thread")]
pub async fn main() {
    init_logger();
    info!("Starting bgtask creator");

    let matches = Command::new("bgtaskcreator")
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .help("Sets a custom config file")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("batch_size")
                .long("batch-size")
                .short('b')
                .help("Sets the batch size for the assets to be processed.")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(value_parser!(u64))
                .default_value("1000"),
        )
        .arg(
            Arg::new("ignore-url")
                .long("ignore-url")
                .short('i')
                .help("ignore matching url when creating tasks")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("include-url")
                .long("include-url")
                .short('u')
                .help("include only the matching url when creating tasks")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("limit")
                .long("limit")
                .short('l')
                .help("maximum number of tasks to create")
                .required(false)
                .action(ArgAction::Set)
                .value_parser(value_parser!(u64))
                .default_value("0"),
        )
        .arg(
            Arg::new("authority")
                .long("authority")
                .short('a')
                .help("Create background tasks for the given authority")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("collection")
                .long("collection")
                .short('o')
                .help("Create background tasks for the given collection")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("mint")
                .long("mint")
                .short('m')
                .help("Create background tasks for the given mint")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("creator")
                .long("creator")
                .short('r')
                .help("Create background tasks for the given creator")
                .required(false)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("force-reindex")
                .long("force-reindex")
                .help("Re-index even if off-chain is already indexed")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("last-day")
                .long("last-day")
                .help("Re-index only the last 24 hours of off-chain data")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("missing-only")
                .long("missing-only")
                .help("Only create tasks for assets whose off-chain metadata was never fetched (e.g. downloads that failed during an upstream outage). Pairs with --collection for targeted repair runs.")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("show-total-matched")
                .long("show-total-matched")
                .help("Show the total number of records that will be indexed/modified. This will be slow down the job.")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("metrics-enabled")
                .long("metrics-enabled")
                .help("Enable metrics for the tool.")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("show").about("Show tasks").arg(
                Arg::new("print")
                    .long("print")
                    .short('p')
                    .help("Print the tasks to stdout")
                    .required(false)
                    .action(clap::ArgAction::SetTrue),
            ),
        )
        .subcommand(Command::new("create").about("Create new background tasks"))
        .subcommand(Command::new("delete").about("Delete ALL pending background tasks"))
        .subcommand(Command::new("find").about("Find and describe a task by asset id"))
        .get_matches();

    let config = setup_ingester_config();

    setup_metrics(&config);

    // One pool many clones, this thing is thread safe and send sync
    let database_pool = setup_database(&config).await;

    // Get a postgres connection from the pool
    let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(database_pool.clone());

    let args = Args {
        batch_size: matches.get_one::<u64>("batch_size").unwrap().to_owned(),
        limit: matches.get_one::<u64>("limit").unwrap().to_owned(),
        authority: matches.get_one::<String>("authority").cloned(),
        collection: matches.get_one::<String>("collection").cloned(),
        mint: matches.get_one::<String>("mint").cloned(),
        creator: matches.get_one::<String>("creator").cloned(),
        ignore_url: matches.get_one::<String>("ignore-url").cloned(),
        include_url: matches.get_one::<String>("include-url").cloned(),
        force_reindex: matches.get_one::<bool>("force-reindex").cloned(),
        last_day: matches.get_one::<bool>("last-day").cloned(),
        missing_only: matches.get_one::<bool>("missing-only").cloned(),
        show_total_matched: matches.get_one::<bool>("show-total-matched").cloned(),
        metrics_enabled: matches.get_one::<bool>("metrics-enabled").cloned(),
    };

    match matches.subcommand_name() {
        Some("create") => {
            subcommands::create::create(conn, database_pool, args).await;
        }
        Some("delete") => {
            subcommands::delete::delete(conn, args).await;
        }
        Some("show") => {
            subcommands::show::show(conn, args).await;
        }
        Some("find") => {
            subcommands::find::find(conn, args).await;
        }
        _ => {
            info!("Please provide an action")
        }
    }
}
