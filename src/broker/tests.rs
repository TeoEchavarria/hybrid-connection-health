#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::broker::types::*;
    use crate::config::Config;
    use crate::config::Role;
    use crate::p2p::protocol;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;

    // Helper to create test storage
    fn create_test_storage() -> (TempDir, Arc<storage::BrokerStorage>) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = Arc::new(storage::BrokerStorage::new(db_path.to_str().unwrap()).unwrap());
        (temp_dir, storage)
    }

    // Helper to create test booking data
    fn create_test_booking() -> (protocol::BookingData, protocol::NotifyData) {
        let booking = protocol::BookingData {
            date: "2026-01-15".to_string(),
            start_time: "10:00".to_string(),
            end_time: "11:00".to_string(),
            name: "Test User".to_string(),
        };
        let notify = protocol::NotifyData {
            email: "test@example.com".to_string(),
            locale: Some("en".to_string()),
            timezone: Some("UTC".to_string()),
        };
        (booking, notify)
    }

    #[tokio::test]
    async fn test_idempotency() {
        let (_temp_dir, storage) = create_test_storage();
        let handler = handler::BrokerHandler::new(storage.clone());

        let correlation_id = Uuid::new_v4().to_string();
        let (booking, notify) = create_test_booking();

        // First submission
        let ack1 = handler
            .handle_submit_booking(
                correlation_id.clone(),
                booking.clone(),
                notify.clone(),
            )
            .await
            .unwrap();

        assert!(matches!(ack1, protocol::Msg::BookingAck { status, .. } if status == "queued"));

        // Second submission with same correlation_id (idempotency)
        let ack2 = handler
            .handle_submit_booking(
                correlation_id.clone(),
                booking.clone(),
                notify.clone(),
            )
            .await
            .unwrap();

        // Should return queued status (already exists)
        assert!(matches!(ack2, protocol::Msg::BookingAck { status, .. } if status == "queued"));

        // Verify only one job was created
        let job = storage.get_booking_job(&correlation_id).unwrap().unwrap();
        assert_eq!(job.correlation_id, correlation_id);
    }

    #[tokio::test]
    async fn test_ack_after_persist() {
        let (_temp_dir, storage) = create_test_storage();
        let handler = handler::BrokerHandler::new(storage.clone());

        let correlation_id = Uuid::new_v4().to_string();
        let (booking, notify) = create_test_booking();

        // Submit booking
        let ack = handler
            .handle_submit_booking(correlation_id.clone(), booking, notify)
            .await
            .unwrap();

        // ACK should be returned
        assert!(matches!(ack, protocol::Msg::BookingAck { status, .. } if status == "queued"));

        // Verify job was persisted
        let job = storage.get_booking_job(&correlation_id).unwrap();
        assert!(job.is_some());
        let job = job.unwrap();
        assert_eq!(job.correlation_id, correlation_id);
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.attempts, 0);
    }

    #[tokio::test]
    async fn test_offline_retry_keeps_job_queued() {
        let (_temp_dir, storage) = create_test_storage();
        
        // Create a job manually
        let correlation_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let job = BookingJob {
            correlation_id: correlation_id.clone(),
            booking_json: r#"{"date":"2026-01-15","start_time":"10:00","end_time":"11:00","name":"Test"}"#.to_string(),
            notify_json: r#"{"email":"test@example.com"}"#.to_string(),
            state: JobState::Queued,
            attempts: 0,
            next_attempt_at: now,
            last_error: None,
            http_status: None,
            central_response_json: None,
            created_at: now,
            updated_at: now,
        };

        storage.persist_booking_job(&job).unwrap();

        // Verify job is queued
        let retrieved = storage.get_booking_job(&correlation_id).unwrap().unwrap();
        assert_eq!(retrieved.state, JobState::Queued);
    }

    #[tokio::test]
    async fn test_notification_only_after_confirmation() {
        let (_temp_dir, storage) = create_test_storage();

        let correlation_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        // Create a confirmed job
        let job = BookingJob {
            correlation_id: correlation_id.clone(),
            booking_json: r#"{"date":"2026-01-15","start_time":"10:00","end_time":"11:00","name":"Test"}"#.to_string(),
            notify_json: r#"{"email":"test@example.com"}"#.to_string(),
            state: JobState::Confirmed,
            attempts: 0,
            next_attempt_at: now,
            last_error: None,
            http_status: Some(200),
            central_response_json: Some(r#"{"id":"123"}"#.to_string()),
            created_at: now,
            updated_at: now,
        };
        storage.persist_booking_job(&job).unwrap();

        // Create notification
        let notif = NotificationRecord {
            correlation_id: correlation_id.clone(),
            email_to: "test@example.com".to_string(),
            state: NotificationState::Pending,
            attempts: 0,
            next_attempt_at: now,
            last_error: None,
            subject: String::new(),
            body: String::new(),
            simulated_sent_at: None,
            created_at: now,
            updated_at: now,
        };
        storage.persist_notification(&notif).unwrap();

        // Verify notification exists and is pending
        let retrieved = storage.get_notification(&correlation_id).unwrap().unwrap();
        assert_eq!(retrieved.state, NotificationState::Pending);
    }

    #[test]
    fn test_exponential_backoff_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = Arc::new(storage::BrokerStorage::new(db_path.to_str().unwrap()).unwrap());

        let config = Config {
            role: Role::Gateway,
            listen: "/ip4/0.0.0.0/tcp/0".to_string(),
            dial: None,
            peers: vec![],
            identity_keypair: libp2p::identity::Keypair::generate_ed25519(),
            bootstrap_peers: vec![],
            enable_mdns: true,
            enable_kad: true,
            enable_relay: false,
            discovery_timeout_secs: 60,
            central_api_url: Some("https://example.com".to_string()),
            db_path: "./data/broker.db".to_string(),
            max_retry_attempts: 10,
            initial_backoff_ms: 1000,
        };

        let forwarder = forwarder::ForwarderWorker::new(storage, config).unwrap();

        // Test backoff calculation
        let backoff1 = forwarder.calculate_backoff(1);
        assert!(backoff1 >= 1000 && backoff1 <= 1000 + 1000); // base + jitter

        let backoff2 = forwarder.calculate_backoff(2);
        assert!(backoff2 >= 2000 && backoff2 <= 2000 + 1000); // 2^2 * 1000 + jitter

        let backoff3 = forwarder.calculate_backoff(3);
        assert!(backoff3 >= 4000 && backoff3 <= 4000 + 1000); // 2^3 * 1000 + jitter
    }
}
