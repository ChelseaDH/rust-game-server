use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        Connection { stream }
    }

    pub async fn write_event<T: Serialize>(&mut self, event: T) -> Result<(), WriteError> {
        let serialised = serde_json::to_string(&event)?;
        let len = serialised.len() as u16;
        let bytes = len.to_be_bytes();

        self.stream.write_all(&bytes[..]).await?;
        self.stream.write_all(serialised.as_bytes()).await?;
        self.stream.flush().await?;

        Ok(())
    }

    pub async fn read_event<T: DeserializeOwned>(&mut self) -> Result<T, ReadError> {
        // Read the length of the event
        let mut len_bytes = [0; 2];
        self.stream.read_exact(&mut len_bytes).await?;
        let len = u16::from_be_bytes(len_bytes);
        if len > 250 {
            return Err(ReadError::InvalidMessageLength);
        }

        // Read the event
        let mut serialised = vec![0; len as usize];
        self.stream.read_exact(&mut serialised).await?;

        Ok(serde_json::from_slice(&serialised)?)
    }

    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        self.stream.shutdown().await
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ReadError {
    #[error("Failed to serialise message")]
    Deserialise(#[from] serde_json::Error),
    #[error("Failed to read from stream")]
    Read(#[from] std::io::Error),
    #[error("Received length parameter exceeds expected bounds")]
    InvalidMessageLength,
}

#[derive(thiserror::Error, Debug)]
pub enum WriteError {
    #[error("Failed to serialise Event")]
    Serialise(#[from] serde_json::Error),
    #[error("Failed to write to stream")]
    Write(#[from] std::io::Error),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ErrorCategory {
    Serialisation,
    Deserialisation,
    ReadWrite,
    InvalidParameters,
}

pub trait HasErrorCategory {
    fn category(&self) -> ErrorCategory;
}

impl HasErrorCategory for ReadError {
    fn category(&self) -> ErrorCategory {
        match self {
            ReadError::Deserialise(_) => ErrorCategory::Deserialisation,
            ReadError::Read(_) => ErrorCategory::ReadWrite,
            ReadError::InvalidMessageLength => ErrorCategory::InvalidParameters,
        }
    }
}

impl HasErrorCategory for WriteError {
    fn category(&self) -> ErrorCategory {
        match self {
            WriteError::Serialise(_) => ErrorCategory::Serialisation,
            WriteError::Write(_) => ErrorCategory::ReadWrite,
        }
    }
}
