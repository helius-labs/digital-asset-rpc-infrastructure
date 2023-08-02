use crate::error::IngesterError;
use digital_asset_types::dao::{asset, asset_creators, cl_audits, cl_items};
use log::{debug, error, warn};
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ColumnTrait, DbBackend, DbErr, EntityTrait,
};
use spl_account_compression::events::ChangeLogEventV1;

use std::convert::From;

pub async fn save_changelog_event<'c, T>(
    change_log_event: &ChangeLogEventV1,
    slot: u64,
    txn_id: &str,
    txn: &T,
    instruction: &str,
) -> Result<u64, IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    insert_change_log(change_log_event, slot, txn_id, txn, instruction).await?;
    Ok(change_log_event.seq)
}

fn node_idx_to_leaf_idx(index: i64, tree_height: u32) -> i64 {
    index - 2i64.pow(tree_height)
}

pub async fn insert_change_log<'c, T>(
    change_log_event: &ChangeLogEventV1,
    slot: u64,
    txn_id: &str,
    txn: &T,
    instruction: &str,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let mut i: i64 = 0;
    let depth = change_log_event.path.len() - 1;
    let tree_id = change_log_event.id.as_ref();
    for p in change_log_event.path.iter() {
        let node_idx = p.index as i64;
        debug!(
            "seq {}, index {} level {}, node {:?}, txn: {:?}, instruction: {:?}",
            change_log_event.seq,
            p.index,
            i,
            bs58::encode(p.node).into_string(),
            txn_id,
            instruction
        );
        let leaf_idx = if i == 0 {
            Some(node_idx_to_leaf_idx(node_idx, depth as u32))
        } else {
            None
        };

        let item = cl_items::ActiveModel {
            tree: Set(tree_id.to_vec()),
            level: Set(i),
            node_idx: Set(node_idx),
            hash: Set(p.node.as_ref().to_vec()),
            seq: Set(change_log_event.seq as i64),
            leaf_idx: Set(leaf_idx),
            ..Default::default()
        };

        let mut audit_item: cl_audits::ActiveModel = item.clone().into();
        audit_item.tx = Set(txn_id.to_string());
        audit_item.instruction = Set(instruction.to_string());

        i += 1;
        let mut query = cl_items::Entity::insert(item)
            .on_conflict(
                OnConflict::columns([cl_items::Column::Tree, cl_items::Column::NodeIdx])
                    .update_columns([
                        cl_items::Column::Hash,
                        cl_items::Column::Seq,
                        cl_items::Column::LeafIdx,
                        cl_items::Column::Level,
                    ])
                    .to_owned(),
            )
            .build(DbBackend::Postgres);
        query.sql = format!("{} WHERE excluded.seq > cl_items.seq", query.sql);
        txn.execute(query)
            .await
            .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

        // Insert the audit item after the insert into cl_items have been completed
        let query = cl_audits::Entity::insert(audit_item)
            .on_conflict(
                OnConflict::columns([
                    cl_audits::Column::Tree,
                    cl_audits::Column::NodeIdx,
                    cl_audits::Column::Seq,
                    cl_audits::Column::Hash,
                    cl_audits::Column::Tx,
                ])
                .do_nothing()
                .to_owned(),
            )
            .build(DbBackend::Postgres);
        match txn.execute(query).await {
            Ok(_) => {}
            Err(e) => {
                error!("Error while inserting into cl_audits: {:?}", e);
            }
        }
    }

    // Helius does not use the backfiller.
    // TODO: Make this configurable.
    //
    //
    // // If and only if the entire path of nodes was inserted into the `cl_items` table, then insert
    // // a single row into the `backfill_items` table.  This way if an incomplete path was inserted
    // // into `cl_items` due to an error, a gap will be created for the tree and the backfiller will
    // // fix it.
    // if i - 1 == depth as i64 {
    //     // See if the tree already exists in the `backfill_items` table.
    //     let rows = backfill_items::Entity::find()
    //         .filter(backfill_items::Column::Tree.eq(tree_id))
    //         .limit(1)
    //         .all(txn)
    //         .await?;

    //     // If the tree does not exist in `backfill_items` and the sequence number is greater than 1,
    //     // then we know we will need to backfill the tree from sequence number 1 up to the current
    //     // sequence number.  So in this case we set at flag to force checking the tree.
    //     let force_chk = rows.is_empty() && change_log_event.seq > 1;

    //     info!("Adding to backfill_items table at level {}", i - 1);
    //     let item = backfill_items::ActiveModel {
    //         tree: Set(tree_id.to_vec()),
    //         seq: Set(change_log_event.seq as i64),
    //         slot: Set(slot as i64),
    //         force_chk: Set(force_chk),
    //         backfilled: Set(false),
    //         failed: Set(false),
    //         ..Default::default()
    //     };

    //     backfill_items::Entity::insert(item).exec(txn).await?;
    // }

    Ok(())
    //TODO -> set maximum size of path and break into multiple statements
}

pub async fn update_compressed_asset<T>(
    txn: &T,
    id: Vec<u8>,
    seq: Option<u64>,
    model: asset::ActiveModel,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let update_one = if let Some(seq) = seq {
        asset::Entity::update(model).filter(
            Condition::all()
                .add(asset::Column::Id.eq(id.clone()))
                .add(asset::Column::Seq.lte(seq))
                // Do not update once asset is decompressed
                .add(asset::Column::Compressed.eq(true)),
        )
    } else {
        asset::Entity::update(model).filter(asset::Column::Id.eq(id.clone()))
    };

    match update_one.exec(txn).await {
        Ok(_) => Ok(()),
        Err(err) => match err {
            DbErr::RecordNotFound(ref s) => {
                if s.contains("None of the database rows are affected") {
                    warn!(
                        "Skipping update for asset {}. It either does not exist or has already been decompressed.",
                        bs58::encode(id).into_string()
                    );
                    Ok(())
                } else {
                    Err(IngesterError::from(err))
                }
            }
            _ => Err(IngesterError::from(err)),
        },
    }
}

pub async fn update_creator<T>(
    txn: &T,
    asset_id: Vec<u8>,
    creator: Vec<u8>,
    seq: u64,
    model: asset_creators::ActiveModel,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    // Using `update_many` to avoid having to supply the primary key as well within `model`.
    // We still effectively end up updating a single row at most, which is uniquely identified
    // by the `(asset_id, creator)` pair. Is there any reason why we should not use
    // `update_many` here?
    let update = asset_creators::Entity::update_many()
        .filter(
            Condition::all()
                .add(asset_creators::Column::AssetId.eq(asset_id))
                .add(asset_creators::Column::Creator.eq(creator))
                .add(asset_creators::Column::Seq.lte(seq)),
        )
        .set(model);

    update.exec(txn).await.map_err(IngesterError::from)?;

    Ok(())
}
