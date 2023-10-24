use crate::server::DispatchMode;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub enum GameEvent<E: Serialize + Send> {
    DispatchToClient {
        dispatch_mode: DispatchMode,
        event: E,
    },
    GameOver,
}

#[async_trait]
pub trait GameServer<E: DeserializeOwned> {
    async fn begin(&mut self);
    async fn handle_event(&mut self, event: E);
}
