use sea_orm::{ConnectionTrait, Statement};
pub use sea_orm_migration::prelude::*;

mod m20220101_000001_init;
mod m20221020_052135_add_asset_hashes;
mod m20221022_140350_add_creator_asset_unique_index;
mod m20221025_182127_remove_creator_error_unique_index;
mod m20221026_155220_add_bg_tasks;
mod m20221104_094327_add_backfiller_failed;
mod m20221114_173041_add_collection_info;
mod m20221115_165700_add_backfiller_locked;
mod m20221116_110500_add_backfiller_failed_and_locked_indeces;
mod m20230105_160722_drop_collection_info;
mod m20230106_051135_unique_groupings;
mod m20230131_140613_change_token_account_indexes;
mod m20230203_205959_improve_upsert_perf;
mod m20230224_093722_performance_improvements;
mod m20230310_162227_add_indexes_to_bg;
mod m20230317_121944_remove_indexes_for_perf;
mod m20230510_183736_add_indices_to_assets;
mod m20230516_185005_add_reindex_to_assets;
mod m20230525_115717_cl_audit_table;
mod m20230526_120101_add_owner_delegate_sequence_number;
mod m20230528_124011_cl_audit_table_index;
mod m20230601_120101_add_pnft_enum_val;
mod m20230613_114817_add_name_symbol_to_asset_data;
mod m20230615_120101_remove_asset_null_constraints;
mod m20230620_120101_add_was_decompressed;
mod m20230623_120101_add_leaf_sequence_number;
mod m20230712_120101_remove_asset_creators_null_constraints;
mod m20230720_120101_add_asset_grouping_verified;
mod m20230720_130101_remove_asset_grouping_null_constraints;
mod m20230724_120101_add_group_info_seq;
mod m20230726_013107_remove_not_null_constraint_from_group_value;
mod m20230810_141739_remove_grouping_verified_not_null_constraint;
mod m20230821_125505_add_creators_auth_collec_to_asset;
mod m20230908_124833_add_creators_array_index;
mod m20230908_160822_add_cl_audits_v2;
mod m20230914_051815_create_asset_optimized_index;
mod m20230915_000001_remove_cl_audits;
mod m20230920_162100_add_asset_collections_indicies;
mod m20230921_184517_add_asset_authorities_indicies;
mod m20231005_153141_add_extensions_column;
mod m20231010_142712_drop_indices_from_asset;
mod m20231011_122400_add_created_at_asset_data;
mod m20231013_095436_add_owners_table;
mod m20231017_103945_remove_authorites_grouping_table;
mod m20231018_140143_change_constraints_for_owners;
mod m20231018_173542_change_slot_updated_type_owners;
mod m20231020_115223_add_freeze_to_owners;
mod m20231020_162815_add_amount_to_owners;
mod m20231102_103202_add_price_table;
mod m20231106_174851_remove_token_accounts_table;
mod m20231109_082940_add_asset_data_v2;
mod m20231125_154209_add_owner_mint_owners_index;
mod m20231126_110855_add_closed_to_owners;
mod m20231214_164547_remove_asset_data_table;
mod m20231219_144547_add_metadata_id_column;
mod m20240104_120101_add_owners_primary_key;
mod m20240108_110804_add_asset_covering;
mod m20240108_120101_add_seq_numbers_bgum_update_metadata;
mod m20240108_120102_remove_was_decompressed;
mod m20240112_211053_add_update_metadata_ix;
mod m20240117_120101_alter_creator_indices;
mod m20240118_233532_add_token_amount_u64_to_owners;
mod m20240123_170736_add_tasks_table_index;
mod m20240124_151900_add_slot_updated_column_per_update_type;
mod m20240131_134754_add_index_to_offchain_metadata;
mod m20240315_095415_add_edition_column;
mod m20240315_095437_add_editions_table;
mod m20240320_174306_drop_attachments_table;
mod m20240326_171414_add_mpl_core_plugins_columns;
mod m20240326_171444_add_mpl_core_enum_vals;
mod m20240326_171506_add_mpl_core_info_items;
mod m20240522_171506_add_creator_and_token_account_indexes;
mod m20240524_120101_add_mpl_core_external_plugins_columns;
mod m20240921_000000_add_owner_asset_id_index;
mod m20240923_000000_add_owner_asset_id_index_v2;
mod m20241023_000000_add_block_metadata;
mod m20250108_131318_add_dedicated_authorities_columns;
mod m20250412_201803_add_bgum_leaf_schema_v2_items;
mod m20250412_201909_add_bubblegum_v2_ixs_to_enum;
mod m20250702_120101_add_bubblegum_v2_enum_vals;
mod m20251024_120101_add_t22_metadata_address;
mod m20260113_000000_add_metadata_url_index;
mod m20260424_000000_add_mpl_core_group_enum_val;
mod m20260424_000001_add_agent_columns;
mod m20260608_000000_add_collections_info_groups_index;
pub mod model;

