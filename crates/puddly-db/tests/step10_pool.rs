//! Step 10: pool checkout API exists (no live Postgres required).

use puddly_db::{PgPool, PoolConfig, PoolStatus};

#[test]
fn pool_config_defaults_and_builder() {
    let cfg = PoolConfig::from_url("postgres://localhost/puddly").with_max_connections(4);
    assert_eq!(cfg.max_connections, 4);
    assert!(cfg.connection_string.contains("puddly"));
}

#[test]
fn pool_status_type_is_public() {
    // Compile-time / API surface proof for health / diagnostics.
    let _ = PoolStatus {
        size: 0,
        available: 0,
        max_size: 10,
    };
}

#[tokio::test]
async fn pg_pool_rejects_empty_url() {
    let err = match PgPool::connect(&PoolConfig::from_url("")).await {
        Err(e) => e,
        Ok(_) => panic!("empty url should fail"),
    };
    assert!(
        err.to_string().contains("empty") || err.to_string().contains("connection"),
        "{err}"
    );
}

#[tokio::test]
async fn pg_pool_from_url_api_exists() {
    // Without a running Postgres this fails at connect/checkout — but the
    // public `from_url` / `get` API must compile and be callable.
    let result = PgPool::from_url("postgres://127.0.0.1:1/does_not_exist_step10").await;
    match result {
        Ok(pool) => {
            let status = pool.status();
            assert!(status.max_size >= 1);
            // Checkout may still fail if the first connect was soft.
            let _ = pool.get().await;
        }
        Err(e) => {
            // Expected when nothing listens on :1
            assert!(
                e.to_string().contains("connection")
                    || e.to_string().contains("connect")
                    || e.to_string().contains("pool")
                    || e.to_string().contains("error"),
                "{e}"
            );
        }
    }
}
