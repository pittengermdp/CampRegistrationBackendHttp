use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use dotenv::dotenv;
use lambda_lib::{PgPool, PgPooledConnection};
use std::env;
use tracing::{error, info};

pub mod models;
pub mod schema;

pub fn create_db_pool() -> Result<PgPool, Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    info!("Connecting to database at: {}", database_url);

    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool = Pool::builder()
        .max_size(5) // Adjust based on your needs
        .build(manager)
        .map_err(|e| {
            error!("Failed to create database connection pool: {}", e);
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;

    // Verify connection works
    let _conn = pool.get().map_err(|e| {
        error!("Failed to get database connection from pool: {}", e);
        Box::new(e) as Box<dyn std::error::Error + Send + Sync>
    })?;

    info!("Successfully connected to database");
    Ok(pool)
}

pub fn get_conn(
    pool: &PgPool,
) -> Result<PgPooledConnection, Box<dyn std::error::Error + Send + Sync>> {
    pool.get().map_err(|e| {
        error!("Failed to get database connection from pool: {}", e);
        Box::new(e) as Box<dyn std::error::Error + Send + Sync>
    })
}