pub struct Migrator;

pub async fn execute_sql<'a>(manager: &SchemaManager<'_>, sql: &str) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            sql.to_string(),
        ))
        .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_init::Migration),
            Box::new(m20221020_052135_add_asset_hashes::Migration),
            Box::new(m20221022_140350_add_creator_asset_unique_index::Migration),
            Box::new(m20221025_182127_remove_creator_error_unique_index::Migration),
            Box::new(m20221026_155220_add_bg_tasks::Migration),
            Box::new(m20221104_094327_add_backfiller_failed::Migration),
            Box::new(m20221114_173041_add_collection_info::Migration),
            Box::new(m20221115_165700_add_backfiller_locked::Migration),
            Box::new(m20221116_110500_add_backfiller_failed_and_locked_indeces::Migration),
            Box::new(m20230105_160722_drop_collection_info::Migration),
            Box::new(m20230106_051135_unique_groupings::Migration),
            Box::new(m20230131_140613_change_token_account_indexes::Migration),
            Box::new(m20230203_205959_improve_upsert_perf::Migration),
            Box::new(m20230224_093722_performance_improvements::Migration),
            Box::new(m20230310_162227_add_indexes_to_bg::Migration),
            Box::new(m20230317_121944_remove_indexes_for_perf::Migration),
            Box::new(m20230510_183736_add_indices_to_assets::Migration),
            Box::new(m20230516_185005_add_reindex_to_assets::Migration),
            Box::new(m20230525_115717_cl_audit_table::Migration),
            Box::new(m20230526_120101_add_owner_delegate_sequence_number::Migration),
            Box::new(m20230528_124011_cl_audit_table_index::Migration),
            Box::new(m20230601_120101_add_pnft_enum_val::Migration),
            Box::new(m20230613_114817_add_name_symbol_to_asset_data::Migration),
            Box::new(m20230615_120101_remove_asset_null_constraints::Migration),
            Box::new(m20230620_120101_add_was_decompressed::Migration),
            Box::new(m20230623_120101_add_leaf_sequence_number::Migration),
            Box::new(m20230712_120101_remove_asset_creators_null_constraints::Migration),
            Box::new(m20230720_120101_add_asset_grouping_verified::Migration),
            Box::new(m20230720_130101_remove_asset_grouping_null_constraints::Migration),
            Box::new(m20230724_120101_add_group_info_seq::Migration),
            Box::new(m20230726_013107_remove_not_null_constraint_from_group_value::Migration),
            Box::new(m20230810_141739_remove_grouping_verified_not_null_constraint::Migration),
            Box::new(m20230821_125505_add_creators_auth_collec_to_asset::Migration),
            Box::new(m20230908_124833_add_creators_array_index::Migration),
            Box::new(m20230908_160822_add_cl_audits_v2::Migration),
            Box::new(m20230914_051815_create_asset_optimized_index::Migration),
            Box::new(m20230915_000001_remove_cl_audits::Migration),
            Box::new(m20230920_162100_add_asset_collections_indicies::Migration),
            Box::new(m20230921_184517_add_asset_authorities_indicies::Migration),
            Box::new(m20231005_153141_add_extensions_column::Migration),
            Box::new(m20231010_142712_drop_indices_from_asset::Migration),
            Box::new(m20231011_122400_add_created_at_asset_data::Migration),
            Box::new(m20231013_095436_add_owners_table::Migration),
            Box::new(m20231017_103945_remove_authorites_grouping_table::Migration),
            Box::new(m20231018_140143_change_constraints_for_owners::Migration),
            Box::new(m20231018_173542_change_slot_updated_type_owners::Migration),
            Box::new(m20231020_115223_add_freeze_to_owners::Migration),
            Box::new(m20231020_162815_add_amount_to_owners::Migration),
            Box::new(m20231102_103202_add_price_table::Migration),
            Box::new(m20231106_174851_remove_token_accounts_table::Migration),
            Box::new(m20231109_082940_add_asset_data_v2::Migration),
            Box::new(m20231125_154209_add_owner_mint_owners_index::Migration),
            Box::new(m20231126_110855_add_closed_to_owners::Migration),
            Box::new(m20231214_164547_remove_asset_data_table::Migration),
            Box::new(m20231219_144547_add_metadata_id_column::Migration),
            Box::new(m20240104_120101_add_owners_primary_key::Migration),
            Box::new(m20240108_110804_add_asset_covering::Migration),
            Box::new(m20240108_120101_add_seq_numbers_bgum_update_metadata::Migration),
            Box::new(m20240108_120102_remove_was_decompressed::Migration),
            Box::new(m20240112_211053_add_update_metadata_ix::Migration),
            Box::new(m20240117_120101_alter_creator_indices::Migration),
            Box::new(m20240118_233532_add_token_amount_u64_to_owners::Migration),
            Box::new(m20240123_170736_add_tasks_table_index::Migration),
            Box::new(m20240124_151900_add_slot_updated_column_per_update_type::Migration),
            Box::new(m20240131_134754_add_index_to_offchain_metadata::Migration),
            Box::new(m20240315_095415_add_edition_column::Migration),
            Box::new(m20240315_095437_add_editions_table::Migration),
            Box::new(m20240320_174306_drop_attachments_table::Migration),
            Box::new(m20240326_171414_add_mpl_core_plugins_columns::Migration),
            Box::new(m20240326_171444_add_mpl_core_enum_vals::Migration),
            Box::new(m20240326_171506_add_mpl_core_info_items::Migration),
            Box::new(m20240522_171506_add_creator_and_token_account_indexes::Migration),
            Box::new(m20240524_120101_add_mpl_core_external_plugins_columns::Migration),
            Box::new(m20240921_000000_add_owner_asset_id_index::Migration),
            Box::new(m20240923_000000_add_owner_asset_id_index_v2::Migration),
            Box::new(m20241023_000000_add_block_metadata::Migration),
            Box::new(m20250108_131318_add_dedicated_authorities_columns::Migration),
            Box::new(m20250412_201803_add_bgum_leaf_schema_v2_items::Migration),
            Box::new(m20250412_201909_add_bubblegum_v2_ixs_to_enum::Migration),
            Box::new(m20250702_120101_add_bubblegum_v2_enum_vals::Migration),
            Box::new(m20251024_120101_add_t22_metadata_address::Migration),
            Box::new(m20260113_000000_add_metadata_url_index::Migration),
            Box::new(m20260424_000000_add_mpl_core_group_enum_val::Migration),
            Box::new(m20260424_000001_add_agent_columns::Migration),
            Box::new(m20260608_000000_add_collections_info_groups_index::Migration),
        ]
    }
}
