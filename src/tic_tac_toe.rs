use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io;
use std::string::String;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

use crate::client::{ClientType, LocalClient, OnlineClient};
use crate::game::{GameClient, GameClientEvent, GameServer, GameServerEvent};
use crate::server::{get_alternative_player_id, DispatchMode, PLAYER_ONE_ID, PLAYER_TWO_ID};
use crate::tic_tac_toe::board::Board;
pub use crate::tic_tac_toe::board::BOARD_SIZE;
use crate::tic_tac_toe::ClientEvent::MoveMade;

mod board;

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Draw,
    WinnerFound { player_id: u8 },
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum ServerEvent {
    BoardUpdated {
        board_cells: [Option<u8>; BOARD_SIZE],
    },
    PlayerTurn {
        player_id: u8,
    },
    GameOver {
        outcome: Outcome,
    },
    ErrorOccurred {
        error: Error,
    },
}

#[derive(Copy, Clone, Serialize, Deserialize, thiserror::Error, Debug)]
pub enum Error {
    #[error("The input should be a number between 1 and {}.", BOARD_SIZE)]
    InvalidCellIndex,
    #[error("This cell is already occupied.")]
    CellOccupied,
    #[error("It's not your turn.")]
    UnexpectedPlayer,
}

pub struct TicTacToeServer {
    current_player: u8,
    board: Board,
    server_channel: Sender<GameServerEvent<ServerEvent>>,
}

impl TicTacToeServer {
    pub fn new(server_channel: Sender<GameServerEvent<ServerEvent>>) -> TicTacToeServer {
        TicTacToeServer {
            current_player: PLAYER_ONE_ID,
            board: Board::new(),
            server_channel,
        }
    }

    fn swap_player(&mut self) {
        self.current_player = get_alternative_player_id(self.current_player);
    }

    async fn dispatch_board_updated_event(&self) {
        let board_cells = self.board.get_cell_occupiers();
        self.server_channel
            .send(GameServerEvent::DispatchToClient {
                dispatch_mode: DispatchMode::AllPlayers,
                event: ServerEvent::BoardUpdated { board_cells },
            })
            .await
            .unwrap()
    }

    async fn dispatch_player_turn_event(&self, dispatch_mode: DispatchMode) {
        self.server_channel
            .send(GameServerEvent::DispatchToClient {
                dispatch_mode,
                event: ServerEvent::PlayerTurn {
                    player_id: self.current_player,
                },
            })
            .await
            .unwrap()
    }

    fn handle_move_made_event(&mut self, player_id: u8, move_index: usize) -> Result<(), Error> {
        if player_id != self.current_player {
            return Err(Error::UnexpectedPlayer);
        }

        self.board.add_move(player_id, move_index)
    }
}

#[async_trait]
impl GameServer<ClientEvent> for TicTacToeServer {
    async fn begin(&self) {
        self.dispatch_board_updated_event().await;
        self.dispatch_player_turn_event(DispatchMode::AllPlayers)
            .await;
    }

