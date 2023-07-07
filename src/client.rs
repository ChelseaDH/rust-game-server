use serde::{Deserialize, Serialize};

use crate::{connection::Connection, server};

pub struct Client {
    player_one: Player,
    player_two: Player,
    connection: Connection,
}

impl Client {
    pub fn new(connection: Connection) -> Client {
        Client {
            player_one: Player { icon: 'X' },
            player_two: Player { icon: 'O' },
            connection,
        }
    }

    pub async fn handle_event(&mut self, event: server::Event) {
        match event {
            server::Event::StateChanged {
                state,
                board_cells,
                winning_player_id,
            } => {
                self.print_board(board_cells);
                match state {
                    server::State::InProgress => (),
                    server::State::Draw => println!("Game over! There was a draw!"),
                    server::State::WinnerFound => match winning_player_id {
                        None => println!("Game over! Someone won!"),
                        Some(id) => {
                            println!("Game over! Player {} won!", self.get_player_by_id(id).icon)
                        }
                    },
                }
            }
            server::Event::PlayerTurn(id) => self.make_player_move(id).await,
            server::Event::ErrorOccurred(error) => self.handle_error(error),
        }
    }

    fn get_player_by_id(&self, id: u8) -> &Player {
        match id {
            server::PLAYER_ONE_ID => &self.player_one,
            server::PLAYER_TWO_ID => &self.player_two,
            _ => panic!("Unexpected id provided"),
        }
    }

    fn get_player_icon_by_id(&self, player_id: Option<u8>) -> char {
        match player_id {
            None => ' ',
            Some(i) => self.get_player_by_id(i).icon,
        }
    }

    async fn make_player_move(&mut self, player_id: u8) {
        let player = self.get_player_by_id(player_id);
        println!("Player {}'s turn!", player.icon);

        let move_index = player.get_move();
        let _ = self.connection.write_event(Event::MoveMade {
            player_id,
            move_index,
        }).await;
    }

    pub async fn play_game(&mut self) {
        loop {
            match self.connection.read_event().await {
                Ok(event) => self.handle_event(event).await,
                Err(error) => {
                    eprintln!("Error: {}\n", error);
                    continue;
                }
            }
        }
    }

    fn handle_error(&self, error: server::Error) {
        println!("Error: {}\n", error.to_user_message())
    }

    fn print_board(&self, board_cells: [Option<u8>; server::BOARD_SIZE]) {
        println!("_________");
        println!(
            "{} | {} | {}",
            self.get_player_icon_by_id(board_cells[0]),
            self.get_player_icon_by_id(board_cells[1]),
            self.get_player_icon_by_id(board_cells[2])
        );
        println!(
            "{} | {} | {}",
            self.get_player_icon_by_id(board_cells[3]),
            self.get_player_icon_by_id(board_cells[4]),
            self.get_player_icon_by_id(board_cells[5])
        );
        println!(
            "{} | {} | {}",
            self.get_player_icon_by_id(board_cells[6]),
            self.get_player_icon_by_id(board_cells[7]),
            self.get_player_icon_by_id(board_cells[8])
        );
        println!("_________");
    }
}

#[derive(Deserialize, Serialize)]
pub enum Event {
    MoveMade { player_id: u8, move_index: usize },
}

#[derive(PartialEq)]
struct Player {
    icon: char,
}

impl Player {
    fn get_move(&self) -> usize {
        loop {
            println!(
                "Input a number between 0 and {} to make your move:",
                server::BOARD_SIZE - 1
            );

            let input_text = &mut String::new();
            std::io::stdin()
                .read_line(input_text)
                .expect("Failed to read stdin");

            match input_text.trim().parse::<usize>() {
                Err(_) => println!("That is not a number, please try again.\n"),
                Ok(index) => return index,
            };
        }
    }
}
