use crate::broker::storage::BrokerStorage;
use crate::broker::types::{BookingJob, JobState};
use crate::p2p::protocol::{BookingData, Msg, NotifyData};
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

pub struct BrokerHandler {
    storage: Arc<BrokerStorage>,
}

impl BrokerHandler {
    pub fn new(storage: Arc<BrokerStorage>) -> Self {
        BrokerHandler { storage }
    }

    /// Handle booking submission with idempotency
    /// Returns BookingAck message
    pub async fn handle_submit_booking(
        &self,
        correlation_id: String,
        booking: BookingData,
        notify: NotifyData,
    ) -> Result<Msg> {
        info!(
            correlation_id = %correlation_id,
            "Received booking submission request"
        );

        // Check if correlation_id already exists (idempotency)
        match self.storage.get_booking_job(&correlation_id)? {
            Some(existing_job) => {
                // Job already exists - return appropriate status
                let status = match existing_job.state {
                    JobState::Confirmed => "confirmed",
                    JobState::Failed => "failed",
                    _ => "queued",
                };

                info!(
                    correlation_id = %correlation_id,
                    status = status,
                    "Booking already exists, returning existing status"
                );

                return Ok(Msg::BookingAck {
                    correlation_id,
                    status: status.to_string(),
                });
            }
            None => {
                // New job - create and persist
            }
        }

        // Serialize booking and notify data
        let booking_json = serde_json::to_string(&booking)
            .context("Failed to serialize booking data")?;
        let notify_json = serde_json::to_string(&notify)
            .context("Failed to serialize notify data")?;

        // Create new booking job
        let now = chrono::Utc::now().timestamp_millis();
        let job = BookingJob {
            correlation_id: correlation_id.clone(),
            booking_json,
            notify_json,
            state: JobState::Queued,
            attempts: 0,
            next_attempt_at: now, // Start immediately
            last_error: None,
            http_status: None,
            central_response_json: None,
            created_at: now,
            updated_at: now,
        };

        // Persist atomically - ACK only after successful persist
        self.storage
            .persist_booking_job(&job)
            .context("Failed to persist booking job")?;

        info!(
            correlation_id = %correlation_id,
            "Booking job persisted successfully, sending ACK"
        );

        Ok(Msg::BookingAck {
            correlation_id,
            status: "queued".to_string(),
        })
    }
}
