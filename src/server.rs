use std::fmt;
use std::fmt::Formatter;

use serde::{Deserialize, Serialize};
use thiserror::__private::DisplayAsDisplay;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{client, connection::Connection};
use crate::connection::ReadWriteError;
use crate::server::Outcome::{Draw, WinnerFound};

pub const PLAYER_ONE_ID: u8 = 1;
pub const PLAYER_TWO_ID: u8 = 2;

pub struct Server {
    player_one: Player,
    player_two: Player,
    board: Board,
    state: State,
    channel: (Sender<ServerEvent>, Receiver<ServerEvent>),
}

#[derive(Copy, Clone)]
enum State {
    PreInitialise,
    PlayerTurn { player_id: u8 },
    GameOver { outcome: Outcome },
    Failure,
}

impl Server {
    pub fn new(player_one: Player, player_two: Player) -> Server {
        Server {
            player_one,
            player_two,
            board: Board::new(),
            state: State::PreInitialise,
            channel: mpsc::channel(1),
        }
    }

    pub async fn init(&mut self) {
        &mut self.channel.0.send(ServerEvent::BeginGame).await.unwrap();
        self.run().await
    }

    async fn run(&mut self) {
        loop {
            let event = tokio::select! {
                result = self.channel.1.recv() => IncomingEvent::Server(result.unwrap()),
                result = self.player_one.connection.read_event() => IncomingEvent::Client(result.unwrap()),
                result = self.player_two.connection.read_event() => IncomingEvent::Client(result.unwrap()),
                else => break,
            };

            self.handle_incoming_event(event).await.unwrap()
        }
    }

    async fn handle_incoming_event(&mut self, event: IncomingEvent) -> Result<(), ServerError> {
        match (self.state, event) {
            (State::PreInitialise, IncomingEvent::Server(ServerEvent::BeginGame)) => {
                self.dispatch_state_changed_event(None).await?;
                self.state = State::PlayerTurn { player_id: PLAYER_ONE_ID };
                self.dispatch_player_turn_event().await?;

                Ok(())
            }
            (State::PlayerTurn { player_id: current_player_id }, IncomingEvent::Client(client::Event::MoveMade { player_id, move_index })) => {
                if player_id != current_player_id {
                    return Err(ServerError::Server(Error::UnexpectedPlayer));
                }

                match self.board.add_move(player_id, move_index) {
                    Err(e) => {
                        self.dispatch_event_to_current_player(Event::ErrorOccurred(e)).await?
                    },
                    Ok(()) => {
                        let outcome = self.board.calculate_outcome();
                        self.dispatch_state_changed_event(outcome).await?;

                        match outcome {
                            None => {
                                self.state = State::PlayerTurn { player_id: self.get_next_player(current_player_id) };
                                self.dispatch_player_turn_event().await?;
                            },
                            Some(oc) => {
                                self.state = State::GameOver { outcome: oc }
                            }
                        }
                    }
                }

                Ok(())
            }
            _ => Ok(())
        }
    }

    // pub async fn play_game(&mut self) {
    //     let (mut state, mut winning_player) = (BoardState::InProgress, None);
    //     self.dispatch_state_changed_event(state, winning_player).await;
    //
    //     while state == BoardState::InProgress {
    //         self.dispatch_player_turn_event().await;
    //
    //         let received_event = self.read_event_from_current_player().await;
    //         match self.handle_event(received_event) {
    //             Ok(_) => (),
    //             Err(e) => {
    //                 self.dispatch_event_to_current_player(Event::ErrorOccurred(e)).await;
    //                 continue;
    //             }
    //         }
    //
    //         (state, winning_player) = self.board.recalculate_state();
    //         self.dispatch_state_changed_event(state, winning_player).await;
    //         self.swap_player();
    //     }
    // }

    fn get_next_player(&mut self, current_player_id: u8) -> u8 {
        if current_player_id == PLAYER_ONE_ID {
            PLAYER_TWO_ID
        } else {
            PLAYER_ONE_ID
        }
    }

    async fn dispatch_event_to_all_players(&mut self, event: Event) -> Result<(), ReadWriteError> {
        if self.player_one.connection.id == self.player_two.connection.id {
            self.player_one.connection.write_event(event).await
        } else {
            let event_copy = event;
            self.player_one.connection.write_event(event).await?;
            self.player_two.connection.write_event(event_copy).await
        }
    }

