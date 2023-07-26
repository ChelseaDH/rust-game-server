use serde::{Deserialize, Serialize};

use crate::{client, connection::Connection};
use async_trait::async_trait;
use tokio::sync::mpsc::{self, Receiver, Sender};

pub const PLAYER_ONE_ID: u8 = 1;
pub const PLAYER_TWO_ID: u8 = 2;

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum State {
    PreInitialise,
    PlayerTurn,
    GameOver(Outcome),
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
    async fn get_next_incoming_event(&mut self) -> Option<IncomingEvent>;
    async fn dispatch_event_to_current_player(&mut self, event: Event);
    async fn dispatch_event_to_all_players(&mut self, event: Event);
}

#[async_trait]
impl ServerGameMode for Server<LocalConnection> {
    async fn get_next_incoming_event(&mut self) -> Option<IncomingEvent> {
        return tokio::select! {
            result = self.channel.1.recv() => Some(IncomingEvent::Server(result.unwrap())),
            result = self.connection.connection.read_event() => Some(IncomingEvent::Client(result.unwrap())),
            else => None
        };
    }

    async fn dispatch_event_to_current_player(&mut self, event: Event) {
        self.connection.connection.write_event(event).await.unwrap()
    }

    async fn dispatch_event_to_all_players(&mut self, event: Event) {
        self.connection.connection.write_event(event).await.unwrap()
    }
}

#[async_trait]
impl ServerGameMode for Server<OnlineConnection> {
    async fn get_next_incoming_event(&mut self) -> Option<IncomingEvent> {
        return tokio::select! {
            result = self.channel.1.recv() => Some(IncomingEvent::Server(result.unwrap())),
            result = self.connection.player_one.connection.read_event() => Some(IncomingEvent::Client(result.unwrap())),
            result = self.connection.player_two.connection.read_event() => Some(IncomingEvent::Client(result.unwrap())),
            else => None
        };
    }

    async fn dispatch_event_to_current_player(&mut self, event: Event) {
        match self.current_player {
            PLAYER_ONE_ID => self
                .connection
                .player_one
                .connection
                .write_event(event)
                .await
                .unwrap(),
            PLAYER_TWO_ID => self
                .connection
                .player_two
                .connection
                .write_event(event)
                .await
                .unwrap(),
            _ => panic!("Unexpected id provided"),
        };
    }

    async fn dispatch_event_to_all_players(&mut self, event: Event) {
        let event_copy = event;
        self.connection
            .player_one
            .connection
            .write_event(event)
            .await
            .unwrap();
        self.connection
            .player_two
            .connection
            .write_event(event_copy)
            .await
            .unwrap();
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
                _ => match self.get_next_incoming_event().await {
                    Some(event) => self.handle_incoming_event(event).await,
                    None => break,
                },
            }
        }
    }

    async fn handle_incoming_event(&mut self, event: IncomingEvent) {
        match (self.state, event) {
            (State::PreInitialise, IncomingEvent::Server(ServerEvent::BeginGame)) => {
                self.dispatch_board_updated_event().await;
                self.dispatch_player_turn_event().await;

                self.state = State::PlayerTurn;
            }
            (
                State::PlayerTurn,
                IncomingEvent::Client(client::Event::MoveMade {
                    player_id,
                    move_index,
                }),
            ) => {
                if let Err(error) = self.handle_move_made_event(player_id, move_index) {
                    self.dispatch_event_to_current_player(Event::ErrorOccurred(error))
                        .await;
                    self.dispatch_player_turn_event().await;
                    return;
                };

                self.dispatch_board_updated_event().await;
                match self.board.determine_outcome() {
                    None => {
                        self.swap_player();
                        self.dispatch_player_turn_event().await
                    }
                    Some(outcome) => {
                        self.state = State::GameOver(outcome);
                        self.dispatch_event_to_all_players(Event::GameOver { outcome })
                            .await;
                    }
                }
            }
            _ => panic!("Invalid state for event"),
        }
    }

    fn swap_player(&mut self) {
        self.current_player = if self.current_player == PLAYER_ONE_ID {
            PLAYER_TWO_ID
        } else {
            PLAYER_ONE_ID
        }
    }

    async fn dispatch_board_updated_event(&mut self) {
        let board_cells = self.board.cells.map(|cell| match cell.state {
            BoardCellState::Empty => None,
            BoardCellState::Occupied { player_id } => Some(player_id),
        });

        self.dispatch_event_to_all_players(Event::BoardUpdated { board_cells })
            .await;
    }

    async fn dispatch_player_turn_event(&mut self) {
        self.dispatch_event_to_current_player(Event::PlayerTurn(self.current_player))
            .await;
    }

    fn handle_move_made_event(&mut self, player_id: u8, move_index: usize) -> Result<(), Error> {
        if player_id != self.current_player {
            return Err(Error::UnexpectedPlayer);
        }

        self.board.add_move(player_id, move_index)
    }
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

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum Error {
    InvalidCellIndex,
    CellOccupied,
    UnexpectedPlayer,
}

impl Error {
    pub fn to_user_message(self) -> String {
        match self {
            Error::InvalidCellIndex => format!(
                "The input should be a number between 0 and {}, try again.",
                BOARD_SIZE - 1
            ),
            Error::CellOccupied => "This cell is already occupied, try again.".to_string(),
            Error::UnexpectedPlayer => "It's not your turn, try again.".to_string(),
        }
    }
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
