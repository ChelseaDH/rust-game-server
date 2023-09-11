use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io;

use crate::server::Outcome;
use crate::{connection::Connection, server};

pub trait ClientType {}

pub struct LocalClient {}
impl ClientType for LocalClient {}

pub struct OnlineClient {
    id: u8,
}
impl ClientType for OnlineClient {}

pub struct Client<T: ClientType, I: io::BufRead + Send, O: io::Write + Send> {
    client_type: T,
    connection: Connection,
    running: bool,
    input: I,
    output: O,
}

impl<I: io::BufRead + Send, O: io::Write + Send> Client<LocalClient, I, O> {
    pub fn new(connection: Connection, input: I, output: O) -> Self {
        Client {
            client_type: LocalClient {},
            connection,
            running: true,
            input,
            output,
        }
    }
}

impl<I: io::BufRead + Send, O: io::Write + Send> Client<OnlineClient, I, O> {
    pub fn new(connection: Connection, id: u8, input: I, output: O) -> Self {
        Client {
            client_type: OnlineClient { id },
            connection,
            running: true,
            input,
            output,
        }
    }
}

#[async_trait]
pub trait ClientGameMode {
    async fn handle_game_started_event(&mut self);
    async fn handle_player_turn_event(&mut self, player_id: u8);
}

#[async_trait]
impl<I: io::BufRead + Send, O: io::Write + Send> ClientGameMode for Client<LocalClient, I, O> {
    async fn handle_game_started_event(&mut self) {
        writeln!(&mut self.output, "Lets begin.").unwrap();
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        let player_icon = self.get_player_icon_by_id(player_id);
        writeln!(&mut self.output, "Player {}'s turn!", player_icon).unwrap();
        self.make_player_move(player_id).await;
    }
}

#[async_trait]
impl<I: io::BufRead + Send, O: io::Write + Send> ClientGameMode for Client<OnlineClient, I, O> {
    async fn handle_game_started_event(&mut self) {
        writeln!(&mut self.output, "All players connected, lets begin.").unwrap();
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        if player_id != self.client_type.id {
            writeln!(&mut self.output, "Waiting for other player to make a move.").unwrap();
            return;
        }

        writeln!(&mut self.output, "It's your turn!").unwrap();
        self.make_player_move(player_id).await;
    }
}

