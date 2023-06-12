use std::ptr;
use std::sync::mpsc::{Receiver, Sender};

use crate::client;

pub const PLAYER_ONE_ID: u8 = 1;
pub const PLAYER_TWO_ID: u8 = 2;

pub struct Server<'a> {
    player_one: &'a Player<'a>,
    player_two: &'a Player<'a>,
    current_player: &'a Player<'a>,
    board: Board<'a>,
}

impl<'a> Server<'a> {
    pub fn new(player_one: &'a Player, player_two: &'a Player) -> Server<'a> {
        return Server {
            player_one,
            player_two,
            current_player: player_one,
            board: Board::new(),
        };
    }

    pub fn play_game(&'a mut self) {
        let (mut state, mut winning_player) = (State::InProgress, None);
        self.dispatch_state_changed_event(state, winning_player);

        while state == State::InProgress {
            self.dispatch_player_turn_event();

            let received_event = self.current_player.receiver.recv().unwrap();
            match self.handle_event(received_event) {
                Ok(_) => (),
                Err(e) => {
                    let _ = self.current_player.sender.send(Event::ErrorOccurred(e));
                    continue;
                }
            }

            (state, winning_player) = self.board.recalculate_state();
            self.dispatch_state_changed_event(state, winning_player);
            self.swap_player();
        }
    }

    fn swap_player(&mut self) {
        self.current_player = if self.current_player == self.player_one {
            self.player_two
        } else {
            self.player_one
        }
    }

    fn dispatch_event_to_all_clients(&self, event: Event) {
        if ptr::eq(self.player_one.sender, self.player_two.sender) {
            let _ = self.player_one.sender.send(event);
        } else {
            let event_copy = event;
            let _ = self.player_one.sender.send(event);
            let _ = self.player_two.sender.send(event_copy);
        }
    }

    fn dispatch_state_changed_event(&self, state: State, winning_player: Option<&Player>) {
        let cells_event_rep = self
            .board
            .cells
            .map(|cell| cell.occupying_player.map(|player| player.id));

        self.dispatch_event_to_all_clients(Event::StateChanged {
            state,
            board_cells: cells_event_rep,
            winning_player_id: winning_player.map(|player| player.id),
        })
    }

    fn dispatch_player_turn_event(&self) {
        let _ = self
            .current_player
            .sender
            .send(Event::PlayerTurn(self.current_player.id));
    }

    fn handle_event(&mut self, event: client::Event) -> Result<(), Error> {
        match event {
            client::Event::MoveMade {
                player_id,
                move_index,
            } => {
                let player = match player_id {
                    PLAYER_ONE_ID => &self.player_one,
                    PLAYER_TWO_ID => &self.player_two,
                    _ => panic!("Unexpected id provided"),
                };

                self.board.add_move(player, move_index)
            }
        }
    }
}

#[derive(Copy, Clone)]
pub enum Event {
    StateChanged {
        state: State,
        board_cells: [Option<u8>; BOARD_SIZE],
        winning_player_id: Option<u8>,
    },
    PlayerTurn(u8),
    ErrorOccurred(Error),
}

#[derive(Debug)]
pub struct Player<'a> {
    id: u8,
    sender: &'a Sender<Event>,
    receiver: &'a Receiver<client::Event>,
}

impl PartialEq for Player<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<'a> Player<'a> {
    pub fn new_player_one(
        sender: &'a Sender<Event>,
        receiver: &'a Receiver<client::Event>,
    ) -> Player<'a> {
        Player {
            id: PLAYER_ONE_ID,
            sender,
            receiver,
        }
    }

    pub fn new_player_two(
        sender: &'a Sender<Event>,
        receiver: &'a Receiver<client::Event>,
    ) -> Player<'a> {
        Player {
            id: PLAYER_TWO_ID,
            sender,
            receiver,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum BoardCellState {
    Empty,
    Occupied,
}

#[derive(Copy, Clone, PartialEq)]
struct BoardCell<'a> {
    state: BoardCellState,
    occupying_player: Option<&'a Player<'a>>,
}

impl BoardCell<'_> {
    fn new() -> BoardCell<'static> {
        BoardCell {
            state: BoardCellState::Empty,
            occupying_player: None,
        }
    }
}

pub const BOARD_SIZE: usize = 9;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum State {
    InProgress,
    Draw,
    WinnerFound,
}

struct Board<'a> {
    cells: [BoardCell<'a>; BOARD_SIZE],
}

