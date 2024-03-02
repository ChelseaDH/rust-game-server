use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::__private::DisplayAsDisplay;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::connection::{self, Connection};
use crate::game::Game;
use crate::server::{OnlineConnection, Player, Server};

const GAME_ID: u16 = 12345;

pub struct Lobby {
    listener: TcpListener,
}

impl Lobby {
    pub fn new(listener: TcpListener) -> Self {
        Lobby { listener }
    }

    pub async fn set_up_online_server(&mut self) -> Server<OnlineConnection> {
        let connection_one = self.get_connection().await;
        let connection_two = self.get_connection().await;

        let player_one = Player::new_player_one(connection_one);
        let player_two = Player::new_player_two(connection_two);

        Server::<OnlineConnection>::new(player_one, player_two, Game::TicTacToe)
    }

    async fn get_connection(&mut self) -> Connection {
        loop {
            let (stream, _) = self.listener.accept().await.unwrap();
            let mut connection = Connection::new(stream);

            if let Ok(ConnectionRequest { game_id }) = connection.read_event().await {
                if game_id == GAME_ID {
                    break connection;
                }
            }

            let _ = connection.shutdown().await;
            continue;
        }
    }
}

pub async fn connect_to_game<A: ToSocketAddrs>(addr: A) -> Result<Connection, Error> {
    let stream = TcpStream::connect(addr).await?;
    let mut connection = Connection::new(stream);
    connection
        .write_event(&ConnectionRequest { game_id: GAME_ID })
        .await?;

    Ok(connection)
}

#[derive(Serialize, Deserialize)]
struct ConnectionRequest {
    game_id: u16,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    StreamError(#[from] std::io::Error),
    WriteError(#[from] connection::WriteError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_display())
    }
}

#[cfg(test)]
mod tests {
    use crate::game;
    use std::net::Ipv4Addr;

    use crate::server::{IncomingEvent, ServerGameMode};
    use crate::tic_tac_toe::ClientEvent;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestEvent {
        content: String,
    }

    #[tokio::test]
    async fn test_set_up_online_server_returns_server_with_expected_player_connections() {
        // Set up listener and Lobby
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        // Create thread for lobby to process within
        let lobby_handle = tokio::spawn(async move {
            let mut lobby = Lobby { listener };
            let mut server = lobby.set_up_online_server().await;

            // Assert that player connections are as expected based on received messages
            // It's not possible to predict the order that the messages will be received in, so we conditionally assert
            for _i in 0..1 {
                match server.get_next_incoming_event().await.unwrap() {
                    IncomingEvent::Client(event) => {
                        let deserialized_event: ClientEvent = game::deserialize_event(event);
                        match deserialized_event {
                            ClientEvent::MoveMade {
                                player_id,
                                move_index,
                            } => {
                                if player_id == 1 {
                                    assert_eq!(move_index, 5)
                                } else {
                                    assert_eq!(move_index, 8)
                                }
                            }
                        }
                    }
                    _ => panic!("Unexpected event received from player one connection"),
                };
            }
        });

        // Simulate Client connecting and sending valid connection request
        let stream_one = TcpStream::connect(local_addr).await.unwrap();
        let mut connection_one = Connection::new(stream_one);
        connection_one
            .write_event(&ConnectionRequest { game_id: 12345 })
            .await
            .unwrap();
        connection_one
            .write_event(&game::serialize_event(ClientEvent::MoveMade {
                player_id: 1,
                move_index: 5,
            }))
            .await
            .unwrap();

        // Simulate bogus Client attempting to connect
        let bogus_stream = TcpStream::connect(local_addr).await.unwrap();
        let mut bogus_connection = Connection::new(bogus_stream);
        bogus_connection
            .write_event(&ConnectionRequest { game_id: 999 })
            .await
            .unwrap();
        bogus_connection
            .write_event(&game::serialize_event(ClientEvent::MoveMade {
                player_id: 2,
                move_index: 2,
            }))
            .await
            .unwrap();

        // Simulate Client connecting and sending valid connection request
        let stream_two = TcpStream::connect(local_addr).await.unwrap();
        let mut connection_two = Connection::new(stream_two);
        connection_two
            .write_event(&ConnectionRequest { game_id: 12345 })
            .await
            .unwrap();
        connection_two
            .write_event(&game::serialize_event(ClientEvent::MoveMade {
                player_id: 2,
                move_index: 8,
            }))
            .await
            .unwrap();

        lobby_handle.await.unwrap()
    }

    #[tokio::test]
    async fn test_get_connection_returns_connection_that_sends_correct_request() {
        // Set up listener and Lobby
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        // Create thread for lobby to process within
        let lobby_handle = tokio::spawn(async move {
            let mut lobby = Lobby { listener };
            let mut connection = lobby.get_connection().await;

            // Assert that connection is as expected based on sent message
            let event: TestEvent = connection.read_event().await.unwrap();
            assert_eq!(event.content, String::from("Content from correct client"))
        });

        // Simulate Client connecting and sending invalid connection request
        let stream_one = TcpStream::connect(local_addr).await.unwrap();
        let mut connection_one = Connection::new(stream_one);
        connection_one
            .write_event(&ConnectionRequest { game_id: 999 })
            .await
            .unwrap();
        connection_one
            .write_event(&TestEvent {
                content: String::from("Content from incorrect client"),
            })
            .await
            .unwrap();

        // Simulate Client connecting and sending valid connection request
        let stream_two = TcpStream::connect(local_addr).await.unwrap();
        let mut connection_two = Connection::new(stream_two);
        connection_two
            .write_event(&ConnectionRequest { game_id: 12345 })
            .await
            .unwrap();
        connection_two
            .write_event(&TestEvent {
                content: String::from("Content from correct client"),
            })
            .await
            .unwrap();

        lobby_handle.await.unwrap()
    }
}