    async fn dispatch_event_to_current_player(&mut self, event: Event) -> Result<(), ReadWriteError> {
        match self.current_player {
            PLAYER_ONE_ID => self.player_one.connection.write_event(event).await,
            PLAYER_TWO_ID => self.player_two.connection.write_event(event).await,
            _ => panic!("Unexpected id provided"),
        }
    }

    // async fn read_event_from_current_player(&mut self) -> client::Event {
    //     match self.current_player {
    //         PLAYER_ONE_ID => self.player_one.connection.read_event().await.unwrap(),
    //         PLAYER_TWO_ID => self.player_two.connection.read_event().await.unwrap(),
    //         _ => panic!("Unexpected id provided"),
    //     }
    // }

    async fn dispatch_state_changed_event(&mut self, outcome: Option<Outcome>) -> Result<(), ReadWriteError> {
        let cells_event_rep = self
            .board
            .cells
            .map(|cell| cell.occupying_player_id);

        self.dispatch_event_to_all_players(Event::StateChanged {
            board_cells: cells_event_rep,
            outcome,
        }).await
    }

    async fn dispatch_player_turn_event(&mut self) -> Result<(), ReadWriteError> {
        self.dispatch_event_to_current_player(Event::PlayerTurn(self.current_player)).await
    }

    fn handle_event(&mut self, event: client::Event) -> Result<(), Error> {
        match event {
            client::Event::MoveMade {
                player_id,
                move_index,
            } => {
                if player_id != self.current_player {
                    return Err(Error::UnexpectedPlayer);
                }

                self.board.add_move(player_id, move_index)
            }
        }
    }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum Event {
    StateChanged {
        board_cells: [Option<u8>; BOARD_SIZE],
        outcome: Option<Outcome>,
    },
    PlayerTurn(u8),
    ErrorOccurred(Error),
}

#[derive(Debug, Deserialize)]
enum ServerEvent {
    BeginGame,
    PlayerDisconnected,
}

#[derive(Deserialize)]
enum IncomingEvent {
    Server(ServerEvent),
    Client(client::Event),
}

#[derive(Debug)]
enum ServerError {
    ReadWrite(ReadWriteError),
    Server(Error),
}

impl From<ReadWriteError> for ServerError {
    fn from(value: ReadWriteError) -> Self {
        ServerError::ReadWrite(value)
    }
}

impl From<Error> for ServerError {
    fn from(value: Error) -> Self {
        ServerError::Server(value)
    }
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
    pub fn new_player_one(
        connection: Connection,
    ) -> Player {
        Player {
            id: PLAYER_ONE_ID,
            connection,
        }
    }

    pub fn new_player_two(
        connection: Connection,
    ) -> Player {
        Player {
            id: PLAYER_TWO_ID,
            connection,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum BoardCellState {
    Empty,
    Occupied,
}

#[derive(Copy, Clone, PartialEq)]
struct BoardCell {
    state: BoardCellState,
    occupying_player_id: Option<u8>,
}

impl BoardCell {
    fn new() -> BoardCell {
        BoardCell {
            state: BoardCellState::Empty,
            occupying_player_id: None,
        }
    }
}

pub const BOARD_SIZE: usize = 9;

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum BoardState {
    InProgress,
    Draw,
    WinnerFound,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Draw,
    WinnerFound { winning_player_id: u8 },
}

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
                cell.state = BoardCellState::Occupied;
                cell.occupying_player_id = Some(player_id);

                Ok(())
            }
            BoardCellState::Occupied => Err(Error::CellOccupied),
        }
    }