impl<T: ClientType, I: io::BufRead + Send, O: io::Write + Send> Client<T, I, O>
where
    Self: ClientGameMode,
{
    pub async fn play_game(&mut self) {
        while self.running {
            match self.connection.read_event().await {
                Ok(event) => self.handle_event(event).await,
                Err(_) => self.handle_shutdown().await,
            }
        }
    }

    pub async fn handle_event(&mut self, event: server::Event) {
        match event {
            server::Event::BoardUpdated { board_cells } => self.print_board(board_cells),
            server::Event::GameOver { outcome } => {
                match outcome {
                    Outcome::Draw => {
                        writeln!(&mut self.output, "Game over! There was a draw!").unwrap();
                    }
                    Outcome::WinnerFound { player_id } => {
                        let player_icon = self.get_player_icon_by_id(player_id);
                        writeln!(&mut self.output, "Game over! Player {} won!", player_icon)
                            .unwrap()
                    }
                }
                self.running = false
            }
            server::Event::PlayerTurn(id) => self.handle_player_turn_event(id).await,
            server::Event::ErrorOccurred(error) => self.handle_error(error),
            server::Event::GameStarted => self.handle_game_started_event().await,
            server::Event::Shutdown => self.handle_shutdown().await,
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
        let move_index = get_move(&mut self.input, &mut self.output);
        let _ = self
            .connection
            .write_event(Event::MoveMade {
                player_id,
                move_index,
            })
            .await;
    }

    fn handle_error(&mut self, error: server::Error) {
        writeln!(&mut self.output, "Error: {}", error).unwrap();
    }

    async fn handle_shutdown(&mut self) {
        writeln!(
            &mut self.output,
            "An unrecoverable error has occurred, game terminating."
        )
        .unwrap();

        self.running = false;
        let _ = self.connection.shutdown().await;
    }

    fn print_board(&mut self, board_cells: [Option<u8>; server::BOARD_SIZE]) {
        let cell_icons = board_cells.map(|x| self.get_optional_player_icon_by_id(x));

        writeln!(&mut self.output, "_________").unwrap();
        writeln!(
            &mut self.output,
            "{} | {} | {}",
            cell_icons[0], cell_icons[1], cell_icons[2],
        )
        .unwrap();
        writeln!(
            &mut self.output,
            "{} | {} | {}",
            cell_icons[3], cell_icons[4], cell_icons[5],
        )
        .unwrap();
        writeln!(
            &mut self.output,
            "{} | {} | {}",
            cell_icons[6], cell_icons[7], cell_icons[8],
        )
        .unwrap();
        writeln!(&mut self.output, "_________").unwrap();
        writeln!(&mut self.output).unwrap();
    }
}

#[derive(Deserialize, Serialize)]
pub enum Event {
    MoveMade { player_id: u8, move_index: usize },
}

fn get_move(mut input: impl io::BufRead, mut output: impl io::Write) -> usize {
    loop {
        writeln!(
            output,
            "Input a number between 0 and {} to make your move:",
            server::BOARD_SIZE - 1
        )
        .unwrap();

        let input_text = &mut String::new();
        input.read_line(input_text).unwrap();

        match input_text.trim().parse::<usize>() {
            Err(_) => writeln!(output, "That is not a number, please try again.").unwrap(),
            Ok(index) => return index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tokio::net::{TcpListener, TcpStream};

    async fn get_test_stream() -> TcpStream {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        TcpStream::connect(local_addr).await.unwrap()
    }

    async fn get_local_test_client<'a>(
        input: &'a [u8],
        output: &'a mut Vec<u8>,
    ) -> Client<LocalClient, &'a [u8], &'a mut Vec<u8>> {
        Client::<LocalClient, &'a [u8], &'a mut Vec<u8>>::new(
            Connection::new(get_test_stream().await),
            input,
            output,
        )
    }

    async fn get_online_test_client<'a>(
        id: u8,
        input: &'a [u8],
        output: &'a mut Vec<u8>,
    ) -> Client<OnlineClient, &'a [u8], &'a mut Vec<u8>> {
        Client::<OnlineClient, &'a [u8], &'a mut Vec<u8>>::new(
            Connection::new(get_test_stream().await),
            id,
            input,
            output,
        )
    }

    #[tokio::test]
    async fn local_client_handles_game_started_event() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client.handle_game_started_event().await;
        assert_eq!(output, b"Lets begin.\n")
    }

    #[tokio::test]
    async fn local_client_handles_player_turn_event() {
        let mut output: Vec<u8> = Vec::new();
        let input = "1".as_bytes();
        let mut client = get_local_test_client(input, &mut output).await;

        client.handle_player_turn_event(1).await;
        assert_eq!(
            output,
            b"Player X's turn!\nInput a number between 0 and 8 to make your move:\n"
        )
    }

    #[tokio::test]
    async fn online_client_handles_game_started_event() {
        let mut output = Vec::new();
        let mut client = get_online_test_client(1, &[], &mut output).await;

        client.handle_game_started_event().await;
        assert_eq!(output, b"All players connected, lets begin.\n")
    }

    #[tokio::test]
    async fn online_client_handles_player_turn_event_when_not_their_turn() {
        let mut output = Vec::new();
        let mut client = get_online_test_client(1, &[], &mut output).await;

        client.handle_player_turn_event(2).await;
        assert_eq!(output, b"Waiting for other player to make a move.\n")
    }

    #[tokio::test]
    async fn online_client_handles_player_turn_event_when_their() {
        let mut output: Vec<u8> = Vec::new();
        let input = "1".as_bytes();
        let mut client = get_online_test_client(1, input, &mut output).await;

        client.handle_player_turn_event(1).await;
        assert_eq!(
            output,
            b"It's your turn!\nInput a number between 0 and 8 to make your move:\n"
        )
    }

    #[tokio::test]
    async fn generic_client_handles_board_updated_event() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        let board_cells = [
            None,
            Some(1),
            Some(1),
            Some(2),
            None,
            None,
            Some(2),
            None,
            Some(1),
        ];

        client
            .handle_event(server::Event::BoardUpdated { board_cells })
            .await;
        assert_eq!(
            output,
            b"_________\n  | X | X\nO |   |  \nO |   | X\n_________\n\n"
        )
    }

    #[tokio::test]
    async fn generic_client_handles_game_over_event_for_a_draw() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(server::Event::GameOver {
                outcome: Outcome::Draw,
            })
            .await;
        assert_eq!(client.running, false);
        assert_eq!(output, b"Game over! There was a draw!\n")
    }

    #[tokio::test]
    async fn generic_client_handles_game_over_event_for_player_one_winning() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(server::Event::GameOver {
                outcome: Outcome::WinnerFound { player_id: 1 },
            })
            .await;
        assert_eq!(client.running, false);
        assert_eq!(output, b"Game over! Player X won!\n")
    }

    #[tokio::test]
    async fn generic_client_handles_game_over_event_for_player_two_winning() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(server::Event::GameOver {
                outcome: Outcome::WinnerFound { player_id: 2 },
            })
            .await;
        assert_eq!(client.running, false);
        assert_eq!(output, b"Game over! Player O won!\n")
    }

    #[tokio::test]
    async fn generic_client_handle_event_handles_error_occurred_event() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(server::Event::ErrorOccurred {
                0: server::Error::CellOccupied,
            })
            .await;
        assert_eq!(output, b"Error: This cell is already occupied.\n")
    }

    #[tokio::test]
    async fn generic_client_handles_shutdown_event() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client.handle_event(server::Event::Shutdown).await;
        assert_eq!(client.running, false);
        assert_eq!(
            output,
            b"An unrecoverable error has occurred, game terminating.\n"
        )
    }

    #[test]
    fn get_move_handles_errors_until_valid_move_provided() {
        let mut output: Vec<u8> = Vec::new();
        let input = "not a number\n1".as_bytes();

        let result = get_move(input, &mut output);
        assert_eq!(result, 1);
        assert_eq!(output, b"Input a number between 0 and 8 to make your move:\nThat is not a number, please try again.\nInput a number between 0 and 8 to make your move:\n")
    }
}
