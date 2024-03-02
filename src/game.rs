use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;

use crate::server::DispatchMode;

pub enum Game {
    TicTacToe,
}

#[derive(Debug)]
pub enum GameServerEvent {
    DispatchToClient {
        dispatch_mode: DispatchMode,
        event: Vec<u8>,
    },
    GameOver,
}

#[async_trait]
pub trait GameServer {
    async fn begin(&self);
    async fn handle_event(&mut self, event: Vec<u8>);
}

#[derive(Debug, PartialEq)]
pub enum GameClientEvent {
    DispatchToServer { event: Vec<u8> },
    GameOver,
}

#[async_trait]
pub trait GameClient {
    async fn handle_game_started_event(&self);
    async fn handle_event(&mut self, event: Vec<u8>) -> Result<(), io::Error>;
}

pub fn serialize_event(event: impl Serialize) -> Vec<u8> {
    serde_json::to_vec(&event).unwrap()
}

pub fn deserialize_event<T: DeserializeOwned>(event: Vec<u8>) -> T {
    serde_json::from_slice(&event).unwrap()
}
