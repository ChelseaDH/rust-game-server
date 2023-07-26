use std::fmt;
use std::fmt::Formatter;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::__private::DisplayAsDisplay;
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

    pub async fn write_event<T: Serialize>(&mut self, event: T) -> Result<(), ReadWriteError> {
        let serialised = serde_json::to_string(&event)?;
        let len = serialised.len() as u16;
        let bytes = len.to_be_bytes();

        self.stream.write_all(&bytes[..]).await?;
        self.stream.write_all(serialised.as_bytes()).await?;
        self.stream.flush().await?;

        Ok(())
    }

    pub async fn read_event<T: DeserializeOwned>(&mut self) -> Result<T, ReadWriteError> {
        // Read the length of the event
        let mut len_bytes = [0; 2];
        self.read_to_buffer(&mut len_bytes).await?;
        let len = u16::from_be_bytes(len_bytes);

        // Read the event
        let mut serialised = vec![0; len as usize];
        self.read_to_buffer(&mut serialised).await?;

        Ok(serde_json::from_slice(&serialised)?)
    }

    async fn read_to_buffer(&mut self, buffer: &mut [u8]) -> Result<(), ReadWriteError> {
        loop {
            match self.stream.read(buffer).await {
                Ok(0) => continue,
                Ok(_) => break,
                Err(_) => {
                    eprintln!("Error reading from socket");
                    continue;
                }
            }
        }

        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ReadWriteError {
    Serde(#[from] serde_json::Error),
    StreamReadWrite(#[from] std::io::Error),
}

impl fmt::Display for ReadWriteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_display())
    }
}
