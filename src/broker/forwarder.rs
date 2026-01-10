use crate::broker::storage::BrokerStorage;
use crate::broker::types::{BookingJob, JobState, NotificationRecord, NotificationState};
use crate::config::Config;
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

const MAX_BACKOFF_MS: u64 = 300_000; // 5 minutes max
const JITTER_MS: u64 = 1000; // 1 second jitter

pub struct ForwarderWorker {
    storage: Arc<BrokerStorage>,
    http_client: Client,
    central_api_url: String,
    max_retry_attempts: u32,
    initial_backoff_ms: u64,
}

impl ForwarderWorker {
    pub fn new(storage: Arc<BrokerStorage>, config: Config) -> Result<Self> {
        let central_api_url = config
            .central_api_url
            .ok_or_else(|| anyhow::anyhow!("central_api_url not configured"))?;

        // Create HTTP client with timeouts
        let http_client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(ForwarderWorker {
            storage,
            http_client,
            central_api_url,
            max_retry_attempts: config.max_retry_attempts,
            initial_backoff_ms: config.initial_backoff_ms,
        })
    }

    /// Run the forwarder worker loop
    pub async fn run(&self) -> Result<()> {
        info!("Forwarder worker started");

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;

            match self.process_due_jobs().await {
                Ok(_) => {}
                Err(e) => {
                    error!("Error in forwarder worker: {:?}", e);
                }
            }
        }
    }

    /// Process due jobs
    async fn process_due_jobs(&self) -> Result<()> {
        let jobs = self.storage.get_due_jobs(10)?;

        for job in jobs {
            if let Err(e) = self.process_job(job).await {
                error!("Failed to process job: {:?}", e);
            }
        }

        Ok(())
    }

    /// Process a single job
    async fn process_job(&self, job: BookingJob) -> Result<()> {
        let correlation_id = job.correlation_id.clone();

        info!(
            correlation_id = %correlation_id,
            attempts = job.attempts,
            "Processing booking job"
        );

        // Update state to Sending
        self.storage
            .update_job_state(
                &correlation_id,
                JobState::Sending,
                None,
                None,
                None,
                None,
                None,
            )
            .context("Failed to update job state to Sending")?;

        // Parse booking data
        let booking: serde_json::Value = serde_json::from_str(&job.booking_json)
            .context("Failed to parse booking_json")?;

        // Build HTTP request
        let url = format!("{}/appointments/book-range", self.central_api_url);
        let request_body = json!({
            "date": booking["date"],
            "start_time": booking["start_time"],
            "end_time": booking["end_time"],
            "name": booking["name"],
        });

        info!(
            correlation_id = %correlation_id,
            url = %url,
            "Sending request to Central API"
        );

        // Make HTTP request
        match self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();

                match response.text().await {
                    Ok(response_body) => {
                        if status.is_success() {
                            // Success - update job to Confirmed
                            info!(
                                correlation_id = %correlation_id,
                                http_status = status_code,
                                "Job forwarded successfully to Central API"
                            );

                            self.storage
                                .update_job_state(
                                    &correlation_id,
                                    JobState::Confirmed,
                                    None,
                                    None,
                                    None,
                                    Some(status_code),
                                    Some(&response_body),
                                )
                                .context("Failed to update job to Confirmed")?;

                            // Create notification record
                            self.create_notification(&correlation_id, &job.notify_json)?;
                        } else {
                            // HTTP error (4xx/5xx) - mark as Failed (non-retryable)
                            warn!(
                                correlation_id = %correlation_id,
                                http_status = status_code,
                                "HTTP error from Central API, marking job as failed"
                            );

                            self.storage
                                .update_job_state(
                                    &correlation_id,
                                    JobState::Failed,
                                    None,
                                    None,
                                    Some(&format!("HTTP {}: {}", status_code, response_body)),
                                    Some(status_code),
                                    Some(&response_body),
                                )
                                .context("Failed to update job to Failed")?;
                        }
                    }
                    Err(e) => {
                        // Failed to read response body
                        warn!(
                            correlation_id = %correlation_id,
                            error = %e,
                            "Failed to read response body"
                        );
                        self.handle_retry(&correlation_id, job.attempts, &e.to_string())?;
                    }
                }
            }
            Err(e) => {
                // Network error or timeout - retry
                warn!(
                    correlation_id = %correlation_id,
                    error = %e,
                    "Network error forwarding job, will retry"
                );
                self.handle_retry(&correlation_id, job.attempts, &e.to_string())?;
            }
        }

        Ok(())
    }

    /// Handle retry with exponential backoff
    fn handle_retry(
        &self,
        correlation_id: &str,
        current_attempts: u32,
        error: &str,
    ) -> Result<()> {
        let new_attempts = current_attempts + 1;

        if new_attempts > self.max_retry_attempts {
            // Max retries exceeded - mark as Failed
            error!(
                correlation_id = %correlation_id,
                attempts = new_attempts,
                "Max retry attempts exceeded, marking job as failed"
            );

            self.storage
                .update_job_state(
                    correlation_id,
                    JobState::Failed,
                    Some(new_attempts),
                    None,
                    Some(&format!("Max retries exceeded: {}", error)),
                    None,
                    None,
                )
                .context("Failed to update job to Failed")?;

            return Ok(());
        }

        // Calculate exponential backoff with jitter
        let backoff_delay = self.calculate_backoff(new_attempts);
        let next_attempt_at = chrono::Utc::now().timestamp_millis() + backoff_delay as i64;

        warn!(
            correlation_id = %correlation_id,
            attempts = new_attempts,
            next_attempt_at = next_attempt_at,
            "Scheduling retry with exponential backoff"
        );

        // Update job back to Queued with new attempt count and next_attempt_at
        self.storage
            .update_job_state(
                correlation_id,
                JobState::Queued,
                Some(new_attempts),
                Some(next_attempt_at),
                Some(error),
                None,
                None,
            )
            .context("Failed to update job for retry")?;

        Ok(())
    }

    /// Calculate exponential backoff delay in milliseconds
    pub fn calculate_backoff(&self, attempts: u32) -> u64 {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Exponential backoff: initial_backoff_ms * 2^attempts
        let base_delay = self.initial_backoff_ms.saturating_mul(1 << attempts.min(20)); // Cap at 2^20 to avoid overflow

        // Cap at max backoff
        let delay = base_delay.min(MAX_BACKOFF_MS);

        // Add jitter: random(0, JITTER_MS)
        let jitter = rng.gen_range(0..=JITTER_MS);

        delay + jitter
    }

    /// Create notification record in outbox
    fn create_notification(&self, correlation_id: &str, notify_json: &str) -> Result<()> {
        // Parse notify data
        let notify: serde_json::Value = serde_json::from_str(notify_json)
            .context("Failed to parse notify_json")?;

        let email = notify["email"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing email in notify data"))?
            .to_string();

        let now = chrono::Utc::now().timestamp_millis();

        // Create notification record (will be populated by notifier worker)
        let notif = NotificationRecord {
            correlation_id: correlation_id.to_string(),
            email_to: email,
            state: NotificationState::Pending,
            attempts: 0,
            next_attempt_at: now, // Process immediately
            last_error: None,
            subject: String::new(), // Will be set by notifier
            body: String::new(),    // Will be set by notifier
            simulated_sent_at: None,
            created_at: now,
            updated_at: now,
        };

        self.storage
            .persist_notification(&notif)
            .context("Failed to persist notification")?;

        info!(
            correlation_id = %correlation_id,
            "Notification record created in outbox"
        );

        Ok(())
    }
}
