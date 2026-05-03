use light_discord_auth::{hash_token, new_invite_code};
use light_discord_core::ChatMessage;
use light_discord_storage::{CreateAccountResult, DeleteMessageResult, RetentionPolicy, Storage};
use uuid::Uuid;

#[tokio::test]
async fn postgres_storage_auth_message_delete_and_audit_flow() -> anyhow::Result<()> {
    let Ok(database_url) = std::env::var("LD_TEST_DATABASE_URL") else {
        eprintln!("skipping postgres integration test; LD_TEST_DATABASE_URL is not set");
        return Ok(());
    };

    let storage = Storage::connect_postgres(&database_url, RetentionPolicy::default()).await?;
    let invite_code = new_invite_code();
    let invite_hash = hash_token(&invite_code);
    storage
        .create_invite(&invite_hash, None, "integration test")
        .await?;

    let display_name = format!("test-{}", Uuid::new_v4().simple());
    let account = match storage
        .create_user_with_invite(&display_name, "password-hash", &invite_hash)
        .await?
    {
        CreateAccountResult::Created(account) => account,
        other => panic!("unexpected account creation result: {other:?}"),
    };

    let session_hash = format!("session-hash-{}", Uuid::new_v4().simple());
    storage
        .create_session(&session_hash, &account.user_id)
        .await?;
    let session = storage.validate_session(&session_hash).await?.unwrap();
    assert_eq!(session.user_id, account.user_id);

    let message = ChatMessage::new(
        format!("msg-{}", Uuid::new_v4().simple()),
        "general",
        &account.user_id,
        &account.display_name,
        "postgres audit test",
    );
    storage.save_message(&message).await?;
    assert!(!storage.recent_messages("general", 50).await?.is_empty());

    let deleted = storage
        .soft_delete_message(&message.id, &account.user_id, false)
        .await?;
    assert!(matches!(deleted, DeleteMessageResult::Deleted(_)));

    let audit_log = storage.audit_log(50).await?;
    assert!(audit_log
        .iter()
        .any(
            |entry| entry.target_message_id.as_deref() == Some(message.id.as_str())
                && entry.message_body_snapshot.as_deref() == Some("postgres audit test")
        ));

    Ok(())
}
