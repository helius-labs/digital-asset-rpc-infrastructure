use sea_orm_migration::prelude::*;

fn set_env_if_not_exists(var: &str, value: &str) {
    if std::env::var(var).is_err() {
        std::env::set_var(var, value);
    }
}

#[async_std::main]
async fn main() {
    if let Ok(env) = std::env::var("ENV") {
        if env == "local" {
            set_env_if_not_exists("INIT_FILE_PATH", "../init.sql");
            set_env_if_not_exists("DATABASE_URL", "postgres://ingest@localhost/das");
        }
    }
    cli::run_cli(migration::Migrator).await;
}
