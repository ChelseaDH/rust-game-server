use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use crate::client;
use crate::game::{GameEvent, GameServer};
use crate::server::{get_alternative_player_id, DispatchMode, PLAYER_ONE_ID};
use crate::tic_tac_toe::board::Board;
pub use crate::tic_tac_toe::board::BOARD_SIZE;

mod board;

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Draw,
    WinnerFound { player_id: u8 },
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Event {
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
    #[error("The input should be a number between 0 and {}.", BOARD_SIZE - 1)]
    InvalidCellIndex,
    #[error("This cell is already occupied.")]
    CellOccupied,
    #[error("It's not your turn.")]
    UnexpectedPlayer,
}

pub struct TicTacToeServer {
    current_player: u8,
    board: Board,
    server_channel: Sender<GameEvent<Event>>,
}

impl TicTacToeServer {
    pub fn new(server_channel: Sender<GameEvent<Event>>) -> TicTacToeServer {
        TicTacToeServer {
            current_player: PLAYER_ONE_ID,
            board: Board::new(),
            server_channel,
        }
    }

    fn swap_player(&mut self) {
        self.current_player = get_alternative_player_id(self.current_player);
    }

    async fn dispatch_board_updated_event(&mut self) {
        let board_cells = self.board.get_cell_occupiers();
        self.server_channel
            .send(GameEvent::DispatchToClient {
                dispatch_mode: DispatchMode::AllPlayers,
                event: Event::BoardUpdated { board_cells },
            })
            .await
            .unwrap()
    }

    async fn dispatch_player_turn_event(&mut self, dispatch_mode: DispatchMode) {
        self.server_channel
            .send(GameEvent::DispatchToClient {
                dispatch_mode,
                event: Event::PlayerTurn {
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
impl GameServer<client::Event> for TicTacToeServer {
    async fn begin(&mut self) {
        self.dispatch_board_updated_event().await;
        self.dispatch_player_turn_event(DispatchMode::AllPlayers)
            .await;
    }

    async fn handle_event(&mut self, event: client::Event) {
        return match event {
            client::Event::MoveMade {
                player_id,
                move_index,
            } => {
                if let Err(error) = self.handle_move_made_event(player_id, move_index) {
                    self.server_channel
                        .send(GameEvent::DispatchToClient {
                            dispatch_mode: DispatchMode::SinglePlayer {
                                player_id: self.current_player,
                            },
                            event: Event::ErrorOccurred { error },
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
                            .send(GameEvent::DispatchToClient {
                                dispatch_mode: DispatchMode::AllPlayers,
                                event: Event::GameOver { outcome },
                            })
                            .await
                            .unwrap();

                        self.server_channel.send(GameEvent::GameOver).await.unwrap()
                    }
                }
            }
        };
    }
}