    async fn handle_event(&mut self, event: ClientEvent) {
        return match event {
            MoveMade {
                player_id,
                move_index,
            } => {
                if let Err(error) = self.handle_move_made_event(player_id, move_index) {
                    self.server_channel
                        .send(GameServerEvent::DispatchToClient {
                            dispatch_mode: DispatchMode::SinglePlayer {
                                player_id: self.current_player,
                            },
                            event: ServerEvent::ErrorOccurred { error },
                        })
                        .await
                        .unwrap();

                    self.dispatch_player_turn_event(DispatchMode::SinglePlayer {
                        player_id: self.current_player,
                    })
                    .await;

                    return;
                }

                self.dispatch_board_updated_event().await;
                match self.board.determine_outcome() {
                    None => {
                        self.swap_player();
                        self.dispatch_player_turn_event(DispatchMode::AllPlayers)
                            .await;
                    }
                    Some(outcome) => {
                        self.server_channel
                            .send(GameServerEvent::DispatchToClient {
                                dispatch_mode: DispatchMode::AllPlayers,
                                event: ServerEvent::GameOver { outcome },
                            })
                            .await
                            .unwrap();

                        self.server_channel
                            .send(GameServerEvent::GameOver)
                            .await
                            .unwrap()
                    }
                }
            }
        };
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ClientEvent {
    MoveMade { player_id: u8, move_index: usize },
}

pub struct TicTacToeClient<I, O, C>
where
    I: io::BufRead + Send,
    O: io::Write + Send,
    C: ClientType,
{
    input: I,
    client_channel: Sender<GameClientEvent<ClientEvent>>,
    client_type: C,
    user_output: Arc<Mutex<O>>,
}

impl<I, O, C> TicTacToeClient<I, O, C>
where
    I: io::BufRead + Send,
    O: io::Write + Send,
    C: ClientType,
{
    pub fn new(
        input: I,
        output: Arc<Mutex<O>>,
        client_channel: Sender<GameClientEvent<ClientEvent>>,
        client_type: C,
    ) -> TicTacToeClient<I, O, C> {
        TicTacToeClient {
            input,
            user_output: output,
            client_channel,
            client_type,
        }
    }

    fn get_player_icon_by_id(&self, id: u8) -> char {
        match id {
            PLAYER_ONE_ID => 'X',
            PLAYER_TWO_ID => 'O',
            _ => panic!("Unexpected id provided"),
        }
    }

    fn get_optional_player_icon_by_id(&self, player_id: Option<u8>) -> char {
        match player_id {
            None => ' ',
            Some(i) => self.get_player_icon_by_id(i),
        }
    }

    async fn handle_board_updated_event(&self, board_cells: [Option<u8>; BOARD_SIZE]) {
        let cell_icons = board_cells.map(|x| self.get_optional_player_icon_by_id(x));
        let board_output = format!(
            "_________\n{} | {} | {}\n{} | {} | {}\n{} | {} | {}\n_________\n",
            cell_icons[0],
            cell_icons[1],
            cell_icons[2],
            cell_icons[3],
            cell_icons[4],
            cell_icons[5],
            cell_icons[6],
            cell_icons[7],
            cell_icons[8]
        );

        writeln!(&mut self.user_output.lock().unwrap(), "{}", board_output).unwrap()
    }

    async fn handle_game_over_event(&self, outcome: Outcome) {
        match outcome {
            Outcome::Draw => writeln!(
                &mut self.user_output.lock().unwrap(),
                "Game over! There was a draw!"
            )
            .unwrap(),
            Outcome::WinnerFound { player_id } => {
                let player_icon = self.get_player_icon_by_id(player_id);
                writeln!(
                    &mut self.user_output.lock().unwrap(),
                    "Game over! Player {} won!",
                    player_icon
                )
                .unwrap()
            }
        }
        self.client_channel
            .send(GameClientEvent::GameOver)
            .await
            .unwrap()
    }

    async fn handle_error_occurred_event(&self, error: Error) {
        writeln!(&mut self.user_output.lock().unwrap(), "Error: {}", error).unwrap()
    }

    async fn make_player_move(&mut self, player_id: u8) {
        let move_index = self.get_move().await;
        self.client_channel
            .send(GameClientEvent::DispatchToServer {
                event: MoveMade {
                    player_id,
                    move_index,
                },
            })
            .await
            .unwrap();
    }

    async fn get_move(&mut self) -> usize {
        loop {
            writeln!(
                &mut self.user_output.lock().unwrap(),
                "Input a number between 1 and {} to make your move:",
                BOARD_SIZE
            )
            .unwrap();

            let input_text = &mut String::new();
            self.input.read_line(input_text).unwrap();

            match input_text.trim().parse::<usize>() {
                Err(_) => writeln!(
                    &mut self.user_output.lock().unwrap(),
                    "That is not a number, please try again."
                )
                .unwrap(),
                Ok(index) => return index,
            };
        }
    }
}

#[async_trait]
pub trait ClientTypeEvent {
    fn get_game_started_message(&self) -> String;
    async fn handle_player_turn_event(&mut self, player_id: u8);
}

#[async_trait]
impl<I, O> ClientTypeEvent for TicTacToeClient<I, O, LocalClient>
where
    I: io::BufRead + Send,
    O: io::Write + Send,
{
    fn get_game_started_message(&self) -> String {
        String::from("Lets begin.")
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        let player_icon = self.get_player_icon_by_id(player_id);
        writeln!(
            &mut self.user_output.lock().unwrap(),
            "Player {}'s turn!",
            player_icon
        )
        .unwrap();

        self.make_player_move(player_id).await;
    }
}

#[async_trait]
impl<I, O> ClientTypeEvent for TicTacToeClient<I, O, OnlineClient>
where
    I: io::BufRead + Send,
    O: io::Write + Send,
{
    fn get_game_started_message(&self) -> String {
        String::from("All players connected, lets begin.")
    }

    async fn handle_player_turn_event(&mut self, player_id: u8) {
        if player_id != self.client_type.id {
            writeln!(
                &mut self.user_output.lock().unwrap(),
                "Waiting for other player to make a move."
            )
            .unwrap();
            return;
        }

        writeln!(&mut self.user_output.lock().unwrap(), "It's your turn!").unwrap();
        self.make_player_move(player_id).await;
    }
}

#[async_trait]
impl<I, O, C> GameClient<ServerEvent> for TicTacToeClient<I, O, C>
where
    I: io::BufRead + Send + Sync,
    O: io::Write + Send + Sync,
    C: ClientType + Send + Sync,
    Self: ClientTypeEvent,
{
    async fn handle_game_started_event(&self) {
        writeln!(
            &mut self.user_output.lock().unwrap(),
            "{}",
            self.get_game_started_message()
        )
        .unwrap();
    }

    async fn handle_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::GameOver { outcome } => self.handle_game_over_event(outcome).await,
            ServerEvent::BoardUpdated { board_cells } => {
                self.handle_board_updated_event(board_cells).await
            }
            ServerEvent::ErrorOccurred { error } => self.handle_error_occurred_event(error).await,
            ServerEvent::PlayerTurn { player_id } => self.handle_player_turn_event(player_id).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::sync::mpsc::Receiver;

    use super::*;

    async fn get_test_client_and_output<C: ClientType>(
        input: &[u8],
        client_type: C,
    ) -> (
        TicTacToeClient<BufReader<&[u8]>, Vec<u8>, C>,
        Arc<Mutex<Vec<u8>>>,
        Receiver<GameClientEvent<ClientEvent>>,
    ) {
        let output = Arc::new(Mutex::new(Vec::new()));
        let output_clone = Arc::clone(&output);

        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let client = TicTacToeClient::new(BufReader::new(input), output, sender, client_type);

        (client, output_clone, receiver)
    }

    fn assert_client_output(output: Arc<Mutex<Vec<u8>>>, expected: &str) {
        let mutex = output.lock().unwrap();
        let actual = mutex.as_slice();

        assert_eq!(
            actual,
            expected.as_bytes(),
            "expected\n{}, actual\n{}",
            std::str::from_utf8(actual).unwrap().to_string(),
            expected
        )
    }

    #[tokio::test]
    async fn client_handles_board_updated_event() {
        let (client, output, _) = get_test_client_and_output(&[], LocalClient {}).await;

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

        client.handle_board_updated_event(board_cells).await;
        assert_client_output(
            output,
            "_________\n  | X | X\nO |   |  \nO |   | X\n_________\n\n",
        )
    }

    #[tokio::test]
    async fn client_handles_game_over_event_for_draw() {
        let (client, output, mut receiver) = get_test_client_and_output(&[], LocalClient {}).await;

        client.handle_game_over_event(Outcome::Draw).await;
        assert_client_output(output, "Game over! There was a draw!\n");

        let event = receiver.recv().await;
        assert!(matches!(event, Some(GameClientEvent::GameOver)))
    }

    #[tokio::test]
    async fn client_handles_game_over_event_for_win() {
        let (client, output, mut receiver) = get_test_client_and_output(&[], LocalClient {}).await;

        client
            .handle_game_over_event(Outcome::WinnerFound { player_id: 1 })
            .await;
        assert_client_output(output, "Game over! Player X won!\n");

        let event = receiver.recv().await;
        assert!(matches!(event, Some(GameClientEvent::GameOver)))
    }

    #[tokio::test]
    async fn client_handles_error_occurred_event() {
        let (client, output, _) = get_test_client_and_output(&[], LocalClient {}).await;

        client
            .handle_error_occurred_event(Error::CellOccupied)
            .await;
        assert_client_output(output, "Error: This cell is already occupied.\n")
    }

    #[tokio::test]
    async fn client_get_move_handles_errors_until_valid_move_provided() {
        let input = "not a number\n1".as_bytes();

        let (mut client, output, _) = get_test_client_and_output(input, LocalClient {}).await;

        client.get_move().await;
        assert_client_output(output, "Input a number between 1 and 9 to make your move:\nThat is not a number, please try again.\nInput a number between 1 and 9 to make your move:\n")
    }

    #[tokio::test]
    async fn client_handles_game_started_event_for_local_client() {
        let (client, output, _) = get_test_client_and_output(&[], LocalClient {}).await;

        client.handle_game_started_event().await;
        assert_client_output(output, "Lets begin.\n")
    }

    #[tokio::test]
    async fn client_handles_game_started_event_for_online_client() {
        let (client, output, _) = get_test_client_and_output(&[], OnlineClient { id: 1 }).await;

        client.handle_game_started_event().await;
        assert_client_output(output, "All players connected, lets begin.\n")
    }

    #[tokio::test]
    async fn client_handles_player_turn_event_for_local_client() {
        let input = "2".as_bytes();
        let (mut client, output, mut receiver) =
            get_test_client_and_output(input, LocalClient {}).await;

        client.handle_player_turn_event(1).await;
        assert_client_output(
            output,
            "Player X's turn!\nInput a number between 1 and 9 to make your move:\n",
        );

        let event = receiver.recv().await;
        assert!(matches!(
            event,
            Some(GameClientEvent::DispatchToServer {
                event: MoveMade {
                    player_id: 1,
                    move_index: 2
                }
            })
        ))
    }

    #[tokio::test]
    async fn client_handles_player_turn_event_for_online_client_current_player() {
        let input = "3".as_bytes();
        let (mut client, output, mut receiver) =
            get_test_client_and_output(input, OnlineClient { id: 1 }).await;

        client.handle_player_turn_event(1).await;
        assert_client_output(
            output,
            "It's your turn!\nInput a number between 1 and 9 to make your move:\n",
        );

        let event = receiver.recv().await;
        assert!(matches!(
            event,
            Some(GameClientEvent::DispatchToServer {
                event: MoveMade {
                    player_id: 1,
                    move_index: 3
                }
            })
        ))
    }

    #[tokio::test]
    async fn client_handles_player_turn_event_for_online_client_other_player() {
        let input = "3".as_bytes();
        let (mut client, output, mut receiver) =
            get_test_client_and_output(input, OnlineClient { id: 1 }).await;

        client.handle_player_turn_event(2).await;
        assert_client_output(output, "Waiting for other player to make a move.\n");

        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }
}
