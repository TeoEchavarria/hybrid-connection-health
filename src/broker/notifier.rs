use crate::broker::storage::BrokerStorage;
use crate::broker::types::{BookingJob, NotificationRecord, NotificationState};
use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

pub struct NotifierWorker {
    storage: Arc<BrokerStorage>,
}

impl NotifierWorker {
    pub fn new(storage: Arc<BrokerStorage>) -> Self {
        NotifierWorker { storage }
    }

    /// Run the notifier worker loop
    pub async fn run(&self) -> Result<()> {
        info!("Notifier worker started");

        let mut interval = tokio::time::interval(Duration::from_secs(2));

        loop {
            interval.tick().await;

            match self.process_due_notifications().await {
                Ok(_) => {}
                Err(e) => {
                    error!("Error in notifier worker: {:?}", e);
                }
            }
        }
    }

    /// Process due notifications
    async fn process_due_notifications(&self) -> Result<()> {
        let notifications = self.storage.get_due_notifications(10)?;

        for notif in notifications {
            if let Err(e) = self.process_notification(notif).await {
                error!("Failed to process notification: {:?}", e);
            }
        }

        Ok(())
    }

    /// Process a single notification
    async fn process_notification(&self, notif: NotificationRecord) -> Result<()> {
        let correlation_id = notif.correlation_id.clone();

        info!(
            correlation_id = %correlation_id,
            email = %notif.email_to,
            "Processing notification"
        );

        // Fetch corresponding booking job
        let job = self
            .storage
            .get_booking_job(&correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("Booking job not found: {}", correlation_id))?;

        // Skip if job is not Confirmed
        if job.state != crate::broker::types::JobState::Confirmed {
            warn!(
                correlation_id = %correlation_id,
                state = %job.state.as_str(),
                "Skipping notification - booking job not confirmed"
            );
            return Ok(());
        }

        // Build email subject and body
        let (subject, body) = self.build_email(&job)?;

        // Log simulated email
        let body_preview = if body.len() > 100 {
            format!("{}...", &body[..100])
        } else {
            body.clone()
        };

        info!(
            correlation_id = %correlation_id,
            to = %notif.email_to,
            subject = %subject,
            "SIMULATED_EMAIL correlation_id={} to={} subject=\"{}\" body_preview=\"{}\"",
            correlation_id,
            notif.email_to,
            subject,
            body_preview
        );

        // Update notification state to SimulatedSent
        let sent_at = chrono::Utc::now().timestamp_millis();
        self.storage
            .update_notification_state(
                &correlation_id,
                NotificationState::SimulatedSent,
                Some(sent_at),
                Some(&subject),
                Some(&body),
            )
            .context("Failed to update notification state")?;

        info!(
            correlation_id = %correlation_id,
            "Notification processed and simulated email sent"
        );

        Ok(())
    }

    /// Build email subject and body from booking job
    fn build_email(&self, job: &BookingJob) -> Result<(String, String)> {
        // Parse booking data
        let booking: Value = serde_json::from_str(&job.booking_json)
            .context("Failed to parse booking_json")?;

        let date = booking["date"].as_str().unwrap_or("Unknown");
        let start_time = booking["start_time"].as_str().unwrap_or("Unknown");
        let end_time = booking["end_time"].as_str().unwrap_or("Unknown");
        let name = booking["name"].as_str().unwrap_or("Unknown");

        // Parse response if available
        let response_info = if let Some(ref response_json) = job.central_response_json {
            if let Ok(_resp_value) = serde_json::from_str::<Value>(response_json) {
                // Extract any useful fields from response (opaque, so we just include it)
                format!("Response: {}", response_json)
            } else {
                format!("Response: {}", response_json)
            }
        } else {
            "Booking confirmed".to_string()
        };

        // Build subject
        let subject = format!("Booking Confirmed - {}", name);

        // Build body
        let body = format!(
            "Hello {},\n\n\
            Your booking has been confirmed:\n\n\
            Date: {}\n\
            Time: {} - {}\n\
            Name: {}\n\n\
            {}\n\n\
            Thank you!",
            name, date, start_time, end_time, name, response_info
        );

        Ok((subject, body))
    }
}