    /// Calculates the current state of the Board
    ///
    /// A win state is determined by looking in each row, column, and diagonal to see if the same
    /// player occupies each, and a draw state is determined by the board being fully occupied
    /// with no winners.
    ///
    /// # Returns
    ///
    /// A `tuple` containing the calculated `State` and an `Option` representing whether or not a
    /// winning `Player` was identified using their id.
    fn recalculate_state(&self) -> (BoardState, Option<u8>) {
        // If first cell is occupied, check for win in first row, column, and left diagonal
        if self.cells[0].state == BoardCellState::Occupied
            && ((self.cells[0] == self.cells[1] && self.cells[0] == self.cells[2])
            || (self.cells[0] == self.cells[3] && self.cells[0] == self.cells[6])
            || (self.cells[0] == self.cells[4] && self.cells[0] == self.cells[8]))
        {
            return (BoardState::WinnerFound, self.cells[0].occupying_player_id);
        }

        // Check for win in second column
        if self.cells[1].state == BoardCellState::Occupied
            && self.cells[1] == self.cells[4]
            && self.cells[1] == self.cells[7]
        {
            return (BoardState::WinnerFound, self.cells[1].occupying_player_id);
        }

        // Check for win in third column and right diagonal
        if self.cells[2].state == BoardCellState::Occupied
            && ((self.cells[2] == self.cells[5] && self.cells[2] == self.cells[8])
                || (self.cells[2] == self.cells[4] && self.cells[2] == self.cells[6]))
        {
            return (BoardState::WinnerFound, self.cells[2].occupying_player_id);
        }

        // Check for win in second row
        if self.cells[3].state == BoardCellState::Occupied
            && self.cells[3] == self.cells[4]
            && self.cells[3] == self.cells[5]
        {
            return (BoardState::WinnerFound, self.cells[3].occupying_player_id);
        }

        // Check for win in third row
        if self.cells[6].state == BoardCellState::Occupied
            && self.cells[6] == self.cells[7]
            && self.cells[6] == self.cells[8]
        {
            return (BoardState::WinnerFound, self.cells[6].occupying_player_id);
        }

        // Check for draw
        if self
            .cells
            .iter()
            .all(|cell| cell.state == BoardCellState::Occupied)
        {
            return (BoardState::Draw, None);
        }

        (BoardState::InProgress, None)
    }

    fn calculate_outcome(&self) -> Option<Outcome> {
        // If first cell is occupied, check for win in first row, column, and left diagonal
        if self.cells[0].state == BoardCellState::Occupied
            && ((self.cells[0] == self.cells[1] && self.cells[0] == self.cells[2])
            || (self.cells[0] == self.cells[3] && self.cells[0] == self.cells[6])
            || (self.cells[0] == self.cells[4] && self.cells[0] == self.cells[8]))
        {
            return Some(WinnerFound { winning_player_id: self.cells[0].occupying_player_id.unwrap() })
        }

        // Check for win in second column
        if self.cells[1].state == BoardCellState::Occupied
            && self.cells[1] == self.cells[4]
            && self.cells[1] == self.cells[7]
        {
            return return Some(WinnerFound { winning_player_id: self.cells[1].occupying_player_id.unwrap() })
        }

        // Check for win in third column and right diagonal
        if self.cells[2].state == BoardCellState::Occupied
            && ((self.cells[2] == self.cells[5] && self.cells[2] == self.cells[8])
            || (self.cells[2] == self.cells[4] && self.cells[2] == self.cells[6]))
        {
            return return Some(WinnerFound { winning_player_id: self.cells[2].occupying_player_id.unwrap() })
        }

        // Check for win in second row
        if self.cells[3].state == BoardCellState::Occupied
            && self.cells[3] == self.cells[4]
            && self.cells[3] == self.cells[5]
        {
            return return Some(WinnerFound { winning_player_id: self.cells[3].occupying_player_id.unwrap() })
        }

        // Check for win in third row
        if self.cells[6].state == BoardCellState::Occupied
            && self.cells[6] == self.cells[7]
            && self.cells[6] == self.cells[8]
        {
            return Some(WinnerFound { winning_player_id: self.cells[6].occupying_player_id.unwrap() })
        }

        // Check for draw
        if self
            .cells
            .iter()
            .all(|cell| cell.state == BoardCellState::Occupied)
        {
            return Some(Draw);
        }

        None
    }
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_user_message())
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
                    state: BoardCellState::Occupied,
                    occupying_player_id: Some(1),
                },
                occupied_cell_player2: BoardCell {
                    state: BoardCellState::Occupied,
                    occupying_player_id: Some(2),
                },
                board: Board::new(),
            }
        }
    }

    #[test]
    fn recalculate_state_empty_board_in_progress() {
        let board = Board::new();
        let (state, winner) = board.recalculate_state();

        assert_eq!(BoardState::InProgress, state);
        assert_eq!(None, winner);
    }

    #[test]
    fn recalculate_state_partial_game_in_progress() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::InProgress, state);
        assert_eq!(None, winner);
    }

    #[test]
    fn recalculate_state_win_in_first_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[2] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_second_row() {
        let mut setup = TestSetup::new();

        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_third_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[6] = setup.occupied_cell_player2;
        setup.board.cells[7] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(2), winner);
    }

    #[test]
    fn recalculate_state_win_in_first_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_second_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[7] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_third_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_left_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
    }

    #[test]
    fn recalculate_state_win_in_right_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::WinnerFound, state);
        assert_eq!(Some(1), winner);
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

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(BoardState::Draw, state);
        assert_eq!(None, winner);
    }
}