impl<'a> Board<'a> {
    fn new() -> Board<'static> {
        Board {
            cells: [BoardCell::new(); BOARD_SIZE],
        }
    }

    fn add_move(&mut self, player: &'a Player, cell_index: usize) -> Result<(), Error> {
        if cell_index >= BOARD_SIZE {
            return Err(Error::InvalidCellIndex);
        }

        let cell = &mut self.cells[cell_index];
        match cell.state {
            BoardCellState::Empty => {
                cell.state = BoardCellState::Occupied;
                cell.occupying_player = Some(player);

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
    /// winning `Player` was identified.
    fn recalculate_state(&self) -> (State, Option<&Player>) {
        // If first cell is occupied, check for win in first row, column, and left diagonal
        if self.cells[0].state == BoardCellState::Occupied
            && ((self.cells[0] == self.cells[1] && self.cells[0] == self.cells[2])
                || (self.cells[0] == self.cells[3] && self.cells[0] == self.cells[6])
                || (self.cells[0] == self.cells[4] && self.cells[0] == self.cells[8]))
        {
            return (State::WinnerFound, self.cells[0].occupying_player);
        }

        // Check for win in second column
        if self.cells[1].state == BoardCellState::Occupied
            && self.cells[1] == self.cells[4]
            && self.cells[1] == self.cells[7]
        {
            return (State::WinnerFound, self.cells[1].occupying_player);
        }

        // Check for win in third column and right diagonal
        if self.cells[2].state == BoardCellState::Occupied
            && ((self.cells[2] == self.cells[5] && self.cells[2] == self.cells[8])
                || (self.cells[2] == self.cells[4] && self.cells[2] == self.cells[6]))
        {
            return (State::WinnerFound, self.cells[2].occupying_player);
        }

        // Check for win in second row
        if self.cells[3].state == BoardCellState::Occupied
            && self.cells[3] == self.cells[4]
            && self.cells[3] == self.cells[5]
        {
            return (State::WinnerFound, self.cells[3].occupying_player);
        }

        // Check for win in third row
        if self.cells[6].state == BoardCellState::Occupied
            && self.cells[6] == self.cells[7]
            && self.cells[6] == self.cells[8]
        {
            return (State::WinnerFound, self.cells[6].occupying_player);
        }

        // Check for draw
        if self
            .cells
            .iter()
            .all(|cell| cell.state == BoardCellState::Occupied)
        {
            return (State::Draw, None);
        }

        (State::InProgress, None)
    }
}

#[derive(Copy, Clone)]
pub enum Error {
    InvalidCellIndex,
    CellOccupied,
}

impl Error {
    pub fn to_user_message(self) -> String {
        match self {
            Error::InvalidCellIndex => format!(
                "The input should be a number between 0 and {}, try again.",
                BOARD_SIZE - 1
            ),
            Error::CellOccupied => "This cell is already occupied, try again.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver, Sender};

    use crate::client;

    struct TestSetup<'a> {
        occupied_cell_player1: BoardCell<'a>,
        occupied_cell_player2: BoardCell<'a>,
        board: Board<'a>,
    }

    impl<'a> TestSetup<'a> {
        fn new(player1: &'a Player, player2: &'a Player) -> TestSetup<'a> {
            TestSetup {
                occupied_cell_player1: BoardCell {
                    state: BoardCellState::Occupied,
                    occupying_player: Some(player1),
                },
                occupied_cell_player2: BoardCell {
                    state: BoardCellState::Occupied,
                    occupying_player: Some(player2),
                },
                board: Board::new(),
            }
        }
    }

    fn sender_receiver_setup() -> (Sender<self::Event>, Receiver<client::Event>) {
        let (server_tx, _client_rx) = mpsc::channel();
        let (client_tx, server_rx) = mpsc::channel();

        let _ = server_tx.send(Event::ErrorOccurred(Error::InvalidCellIndex));
        let _ = client_tx.send(client::Event::MoveMade {
            player_id: 1,
            move_index: 1,
        });

        (server_tx, server_rx)
    }

    fn player_setup<'a>(
        sender: &'a Sender<self::Event>,
        receiver: &'a Receiver<client::Event>,
    ) -> (Player<'a>, Player<'a>) {
        (
            Player {
                id: 1,
                sender,
                receiver,
            },
            Player {
                id: 0,
                sender,
                receiver,
            },
        )
    }

    #[test]
    fn recalculate_state_empty_board_in_progress() {
        let board = Board::new();
        let (state, winner) = board.recalculate_state();

        assert_eq!(State::InProgress, state);
        assert_eq!(None, winner);
    }

    #[test]
    fn recalculate_state_partial_game_in_progress() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::InProgress, state);
        assert_eq!(None, winner);
    }

    #[test]
    fn recalculate_state_win_in_first_row() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[2] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_second_row() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_third_row() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[6] = setup.occupied_cell_player2;
        setup.board.cells[7] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player2), winner);
    }

    #[test]
    fn recalculate_state_win_in_first_column() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_second_column() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[7] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_third_column() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_left_diagonal() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_win_in_right_diagonal() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        let (state, winner) = setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, state);
        assert_eq!(Some(&player1), winner);
    }

    #[test]
    fn recalculate_state_draw() {
        let (sender, receiver) = sender_receiver_setup();
        let (player1, player2) = player_setup(&sender, &receiver);
        let mut setup = TestSetup::new(&player1, &player2);

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
        assert_eq!(State::Draw, state);
        assert_eq!(None, winner);
    }
}
