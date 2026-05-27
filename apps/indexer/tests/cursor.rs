//! `#[sqlx::test]` tests for the cursor manager (indexer_state table).

mod common;

use dugong_indexer::cursor::CursorManager;
use sqlx::PgPool;

#[sqlx::test(migrations = "../core/migrations")]
async fn cursor_round_trips(pool: PgPool) {
    let mgr = CursorManager::new(pool);

    // No cursor stored yet -> start from genesis.
    assert_eq!(mgr.load_cursor("dugong").await.unwrap(), None);

    // Save and read back.
    mgr.save_cursor("dugong", Some(&"DIGEST1:0".to_string()))
        .await
        .unwrap();
    assert_eq!(
        mgr.load_cursor("dugong").await.unwrap(),
        Some("DIGEST1:0".to_string())
    );

    // Overwrite with a newer cursor.
    mgr.save_cursor("dugong", Some(&"DIGEST2:3".to_string()))
        .await
        .unwrap();
    assert_eq!(
        mgr.load_cursor("dugong").await.unwrap(),
        Some("DIGEST2:3".to_string())
    );

    // Reset clears it back to genesis.
    mgr.reset_cursor("dugong").await.unwrap();
    assert_eq!(mgr.load_cursor("dugong").await.unwrap(), None);
}
