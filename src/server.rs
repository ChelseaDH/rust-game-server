use serde::{Deserialize, Serialize};

use crate::client;
use crate::connection::{Connection, ErrorCategory, HasErrorCategory, ReadError, WriteError};
use async_trait::async_trait;
use tokio::join;
use tokio::sync::mpsc::{self, Receiver, Sender};

pub const PLAYER_ONE_ID: u8 = 1;
pub const PLAYER_TWO_ID: u8 = 2;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum State {
    PreInitialise,
    PlayerTurn,
    GameOver(Outcome),
    Error {
        category: ErrorCategory,
        player_id: u8,
    },
}

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Draw,
    WinnerFound { player_id: u8 },
}

pub trait ConnectionType {}

pub struct LocalConnection {
    connection: Connection,
}
impl ConnectionType for LocalConnection {}

pub struct OnlineConnection {
    player_one: Player,
    player_two: Player,
}
impl ConnectionType for OnlineConnection {}

pub struct Server<T: ConnectionType> {
    connection: T,
    current_player: u8,
    board: Board,
    state: State,
    channel: (Sender<ServerEvent>, Receiver<ServerEvent>),
}

impl Server<LocalConnection> {
    pub fn new(connection: Connection) -> Self {
        Server {
            connection: LocalConnection { connection },
            current_player: PLAYER_ONE_ID,
            board: Board::new(),
            state: State::PreInitialise,
            channel: mpsc::channel(1),
        }
    }
}

impl Server<OnlineConnection> {
    pub fn new(player_one: Player, player_two: Player) -> Self {
        Server {
            connection: OnlineConnection {
                player_one,
                player_two,
            },
            current_player: PLAYER_ONE_ID,
            board: Board::new(),
            state: State::PreInitialise,
            channel: mpsc::channel(1),
        }
    }
}

#[async_trait]
pub trait ServerGameMode {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)>;
    async fn dispatch_event_to_player(
        &mut self,
        event: Event,
        player_id: u8,
    ) -> Result<(), (WriteError, u8)>;
    async fn dispatch_event_to_all_players(&mut self, event: Event)
        -> Result<(), (WriteError, u8)>;
    async fn shutdown_all_client_connections(&mut self);
}

#[async_trait]
impl ServerGameMode for Server<LocalConnection> {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)> {
        return tokio::select! {
            result = self.channel.1.recv() => Ok(IncomingEvent::Server(result.unwrap())),
            result = self.connection.connection.read_event() => result.map_err(|e| (e, PLAYER_ONE_ID)).map(IncomingEvent::Client),
        };
    }

    async fn dispatch_event_to_player(
        &mut self,
        event: Event,
        _player_id: u8,
    ) -> Result<(), (WriteError, u8)> {
        self.connection
            .connection
            .write_event(event)
            .await
            .map_err(|e| (e, PLAYER_ONE_ID))
    }

    async fn dispatch_event_to_all_players(
        &mut self,
        event: Event,
    ) -> Result<(), (WriteError, u8)> {
        self.dispatch_event_to_player(event, PLAYER_ONE_ID).await
    }

    async fn shutdown_all_client_connections(&mut self) {
        let _ = self.connection.connection.shutdown().await;
    }
}

#[async_trait]
impl ServerGameMode for Server<OnlineConnection> {
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, (ReadError, u8)> {
        return tokio::select! {
            result = self.channel.1.recv() => Ok(IncomingEvent::Server(result.unwrap())),
            result = self.connection.player_one.connection.read_event() => result.map_err(|e| (e, PLAYER_ONE_ID)).map(IncomingEvent::Client),
            result = self.connection.player_two.connection.read_event() => result.map_err(|e| (e, PLAYER_TWO_ID)).map(IncomingEvent::Client),
        };
    }

