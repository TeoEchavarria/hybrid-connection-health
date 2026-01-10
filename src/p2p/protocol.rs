use serde::{Deserialize, Serialize};
use std::io;
use async_trait::async_trait;
use futures::{prelude::*, AsyncRead, AsyncWrite};
use libp2p::request_response::Codec;

// --- Mensajes ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub op_id: String,          // uuid string
    pub actor_id: String,       // peer id string
    pub kind: String,           // "UpsertNote"
    pub entity: String,         // "note:123"
    pub payload_json: String,   // json string
    pub created_at_ms: i64,
}

// Booking message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookingData {
    pub date: String,           // "YYYY-MM-DD"
    pub start_time: String,     // "H:MM" or "HH:MM"
    pub end_time: String,       // "H:MM" or "HH:MM"
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyData {
    pub email: String,
    pub locale: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    OpSubmit { op: Op },
    OpAck { op_id: String, ok: bool, msg: String },
    Heartbeat { role: String },
    SubmitBooking {
        correlation_id: String,
        booking: BookingData,
        notify: NotifyData,
    },
    BookingAck {
        correlation_id: String,
        status: String,  // "queued"
    },
}

// --- Codec ---

// --- Codec ---

#[derive(Debug, Clone)]
pub struct OpProtocol;

impl AsRef<str> for OpProtocol {
    fn as_ref(&self) -> &str {
        "/node-agent/rr/1"
    }
}

#[derive(Clone, Default)]
pub struct OpCodec;

#[async_trait]
impl Codec for OpCodec {
    type Protocol = OpProtocol;
    type Request = Msg;
    type Response = Msg;

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut data = Vec::new();
        io.read_to_end(&mut data).await?;
        
        if data.is_empty() {
             return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Empty request"));
        }

        let msg: Msg = serde_json::from_slice(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        Ok(msg)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut data = Vec::new();
        io.read_to_end(&mut data).await?;
        
        if data.is_empty() {
             return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Empty response"));
        }

        let msg: Msg = serde_json::from_slice(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        Ok(msg)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&req)
             .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        io.write_all(&data).await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&res)
             .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        io.write_all(&data).await?;
        Ok(())
    }
}
