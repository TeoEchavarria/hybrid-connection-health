use serde::{Deserialize, Serialize};

/// Booking job state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    Sending,
    Confirmed,
    Failed,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Sending => "sending",
            JobState::Confirmed => "confirmed",
            JobState::Failed => "failed",
        }
    }
}

/// Booking job stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookingJob {
    pub correlation_id: String,
    pub booking_json: String,
    pub notify_json: String,
    pub state: JobState,
    pub attempts: u32,
    pub next_attempt_at: i64,      // epoch ms
    pub last_error: Option<String>,
    pub http_status: Option<u16>,
    pub central_response_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Notification state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationState {
    Pending,
    SimulatedSent,
    Failed,
}

impl NotificationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationState::Pending => "pending",
            NotificationState::SimulatedSent => "simulated_sent",
            NotificationState::Failed => "failed",
        }
    }
}

/// Notification record stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecord {
    pub correlation_id: String,
    pub email_to: String,
    pub state: NotificationState,
    pub attempts: u32,
    pub next_attempt_at: i64,
    pub last_error: Option<String>,
    pub subject: String,
    pub body: String,
    pub simulated_sent_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
