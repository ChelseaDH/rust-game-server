use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::join;
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::connection::{Connection, ErrorCategory, HasErrorCategory, ReadError, WriteError};
use crate::game::{Game, GameServer, GameServerEvent};
pub use crate::server::player::{get_alternative_player_id, Player, PLAYER_ONE_ID, PLAYER_TWO_ID};
use crate::tic_tac_toe::TicTacToeServer;

mod player;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum State {
    PreInitialise,
    InProgress,
    GameOver,
    Error {
        category: ErrorCategory,
        player_id: u8,
    },
}

#[derive(Debug, Deserialize)]
pub enum ServerEvent {
    BeginGame,
    PlayerDisconnected,
}

pub trait ClientConnectionType {}

pub struct LocalConnection {
    connection: Connection,
}

impl ClientConnectionType for LocalConnection {}

pub struct OnlineConnection {
    player_one: Player,
    player_two: Player,
}

impl ClientConnectionType for OnlineConnection {}

pub struct Server<C>
where
    C: ClientConnectionType,
{
    state: State,
    client_connection: C,
    channel: (Sender<ServerEvent>, Receiver<ServerEvent>),
    game: Box<dyn GameServer + Send + Sync>,
    game_receiver: Receiver<GameServerEvent>,
}

impl Server<LocalConnection> {
    pub fn new(connection: Connection, game: Game) -> Server<LocalConnection> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let game: Box<dyn GameServer + Send + Sync> = match game {
            Game::TicTacToe => Box::new(TicTacToeServer::new(game_sender)),
        };

        Server {
            state: State::PreInitialise,
            client_connection: LocalConnection { connection },
            channel: mpsc::channel(1),
            game,
            game_receiver,
        }
    }
}

impl Server<OnlineConnection> {
    pub fn new(player_one: Player, player_two: Player, game: Game) -> Server<OnlineConnection> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let game: Box<dyn GameServer + Send + Sync> = match game {
            Game::TicTacToe => Box::new(TicTacToeServer::new(game_sender)),
        };

        Server {
            state: State::PreInitialise,
            client_connection: OnlineConnection {
                player_one,
                player_two,
            },
            channel: mpsc::channel(1),
            game,
            game_receiver,
        }
    }
}

pub enum IncomingEvent {
    Server(ServerEvent),
    Game(GameServerEvent),
    Client(Vec<u8>),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum OutgoingEvent {
    ErrorOccurred(Error),
    GameStarted,
    Shutdown,
    Game { event: Vec<u8> },
}

#[derive(Copy, Clone, Serialize, Deserialize, thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid message sent.")]
    InvalidMessage,
}

#[async_trait]
pub trait ServerGameMode {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)>;
    async fn dispatch_event_to_player(
        &mut self,
        event: &OutgoingEvent,
        player_id: u8,
    ) -> Result<(), (WriteError, u8)>;
    async fn dispatch_event_to_all_players(
        &mut self,
        event: &OutgoingEvent,
    ) -> Result<(), (WriteError, u8)>;
    async fn shutdown_all_client_connections(&mut self);
}

#[async_trait]
impl ServerGameMode for Server<LocalConnection> {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)> {
        return tokio::select! {
            result = self.channel.1.recv() => Ok(IncomingEvent::Server(result.unwrap())),
            result = self.game_receiver.recv() => Ok(IncomingEvent::Game(result.unwrap())),
            result = self.client_connection.connection.read_event() => result.map_err(|e| (e, PLAYER_ONE_ID)).map(IncomingEvent::Client),
        };
    }

    async fn dispatch_event_to_player(
        &mut self,
        event: &OutgoingEvent,
        _player_id: u8,
    ) -> Result<(), (WriteError, u8)> {
        self.client_connection
            .connection
            .write_event(event)
            .await
            .map_err(|e| (e, PLAYER_ONE_ID))
    }

    async fn dispatch_event_to_all_players(
        &mut self,
        event: &OutgoingEvent,
    ) -> Result<(), (WriteError, u8)> {
        self.dispatch_event_to_player(event, PLAYER_ONE_ID).await
    }

    async fn shutdown_all_client_connections(&mut self) {
        let _ = self.client_connection.connection.shutdown().await;
    }
}

