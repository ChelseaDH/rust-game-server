use crate::tic_tac_toe::{Error, Outcome};

pub const BOARD_SIZE: usize = 9;

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

pub struct Board {
    cells: [BoardCell; BOARD_SIZE],
}

impl Board {
    pub(crate) fn new() -> Board {
        Board {
            cells: [BoardCell::new(); BOARD_SIZE],
        }
    }

    pub(crate) fn get_cell_occupiers(&self) -> [Option<u8>; BOARD_SIZE] {
        self.cells.map(|cell| match cell.state {
            BoardCellState::Empty => None,
            BoardCellState::Occupied { player_id } => Some(player_id),
        })
    }

    pub(crate) fn add_move(&mut self, player_id: u8, cell_index: usize) -> Result<(), Error> {
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
    pub(crate) fn determine_outcome(&self) -> Option<Outcome> {
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
