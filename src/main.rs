use std::ptr;

fn main() {
    let player_one = Player { icon: 'X' };
    let player_two = Player { icon: 'O' };

    let mut game = Game {
        player_one: &player_one,
        player_two: &player_two,
        current_player: &player_one,
        board: Board::new(),
    };

    println!("Hello from Rust Game Server, welcome to Tic Tac Toe!");
    game.board.print_board();
    println!();

    game.play();
}

struct Game<'a> {
    player_one: &'a Player,
    player_two: &'a Player,
    current_player: &'a Player,
    board: Board<'a>,
}

impl<'a> Game<'a> {
    fn play(&'a mut self) {
        loop {
            println!("Player {}'s turn!", self.current_player.icon);
            self.current_player.make_move(&mut self.board);

            self.board.print_board();

            self.board.recalculate_state();
            match self.board.state {
                State::InProgress => (),
                State::Draw => {
                    println!("The game is a draw!");
                    return;
                }
                State::WinnerFound => {
                    println!("Player {} wins!", self.current_player.icon);
                    return;
                }
            }

            self.swap_player()
        }
    }

    fn swap_player(&mut self) {
        self.current_player = if ptr::eq(self.current_player, self.player_one) {
            self.player_two
        } else {
            self.player_one
        }
    }
}

#[derive(PartialEq)]
struct Player {
    icon: char,
}

impl<'a> Player {
    fn make_move(&'a self, board: &mut Board<'a>) {
        loop {
            println!(
                "Input a number between 0 and {} to make your move:",
                BOARD_SIZE - 1
            );

            let input_text = &mut String::new();
            std::io::stdin()
                .read_line(input_text)
                .expect("Failed to read stdin");

            match input_text.trim().parse::<usize>() {
                Err(_) => println!("That is not a number, please try again.\n"),
                Ok(index) => match board.add_move(self, index) {
                    Ok(_) => return,
                    Err(error) => println!("{}\n", error.to_user_message()),
                },
            }
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
    occupying_player: Option<&'a Player>,
}

impl BoardCell<'_> {
    fn new() -> BoardCell<'static> {
        BoardCell {
            state: BoardCellState::Empty,
            occupying_player: None,
        }
    }

    fn get_icon(&self) -> char {
        match self.state {
            BoardCellState::Empty => ' ',
            BoardCellState::Occupied => match &self.occupying_player {
                Some(player) => player.icon,
                None => panic!("Occupied cell with no occupying player!"),
            },
        }
    }
}

const BOARD_SIZE: usize = 9;

#[derive(PartialEq, Debug)]
enum State {
    InProgress,
    Draw,
    WinnerFound,
}

struct Board<'a> {
    cells: [BoardCell<'a>; BOARD_SIZE],
    state: State,
}

impl<'a> Board<'a> {
    fn new() -> Board<'static> {
        Board {
            cells: [BoardCell::new(); BOARD_SIZE],
            state: State::InProgress,
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

    fn recalculate_state(&mut self) {
        // If the board is in a final state, don't recalculate
        if self.state != State::InProgress {
            return;
        }

        // If first cell is occupied, check for win in first row, column, and left diagonal
        if self.cells[0].state == BoardCellState::Occupied
            && ((self.cells[0] == self.cells[1] && self.cells[0] == self.cells[2])
                || (self.cells[0] == self.cells[3] && self.cells[0] == self.cells[6])
                || (self.cells[0] == self.cells[4] && self.cells[0] == self.cells[8]))
        {
            self.state = State::WinnerFound;
            return;
        }

        // Check for win in second column
        if self.cells[1].state == BoardCellState::Occupied
            && self.cells[1] == self.cells[4]
            && self.cells[1] == self.cells[7]
        {
            self.state = State::WinnerFound;
            return;
        }

        // Check for win in third column and right diagonal
        if self.cells[2].state == BoardCellState::Occupied
            && ((self.cells[2] == self.cells[5] && self.cells[2] == self.cells[8])
                || (self.cells[2] == self.cells[4] && self.cells[2] == self.cells[6]))
        {
            self.state = State::WinnerFound;
            return;
        }

        // Check for win in second row
        if self.cells[3].state == BoardCellState::Occupied
            && self.cells[3] == self.cells[4]
            && self.cells[3] == self.cells[5]
        {
            self.state = State::WinnerFound;
            return;
        }

        // Check for win in third row
        if self.cells[6].state == BoardCellState::Occupied
            && self.cells[6] == self.cells[7]
            && self.cells[6] == self.cells[8]
        {
            self.state = State::WinnerFound;
            return;
        }

        // Check for draw
        if self
            .cells
            .iter()
            .all(|cell| cell.state == BoardCellState::Occupied)
        {
            self.state = State::Draw;
        }
    }

    fn print_board(&self) {
        println!("_________");
        println!(
            "{} | {} | {}",
            self.cells[0].get_icon(),
            self.cells[1].get_icon(),
            self.cells[2].get_icon()
        );
        println!(
            "{} | {} | {}",
            self.cells[3].get_icon(),
            self.cells[4].get_icon(),
            self.cells[5].get_icon()
        );
        println!(
            "{} | {} | {}",
            self.cells[6].get_icon(),
            self.cells[7].get_icon(),
            self.cells[8].get_icon()
        );
        println!("_________");
    }
}

enum Error {
    InvalidCellIndex,
    CellOccupied,
}

impl Error {
    fn to_user_message(&self) -> String {
        match &self {
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

    struct TestSetup<'a> {
        occupied_cell_player1: BoardCell<'a>,
        occupied_cell_player2: BoardCell<'a>,
        board: Board<'a>,
    }

    impl<'a> TestSetup<'a> {
        fn new() -> TestSetup<'a> {
            TestSetup {
                occupied_cell_player1: BoardCell {
                    state: BoardCellState::Occupied,
                    occupying_player: Some(&Player { icon: 'X' }),
                },
                occupied_cell_player2: BoardCell {
                    state: BoardCellState::Occupied,
                    occupying_player: Some(&Player { icon: 'O' }),
                },
                board: Board::new(),
            }
        }
    }

    #[test]
    fn recalculate_state_empty_board_in_progress() {
        let mut board = Board::new();
        board.recalculate_state();

        assert_eq!(State::InProgress, board.state)
    }

    #[test]
    fn recalculate_state_partial_game_in_progress() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        setup.board.recalculate_state();
        assert_eq!(State::InProgress, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_first_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[2] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_second_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_third_row() {
        let mut setup = TestSetup::new();
        setup.board.cells[6] = setup.occupied_cell_player2;
        setup.board.cells[7] = setup.occupied_cell_player2;
        setup.board.cells[8] = setup.occupied_cell_player2;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_first_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[3] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_second_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[1] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[7] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_third_column() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[5] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_left_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[0] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[8] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
    }

    #[test]
    fn recalculate_state_win_in_right_diagonal() {
        let mut setup = TestSetup::new();
        setup.board.cells[2] = setup.occupied_cell_player1;
        setup.board.cells[4] = setup.occupied_cell_player1;
        setup.board.cells[6] = setup.occupied_cell_player1;

        setup.board.recalculate_state();
        assert_eq!(State::WinnerFound, setup.board.state)
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

        setup.board.recalculate_state();
        assert_eq!(State::Draw, setup.board.state)
    }
}
