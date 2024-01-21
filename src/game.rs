use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::server::DispatchMode;

#[derive(Debug)]
pub enum GameServerEvent<E: Serialize + DeserializeOwned> {
    DispatchToClient {
        dispatch_mode: DispatchMode,
        event: E,
    },
    GameOver,
}

#[async_trait]
pub trait GameServer<E: DeserializeOwned> {
    async fn begin(&self);
    async fn handle_event(&mut self, event: E);
}

#[derive(Debug, PartialEq)]
pub enum GameClientEvent<E: Serialize + DeserializeOwned> {
    DispatchToServer { event: E },
    GameOver,
}

#[async_trait]
pub trait GameClient<E: DeserializeOwned> {
    async fn handle_game_started_event(&self);
    async fn handle_event(&mut self, event: E);
}
