use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use crate::models::WreckRecord;

pub struct DbClient {
    pool: Pool<Postgres>,
}

impl DbClient {
    /// Initialize a connection pool to the PostgreSQL database.
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        Ok(Self { pool })
    }

    /// Retrieve the underlying connection pool
    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    /// Run health check
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Fetch all wrecks from the primary PostGIS `wrecks` table.
    pub async fn fetch_all_wrecks(&self) -> Result<Vec<WreckRecord>> {
        let records = sqlx::query_as::<_, WreckRecord>(
            r#"
            SELECT 
                id, source_id, name, status, depth_m, region, 
                ST_AsBinary(location) as location_wkb, 
                metadata, created_at
            FROM wrecks
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(records)
    }
}
