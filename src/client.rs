use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::server::Outcome;
use crate::{connection::Connection, server};

pub trait ClientType {}

pub struct LocalClient {}
impl ClientType for LocalClient {}

pub struct OnlineClient {
    id: u8,
}
impl ClientType for OnlineClient {}

pub struct Client<T: ClientType> {
    client_type: T,
    connection: Connection,
    running: bool,
}

impl Client<LocalClient> {
    pub fn new(connection: Connection) -> Self {
        Client {
            client_type: LocalClient {},
            connection,
            running: true,
        }
    }
}

impl Client<OnlineClient> {
    pub fn new(connection: Connection, id: u8) -> Self {
        Client {
            client_type: OnlineClient { id },
            connection,
            running: true,
        }
    }
}

#[async_trait]
pub trait ClientGameMode {
    async fn handle_game_started_event(&mut self);
    async fn handle_player_turn_event(&mut self, player_id: u8);
}

#[async_trait]
impl ClientGameMode for Client<LocalClient> {
    async fn handle_game_started_event(&mut self) {
        println!("Lets begin.")
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        println!("Player {}'s turn!", self.get_player_icon_by_id(player_id));
        self.make_player_move(player_id).await;
    }
}

#[async_trait]
impl ClientGameMode for Client<OnlineClient> {
    async fn handle_game_started_event(&mut self) {
        println!("All players connected, lets begin.")
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        if player_id != self.client_type.id {
            println!("Waiting for other player to make a move.");
            return;
        }

        println!("It's your turn!");
        self.make_player_move(player_id).await;
    }
}

impl<T: ClientType> Client<T>
where
    Self: ClientGameMode,
{
    pub async fn play_game(&mut self) {
        while self.running {
            match self.connection.read_event().await {
                Ok(event) => self.handle_event(event).await,
                Err(error) => {
                    eprintln!("Error: {}\n", error);
                    continue;
                }
            }
        }
    }

    pub async fn handle_event(&mut self, event: server::Event) {
        match event {
            server::Event::BoardUpdated { board_cells } => self.print_board(board_cells),
            server::Event::GameOver { outcome } => {
                match outcome {
                    Outcome::Draw => {
                        println!("Game over! There was a draw!")
                    }
                    Outcome::WinnerFound { player_id } => println!(
                        "Game over! Player {} won!",
                        self.get_player_icon_by_id(player_id)
                    ),
                }
                self.running = false
            }
            server::Event::PlayerTurn(id) => self.handle_player_turn_event(id).await,
            server::Event::ErrorOccurred(error) => self.handle_error(error),
            server::Event::GameStarted => self.handle_game_started_event().await,
        }
    }

    fn get_player_icon_by_id(&self, id: u8) -> char {
        match id {
            server::PLAYER_ONE_ID => 'X',
            server::PLAYER_TWO_ID => 'O',
            _ => panic!("Unexpected id provided"),
        }
    }

    fn get_optional_player_icon_by_id(&self, player_id: Option<u8>) -> char {
        match player_id {
            None => ' ',
            Some(i) => self.get_player_icon_by_id(i),
        }
    }

    async fn make_player_move(&mut self, player_id: u8) {
        let move_index = get_move();
        let _ = self
            .connection
            .write_event(Event::MoveMade {
                player_id,
                move_index,
            })
            .await;
    }

    fn handle_error(&self, error: server::Error) {
        println!("Error: {}\n", error.to_user_message())
    }

    fn print_board(&self, board_cells: [Option<u8>; server::BOARD_SIZE]) {
        println!("_________");
        println!(
            "{} | {} | {}",
            self.get_optional_player_icon_by_id(board_cells[0]),
            self.get_optional_player_icon_by_id(board_cells[1]),
            self.get_optional_player_icon_by_id(board_cells[2])
        );
        println!(
            "{} | {} | {}",
            self.get_optional_player_icon_by_id(board_cells[3]),
            self.get_optional_player_icon_by_id(board_cells[4]),
            self.get_optional_player_icon_by_id(board_cells[5])
        );
        println!(
            "{} | {} | {}",
            self.get_optional_player_icon_by_id(board_cells[6]),
            self.get_optional_player_icon_by_id(board_cells[7]),
            self.get_optional_player_icon_by_id(board_cells[8])
        );
        println!("_________");
        println!();
    }
}

#[derive(Deserialize, Serialize)]
pub enum Event {
    MoveMade { player_id: u8, move_index: usize },
}

fn get_move() -> usize {
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