#[async_trait]
impl ServerGameMode for Server<OnlineConnection> {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)> {
        return tokio::select! {
            result = self.channel.1.recv() => Ok(IncomingEvent::Server(result.unwrap())),
            result = self.game_receiver.recv() => Ok(IncomingEvent::Game(result.unwrap())),
            result = self.client_connection.player_one.connection.read_event() => result.map_err(|e| (e, PLAYER_ONE_ID)).map(IncomingEvent::Client),
            result = self.client_connection.player_two.connection.read_event() => result.map_err(|e| (e, PLAYER_TWO_ID)).map(IncomingEvent::Client),
        };
    }

    async fn dispatch_event_to_player(
        &mut self,
        event: &OutgoingEvent,
        player_id: u8,
    ) -> Result<(), (WriteError, u8)> {
        match player_id {
            PLAYER_ONE_ID => {
                self.client_connection
                    .player_one
                    .connection
                    .write_event(event)
                    .await
            }
            PLAYER_TWO_ID => {
                self.client_connection
                    .player_two
                    .connection
                    .write_event(event)
                    .await
            }
            _ => panic!("Unexpected id provided"),
        }
        .map_err(|e| (e, player_id))
    }

    async fn dispatch_event_to_all_players(
        &mut self,
        event: &OutgoingEvent,
    ) -> Result<(), (WriteError, u8)> {
        self.dispatch_event_to_player(event, PLAYER_ONE_ID).await?;
        self.dispatch_event_to_player(event, PLAYER_TWO_ID).await
    }

    async fn shutdown_all_client_connections(&mut self) {
        let _ = join!(
            self.client_connection.player_one.connection.shutdown(),
            self.client_connection.player_two.connection.shutdown()
        );
    }
}

impl<C> Server<C>
where
    C: ClientConnectionType,
    Self: ServerGameMode,
{
    pub async fn init(&mut self) {
        self.channel.0.send(ServerEvent::BeginGame).await.unwrap();
        self.run().await
    }

    async fn run(&mut self) {
        loop {
            match self.state {
                State::GameOver => break,
                State::Error {
                    category: error_type,
                    player_id,
                } => {
                    self.handle_error(error_type, player_id).await;
                    break;
                }
                _ => match self.get_next_incoming_event().await {
                    Ok(event) => {
                        if let Err((error, id)) = self.handle_incoming_event(event).await {
                            self.state = State::Error {
                                category: error.category(),
                                player_id: id,
                            }
                        }
                    }
                    Err((error, id)) => {
                        self.state = State::Error {
                            category: error.category(),
                            player_id: id,
                        }
                    }
                },
            }
        }
    }

    async fn handle_incoming_event(
        &mut self,
        event: IncomingEvent,
    ) -> Result<(), (WriteError, u8)> {
        match (self.state, event) {
            (State::PreInitialise, IncomingEvent::Server(ServerEvent::BeginGame)) => {
                self.dispatch_event_to_all_players(&OutgoingEvent::GameStarted)
                    .await?;

                self.game.begin().await;
                self.state = State::InProgress;
                Ok(())
            }
            (State::InProgress, IncomingEvent::Client(event)) => {
                self.game.handle_event(event).await;

                Ok(())
            }
            (
                State::InProgress,
                IncomingEvent::Game(GameServerEvent::DispatchToClient {
                    dispatch_mode,
                    event,
                }),
            ) => self.dispatch_game_event(dispatch_mode, event).await,
            (State::InProgress, IncomingEvent::Game(GameServerEvent::GameOver)) => {
                self.state = State::GameOver;

                Ok(())
            }
            _ => panic!("Invalid state for event"),
        }
    }

    /// Handles errors that can occur when reading/writing from/to a Client connection.
    ///
    /// Possible errors to be handled; IO, invalid parameters, and serialisation and deserialisation
    /// of `Event` objects.
    ///
    /// # Parameters
    ///
    /// - `error_category`: The category of error.
    /// - `player_id`: The ID of the player associated with the error.
    ///
    /// # Panics
    ///
    /// This function will panic if the error category is `ErrorCategory::Serialise`, indicating
    /// an unexpected serialisation error.
    ///
    /// # Example
    ///
    /// ```
    /// handle_error(ErrorCategory::Deserialise, 1).await;
    /// ```
    async fn handle_error(&mut self, error_category: ErrorCategory, player_id: u8) {
        match error_category {
            ErrorCategory::Serialisation => {
                panic!("Error occurred while attempting to serialise an event.")
            }
            ErrorCategory::Deserialisation | ErrorCategory::InvalidParameters => {
                let _ = self
                    .dispatch_event_to_player(
                        &OutgoingEvent::ErrorOccurred(Error::InvalidMessage),
                        player_id,
                    )
                    .await;
                let _ = self
                    .dispatch_event_to_all_players(&OutgoingEvent::Shutdown)
                    .await;
                self.shutdown_all_client_connections().await;
            }
            ErrorCategory::ReadWrite => {
                let _ = self
                    .dispatch_event_to_player(
                        &OutgoingEvent::Shutdown,
                        get_alternative_player_id(player_id),
                    )
                    .await;
                self.shutdown_all_client_connections().await;
            }
        }
    }

    async fn dispatch_game_event(
        &mut self,
        dispatch_mode: DispatchMode,
        event: Vec<u8>,
    ) -> Result<(), (WriteError, u8)> {
        let event = OutgoingEvent::Game { event };

        match dispatch_mode {
            DispatchMode::AllPlayers => self.dispatch_event_to_all_players(&event),
            DispatchMode::SinglePlayer { player_id } => {
                self.dispatch_event_to_player(&event, player_id)
            }
        }
        .await
    }
}

#[derive(Debug, Deserialize)]
pub enum DispatchMode {
    AllPlayers,
    SinglePlayer { player_id: u8 },
}