    async fn dispatch_event_to_player(
        &mut self,
        event: Event,
        player_id: u8,
    ) -> Result<(), (WriteError, u8)> {
        match player_id {
            PLAYER_ONE_ID => {
                self.connection
                    .player_one
                    .connection
                    .write_event(event)
                    .await
            }
            PLAYER_TWO_ID => {
                self.connection
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
        event: Event,
    ) -> Result<(), (WriteError, u8)> {
        let event_copy = event;
        self.dispatch_event_to_player(event, PLAYER_ONE_ID).await?;
        self.dispatch_event_to_player(event_copy, PLAYER_TWO_ID)
            .await
    }

    async fn shutdown_all_client_connections(&mut self) {
        let _ = join!(
            self.connection.player_one.connection.shutdown(),
            self.connection.player_two.connection.shutdown()
        );
    }
}

impl<T: ConnectionType> Server<T>
where
    Self: ServerGameMode,
{
    pub async fn init(&mut self) {
        self.channel.0.send(ServerEvent::BeginGame).await.unwrap();
        self.run().await
    }

    async fn run(&mut self) {
        loop {
            match self.state {
                State::GameOver(_) => break,
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
                self.dispatch_event_to_all_players(Event::GameStarted)
                    .await?;
                self.dispatch_board_updated_event().await?;
                self.dispatch_player_turn_event(DispatchMode::AllPlayers)
                    .await?;

                self.state = State::PlayerTurn;
                Ok(())
            }
            (
                State::PlayerTurn,
                IncomingEvent::Client(client::Event::MoveMade {
                    player_id,
                    move_index,
                }),
            ) => {
                if let Err(error) = self.handle_move_made_event(player_id, move_index) {
                    self.dispatch_event_to_player(Event::ErrorOccurred(error), self.current_player)
                        .await?;
                    self.dispatch_player_turn_event(DispatchMode::CurrentPlayer)
                        .await?;
                    return Ok(());
                };

                self.dispatch_board_updated_event().await?;
                match self.board.determine_outcome() {
                    None => {
                        self.swap_player();
                        self.dispatch_player_turn_event(DispatchMode::AllPlayers)
                            .await
                    }
                    Some(outcome) => {
                        self.state = State::GameOver(outcome);
                        self.dispatch_event_to_all_players(Event::GameOver { outcome })
                            .await
                    }
                }
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
                        Event::ErrorOccurred(Error::InvalidMessage),
                        player_id,
                    )
                    .await;
                let _ = self.dispatch_event_to_all_players(Event::Shutdown).await;
                self.shutdown_all_client_connections().await;
            }
            ErrorCategory::ReadWrite => {
                let alternative_player_id = self.get_alternative_player_id(player_id);
                let _ = self
                    .dispatch_event_to_player(Event::Shutdown, alternative_player_id)
                    .await;
                self.shutdown_all_client_connections().await;
            }
        }
    }

    fn swap_player(&mut self) {
        self.current_player = self.get_alternative_player_id(self.current_player)
    }

    fn get_alternative_player_id(&mut self, player_id: u8) -> u8 {
        if player_id == PLAYER_ONE_ID {
            PLAYER_TWO_ID
        } else {
            PLAYER_ONE_ID
        }
    }

    async fn dispatch_board_updated_event(&mut self) -> Result<(), (WriteError, u8)> {
        let board_cells = self.board.cells.map(|cell| match cell.state {
            BoardCellState::Empty => None,
            BoardCellState::Occupied { player_id } => Some(player_id),
        });

        self.dispatch_event_to_all_players(Event::BoardUpdated { board_cells })
            .await
    }

    async fn dispatch_player_turn_event(
        &mut self,
        dispatch_mode: DispatchMode,
    ) -> Result<(), (WriteError, u8)> {
        let event = Event::PlayerTurn(self.current_player);

        match dispatch_mode {
            DispatchMode::AllPlayers => self.dispatch_event_to_all_players(event).await,
            DispatchMode::CurrentPlayer => {
                self.dispatch_event_to_player(event, self.current_player)
                    .await
            }
        }
    }

    fn handle_move_made_event(&mut self, player_id: u8, move_index: usize) -> Result<(), Error> {
        if player_id != self.current_player {
            return Err(Error::UnexpectedPlayer);
        }

        self.board.add_move(player_id, move_index)
    }
}

pub enum DispatchMode {
    AllPlayers,
    CurrentPlayer,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum Event {
    GameOver {
        outcome: Outcome,
    },
    BoardUpdated {
        board_cells: [Option<u8>; BOARD_SIZE],
    },
    PlayerTurn(u8),
    ErrorOccurred(Error),
    GameStarted,
    Shutdown,
}

#[derive(Debug, Deserialize)]
pub enum ServerEvent {
    BeginGame,
    PlayerDisconnected,
}

#[derive(Deserialize)]
pub enum IncomingEvent {
    Server(ServerEvent),
    Client(client::Event),
}

#[derive(Debug)]
pub struct Player {
    id: u8,
    connection: Connection,
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Player {
    pub fn new_player_one(connection: Connection) -> Player {
        Player {
            id: PLAYER_ONE_ID,
            connection,
        }
    }

    pub fn new_player_two(connection: Connection) -> Player {
        Player {
            id: PLAYER_TWO_ID,
            connection,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum BoardCellState {
    Empty,
    Occupied { player_id: u8 },
}

#[derive(Copy, Clone, PartialEq)]
struct BoardCell {
    state: BoardCellState,
}

impl BoardCell {
    fn new() -> BoardCell {
        BoardCell {
            state: BoardCellState::Empty,
        }
    }

    fn is_occupied(&self) -> bool {
        match self.state {
            BoardCellState::Occupied { player_id: _ } => true,
            BoardCellState::Empty => false,
        }
    }

    fn get_occupying_player_id(&self) -> u8 {
        match self.state {
            BoardCellState::Occupied { player_id } => player_id,
            BoardCellState::Empty => {
                panic!("Cannot retrieve occupying player id from an empty cell.")
            }
        }
    }
}

pub const BOARD_SIZE: usize = 9;

struct Board {
    cells: [BoardCell; BOARD_SIZE],
}

impl Board {
    fn new() -> Board {
        Board {
            cells: [BoardCell::new(); BOARD_SIZE],
        }
    }

    fn add_move(&mut self, player_id: u8, cell_index: usize) -> Result<(), Error> {
        if cell_index >= BOARD_SIZE {
            return Err(Error::InvalidCellIndex);
        }

        let cell = &mut self.cells[cell_index];
        match cell.state {
            BoardCellState::Empty => {
                cell.state = BoardCellState::Occupied { player_id };

                Ok(())
            }
            BoardCellState::Occupied { player_id: _ } => Err(Error::CellOccupied),
        }
    }

    /// Calculates the outcome of the current state of the `Board`
    ///
    /// An `Outcome::WinnerFound` is determined if the same player occupies an entire row, column,
    /// or diagonal. An `Outcome::Draw` is determined if the board if fully occupied with no winners.
    ///
    /// # Returns
    ///
    /// - `Some(Outcome)` if an outcome is found.
    /// - `None` if there is no outcome yet (the game is ongoing).
    fn determine_outcome(&self) -> Option<Outcome> {
        // If first cell is occupied, check for win in first row, column, and left diagonal
        if self.cells[0].is_occupied()
            && ((self.cells[0] == self.cells[1] && self.cells[0] == self.cells[2])
                || (self.cells[0] == self.cells[3] && self.cells[0] == self.cells[6])
                || (self.cells[0] == self.cells[4] && self.cells[0] == self.cells[8]))
        {
            return Some(Outcome::WinnerFound {
                player_id: self.cells[0].get_occupying_player_id(),
            });
        }

        // Check for win in second column
        if self.cells[1].is_occupied()
            && self.cells[1] == self.cells[4]
            && self.cells[1] == self.cells[7]
        {
            return Some(Outcome::WinnerFound {
                player_id: self.cells[1].get_occupying_player_id(),
            });
        }

        // Check for win in third column and right diagonal
        if self.cells[2].is_occupied()
            && ((self.cells[2] == self.cells[5] && self.cells[2] == self.cells[8])
                || (self.cells[2] == self.cells[4] && self.cells[2] == self.cells[6]))
        {
            return Some(Outcome::WinnerFound {
                player_id: self.cells[2].get_occupying_player_id(),
            });
        }

        // Check for win in second row
        if self.cells[3].is_occupied()
            && self.cells[3] == self.cells[4]
            && self.cells[3] == self.cells[5]
        {
            return Some(Outcome::WinnerFound {
                player_id: self.cells[3].get_occupying_player_id(),
            });
        }

        // Check for win in third row
        if self.cells[6].is_occupied()
            && self.cells[6] == self.cells[7]
            && self.cells[6] == self.cells[8]
        {
            return Some(Outcome::WinnerFound {
                player_id: self.cells[6].get_occupying_player_id(),
            });
        }

        // Check for draw
        if self.cells.iter().all(|cell| cell.is_occupied()) {
            return Some(Outcome::Draw);
        }

        None
    }
}

#[derive(Copy, Clone, Serialize, Deserialize, thiserror::Error, Debug)]
pub enum Error {
    #[error("The input should be a number between 0 and {}.", BOARD_SIZE -1)]
    InvalidCellIndex,
    #[error("This cell is already occupied.")]
    CellOccupied,
    #[error("It's not your turn.")]
    UnexpectedPlayer,
    #[error("Invalid message sent.")]
    InvalidMessage,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSetup {
        occupied_cell_player1: BoardCell,
        occupied_cell_player2: BoardCell,
        board: Board,
    }

    impl TestSetup {
        fn new() -> TestSetup {
            TestSetup {
                occupied_cell_player1: BoardCell {
                    state: BoardCellState::Occupied { player_id: 1 },
                },
                occupied_cell_player2: BoardCell {
                    state: BoardCellState::Occupied { player_id: 2 },
                },
                board: Board::new(),
            }
        }
    }

    #[test]
    fn determine_outcome_empty_board_in_progress() {
        let board = Board::new();

        let outcome = board.determine_outcome();
        assert_eq!(None, outcome);
    }

    #[test]
    fn recalculate_state_partial_game_in_progress() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let outcome = setup.board.determine_outcome();
        assert_eq!(None, outcome);
    }

    #[test]
    fn recalculate_state_win_in_first_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[2] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_second_row() {
        let mut setup = TestSetup::new();

        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_third_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[6] = setup.occupied_cell_player2;
        setup.board.cells[7] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 2 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_first_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_second_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[7] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_third_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_left_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_win_in_right_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::WinnerFound { player_id: 1 }), outcome);
    }

    #[test]
    fn recalculate_state_draw() {
        let mut setup = TestSetup::new();
        setup.board.cells = [
            setup.occupied_cell_player1,
            setup.occupied_cell_player2,
            setup.occupied_cell_player1,
            setup.occupied_cell_player2,
            setup.occupied_cell_player1,
            setup.occupied_cell_player2,
            setup.occupied_cell_player2,
            setup.occupied_cell_player1,
            setup.occupied_cell_player2,
        ];

        let outcome = setup.board.determine_outcome();
        assert_eq!(Some(Outcome::Draw), outcome);
    }
}
