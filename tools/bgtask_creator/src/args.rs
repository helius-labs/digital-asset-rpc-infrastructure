#[derive(Clone)]
pub struct Args {
    pub batch_size: u64,
    pub limit: u64,
    pub authority: Option<String>,
    pub collection: Option<String>,
    pub creator: Option<String>,
    pub mint: Option<String>,
    pub ignore_url: Option<String>,
    pub include_url: Option<String>,
    pub force_reindex: Option<bool>,
    pub last_day: Option<bool>,
    pub missing_only: Option<bool>,
    pub show_total_matched: Option<bool>,
    pub metrics_enabled: Option<bool>,
}
