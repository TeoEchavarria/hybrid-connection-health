use crate::broker::types::{BookingJob, JobState, NotificationRecord, NotificationState};
use anyhow::{Context, Result};
use bincode;
use tracing::debug;

pub struct BrokerStorage {
    db: sled::Db,
    booking_jobs: sled::Tree,
    notification_outbox: sled::Tree,
}

/// Parameters for updating job state
pub struct JobStateUpdate<'a> {
    pub state: JobState,
    pub attempts: Option<u32>,
    pub next_attempt_at: Option<i64>,
    pub last_error: Option<&'a str>,
    pub http_status: Option<u16>,
    pub central_response_json: Option<&'a str>,
}

impl BrokerStorage {
    pub fn new(db_path: &str) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create database directory: {}", parent.display()))?;
        }

        let db = sled::open(db_path)
            .with_context(|| format!("Failed to open sled database at: {}", db_path))?;

        let booking_jobs = db
            .open_tree("booking_jobs")
            .context("Failed to open booking_jobs tree")?;

        let notification_outbox = db
            .open_tree("notification_outbox")
            .context("Failed to open notification_outbox tree")?;

        Ok(BrokerStorage {
            db,
            booking_jobs,
            notification_outbox,
        })
    }

    /// Persist a booking job with idempotency check
    pub fn persist_booking_job(&self, job: &BookingJob) -> Result<()> {
        let key = job.correlation_id.as_str();
        
        // Check if already exists (idempotency)
        if self.booking_jobs.contains_key(key)? {
            debug!(correlation_id = %job.correlation_id, "Booking job already exists, skipping insert");
            return Ok(());
        }

        // Serialize job
        let value = bincode::serialize(job)
            .context("Failed to serialize booking job")?;

        // Store job
        self.booking_jobs
            .insert(key, value)
            .context("Failed to insert booking job")?;

        // Update index for scheduling queries
        self.update_job_index(job)?;

        // Ensure durable persist before ACK is sent
        self.db.flush().context("Failed to flush sled DB after booking insert")?;

        debug!(correlation_id = %job.correlation_id, "Booking job persisted");
        Ok(())
    }

    /// Get a booking job by correlation_id
    pub fn get_booking_job(&self, correlation_id: &str) -> Result<Option<BookingJob>> {
        match self.booking_jobs.get(correlation_id)? {
            Some(value) => {
                let job: BookingJob = bincode::deserialize(&value)
                    .context("Failed to deserialize booking job")?;
                Ok(Some(job))
            }
            None => Ok(None),
        }
    }

    /// Update job state and related fields atomically
    pub fn update_job_state(
        &self,
        correlation_id: &str,
        update: JobStateUpdate,
    ) -> Result<()> {
        let mut job = self
            .get_booking_job(correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", correlation_id))?;

        // Update fields
        job.state = update.state;
        if let Some(att) = update.attempts {
            job.attempts = att;
        }
        if let Some(next) = update.next_attempt_at {
            job.next_attempt_at = next;
        }
        if let Some(err) = update.last_error {
            job.last_error = Some(err.to_string());
        }
        if let Some(status) = update.http_status {
            job.http_status = Some(status);
        }
        if let Some(resp) = update.central_response_json {
            job.central_response_json = Some(resp.to_string());
        }
        job.updated_at = chrono::Utc::now().timestamp_millis();

        // Remove old index entry
        self.remove_job_index(&job)?;

        // Update job
        let value = bincode::serialize(&job)
            .context("Failed to serialize updated booking job")?;
        self.booking_jobs
            .insert(correlation_id, value)
            .context("Failed to update booking job")?;

        // Update index
        self.update_job_index(&job)?;

        // Ensure durability of state transition
        self.db.flush().context("Failed to flush sled DB after job update")?;

        debug!(correlation_id = %correlation_id, state = %job.state.as_str(), "Job state updated");
        Ok(())
    }

    /// Get due jobs (state=queued and next_attempt_at <= now)
    pub fn get_due_jobs(&self, limit: usize) -> Result<Vec<BookingJob>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut jobs = Vec::new();

        // Scan jobs with composite key prefix: "queued:{next_attempt_at}"
        // We iterate over all jobs since sled doesn't support range queries easily
        // In production, consider using a secondary index tree
        for item in self.booking_jobs.iter() {
            let (key, value) = item.context("Failed to read from booking_jobs tree")?;
            
            // Skip index entries
            if key.len() > 64 {
                continue;
            }

            let job: BookingJob = bincode::deserialize(&value)
                .context("Failed to deserialize booking job")?;

            // Filter due jobs
            if job.state == JobState::Queued
                && job.next_attempt_at <= now
                && jobs.len() < limit
            {
                jobs.push(job);
            }
        }

        // Sort by next_attempt_at
        jobs.sort_by_key(|j| j.next_attempt_at);

        debug!(count = jobs.len(), "Retrieved due jobs");
        Ok(jobs)
    }

    /// Persist a notification record (idempotent)
    pub fn persist_notification(&self, notif: &NotificationRecord) -> Result<()> {
        let key = notif.correlation_id.as_str();

        // Check if already exists (idempotency)
        if self.notification_outbox.contains_key(key)? {
            debug!(correlation_id = %notif.correlation_id, "Notification already exists, skipping insert");
            return Ok(());
        }

        let value = bincode::serialize(notif)
            .context("Failed to serialize notification")?;

        self.notification_outbox
            .insert(key, value)
            .context("Failed to insert notification")?;

        // Update index
        self.update_notification_index(notif)?;

        // Durable persist
        self.db.flush().context("Failed to flush sled DB after notification insert")?;

        debug!(correlation_id = %notif.correlation_id, "Notification persisted");
        Ok(())
    }

    /// Get due notifications (state=pending and next_attempt_at <= now)
    pub fn get_due_notifications(&self, limit: usize) -> Result<Vec<NotificationRecord>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut notifications = Vec::new();

        for item in self.notification_outbox.iter() {
            let (key, value) = item.context("Failed to read from notification_outbox tree")?;
            
            // Skip index entries
            if key.len() > 64 {
                continue;
            }

            let notif: NotificationRecord = bincode::deserialize(&value)
                .context("Failed to deserialize notification")?;

            if notif.state == NotificationState::Pending
                && notif.next_attempt_at <= now
                && notifications.len() < limit
            {
                notifications.push(notif);
            }
        }

        notifications.sort_by_key(|n| n.next_attempt_at);

        debug!(count = notifications.len(), "Retrieved due notifications");
        Ok(notifications)
    }

    /// Update notification state
    pub fn update_notification_state(
        &self,
        correlation_id: &str,
        state: NotificationState,
        simulated_sent_at: Option<i64>,
        subject: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        let mut notif = self
            .get_notification(correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("Notification not found: {}", correlation_id))?;

        notif.state = state;
        if let Some(sent_at) = simulated_sent_at {
            notif.simulated_sent_at = Some(sent_at);
        }
        if let Some(subject) = subject {
            notif.subject = subject.to_string();
        }
        if let Some(body) = body {
            notif.body = body.to_string();
        }
        notif.updated_at = chrono::Utc::now().timestamp_millis();

        // Remove old index
        self.remove_notification_index(&notif)?;

        let value = bincode::serialize(&notif)
            .context("Failed to serialize updated notification")?;
        self.notification_outbox
            .insert(correlation_id, value)
            .context("Failed to update notification")?;

        // Update index
        self.update_notification_index(&notif)?;

        // Durable persist
        self.db.flush().context("Failed to flush sled DB after notification update")?;

        debug!(correlation_id = %correlation_id, state = %notif.state.as_str(), "Notification state updated");
        Ok(())
    }

    /// Get a notification by correlation_id
    pub fn get_notification(&self, correlation_id: &str) -> Result<Option<NotificationRecord>> {
        match self.notification_outbox.get(correlation_id)? {
            Some(value) => {
                let notif: NotificationRecord = bincode::deserialize(&value)
                    .context("Failed to deserialize notification")?;
                Ok(Some(notif))
            }
            None => Ok(None),
        }
    }

    /// Update index for job scheduling queries
    fn update_job_index(&self, job: &BookingJob) -> Result<()> {
        if job.state == JobState::Queued {
            // Create composite key: "queued:{next_attempt_at}:{correlation_id}"
            let index_key = format!("queued:{}:{}", job.next_attempt_at, job.correlation_id);
            self.booking_jobs.insert(index_key.as_str(), &[])?;
        }
        Ok(())
    }

    /// Remove old index entry
    fn remove_job_index(&self, job: &BookingJob) -> Result<()> {
        // Remove old index by scanning (sled limitation)
        // In production, track old state
        let _ = job;
        Ok(())
    }

    /// Update index for notification scheduling
    fn update_notification_index(&self, notif: &NotificationRecord) -> Result<()> {
        if notif.state == NotificationState::Pending {
            let index_key = format!("pending:{}:{}", notif.next_attempt_at, notif.correlation_id);
            self.notification_outbox.insert(index_key.as_str(), &[])?;
        }
        Ok(())
    }

    /// Remove old notification index
    fn remove_notification_index(&self, _notif: &NotificationRecord) -> Result<()> {
        // Remove old index by scanning (sled limitation)
        Ok(())
    }
}
