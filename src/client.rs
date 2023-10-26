use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;

use crate::connection::{Connection, ReadError, WriteError};
use crate::game::{GameClient, GameClientEvent};
use crate::server;
use crate::tic_tac_toe::{self, ClientEvent, TicTacToeClient};

pub trait ClientType {}

pub struct LocalClient {}

impl ClientType for LocalClient {}

pub struct OnlineClient {
    pub(crate) id: u8,
}

impl ClientType for OnlineClient {}

pub struct Client<O, GE, G, GSE>
where
    O: io::Write + Send + Sync + Sync,
    GE: Serialize + Send,
    G: GameClient<GSE>,
    GSE: DeserializeOwned + Send,
{
    running: bool,
    server_connection: Connection,
    user_output: Arc<Mutex<O>>,
    game: G,
    game_receiver: Receiver<GameClientEvent<GE>>,
    game_server_event_type: PhantomData<GSE>,
}

impl<O, GE, G, GSE> Client<O, GE, G, GSE>
where
    O: io::Write + Send + Sync,
    GE: Serialize + Send,
    G: GameClient<GSE>,
    GSE: DeserializeOwned + Send,
{
    pub fn new_local_tic_tac_toe<I: io::BufRead + Send + Sync>(
        connection: Connection,
        input: I,
        output: O,
    ) -> Client<O, ClientEvent, TicTacToeClient<I, O, LocalClient>, tic_tac_toe::ServerEvent> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let output = Arc::new(Mutex::new(output));
        let output_clone = Arc::clone(&output);

        Client {
            running: true,
            server_connection: connection,
            user_output: output,
            game: TicTacToeClient::new(input, output_clone, game_sender, LocalClient {}),
            game_receiver,
            game_server_event_type: PhantomData,
        }
    }

    pub fn new_online_tic_tac_toe<I: io::BufRead + Send + Sync>(
        connection: Connection,
        id: u8,
        input: I,
        output: O,
    ) -> Client<O, ClientEvent, TicTacToeClient<I, O, OnlineClient>, tic_tac_toe::ServerEvent> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let output = Arc::new(Mutex::new(output));
        let output_clone = Arc::clone(&output);

        Client {
            running: true,
            server_connection: connection,
            user_output: output,
            game: TicTacToeClient::new(input, output_clone, game_sender, OnlineClient { id }),
            game_receiver,
            game_server_event_type: PhantomData,
        }
    }
}

impl<O, GE, G, GSE> Client<O, GE, G, GSE>
where
    O: io::Write + Send + Sync,
    GE: Serialize + Send,
    G: GameClient<GSE>,
    GSE: DeserializeOwned + Send,
{
    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent<GSE, GE>, ReadError> {
        tokio::select! {
            result = self.game_receiver.recv() => Ok(IncomingEvent::Game(result.unwrap())),
            result = self.server_connection.read_event() => result.map(IncomingEvent::Server),
        }
    }

    pub async fn play_game(&mut self) {
        while self.running {
            match self.get_next_incoming_event().await {
                Ok(event) => {
                    if (self.handle_event(event).await).is_err() {
                        self.handle_shutdown().await
                    }
                }
                Err(_) => self.handle_shutdown().await,
            }
        }
    }

    pub async fn handle_event(&mut self, event: IncomingEvent<GSE, GE>) -> Result<(), WriteError> {
        match event {
            IncomingEvent::Server(server_event) => {
                match server_event {
                    server::OutgoingEvent::ErrorOccurred(error) => self.handle_error(error),
                    server::OutgoingEvent::GameStarted => {
                        self.game.handle_game_started_event().await
                    }
                    server::OutgoingEvent::Shutdown => self.handle_shutdown().await,
                    server::OutgoingEvent::Game { event } => self.game.handle_event(event).await,
                };
                Ok(())
            }
            IncomingEvent::Game(game_event) => match game_event {
                GameClientEvent::DispatchToServer { event } => {
                    self.server_connection.write_event(event).await
                }
                GameClientEvent::Write { message } => {
                    writeln!(&mut self.user_output.lock().unwrap(), "{}", message).unwrap();
                    Ok(())
                }
                GameClientEvent::GameOver => {
                    self.shutdown().await;
                    Ok(())
                }
            },
        }
    }

    fn handle_error(&mut self, error: server::Error) {
        writeln!(&mut self.user_output.lock().unwrap(), "Error: {}", error).unwrap();
    }

    async fn handle_shutdown(&mut self) {
        writeln!(
            &mut self.user_output.lock().unwrap(),
            "An unrecoverable error has occurred, game terminating."
        )
        .unwrap();

        self.shutdown().await
    }

    async fn shutdown(&mut self) {
        self.running = false;
        let _ = self.server_connection.shutdown().await;
    }
}

pub enum IncomingEvent<SE: DeserializeOwned + Send, GE: Serialize + Send> {
    Server(server::OutgoingEvent<SE>),
    Game(GameClientEvent<GE>),
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use tokio::net::{TcpListener, TcpStream};

    use super::*;

    async fn get_test_stream() -> TcpStream {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        TcpStream::connect(local_addr).await.unwrap()
    }

    async fn get_local_test_client<'a>(
        input: &'a [u8],
        output: &'a mut Vec<u8>,
    ) -> Client<
        &'a mut Vec<u8>,
        ClientEvent,
        TicTacToeClient<&'a [u8], &'a mut Vec<u8>, LocalClient>,
        tic_tac_toe::ServerEvent,
    > {
        Client::<
            &'a mut Vec<u8>,
            ClientEvent,
            TicTacToeClient<&'a [u8], &'a mut Vec<u8>, LocalClient>,
            tic_tac_toe::ServerEvent,
        >::new_local_tic_tac_toe(Connection::new(get_test_stream().await), input, output)
    }

    #[tokio::test]
    async fn generic_client_handles_shutdown_event_from_server() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(IncomingEvent::Server(server::OutgoingEvent::Shutdown))
            .await
            .unwrap();
        assert_eq!(client.running, false);
        assert_eq!(
            output,
            b"An unrecoverable error has occurred, game terminating.\n"
        )
    }

    #[tokio::test]
    async fn generic_client_handles_error_event_from_server() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(IncomingEvent::Server(server::OutgoingEvent::ErrorOccurred(
                server::Error::InvalidMessage,
            )))
            .await
            .unwrap();
        assert_eq!(client.running, true);
        assert_eq!(output, b"Error: Invalid message sent.\n")
    }

    #[tokio::test]
    async fn generic_client_handles_write_event_from_game() {
        let mut output = Vec::new();
        let mut client = get_local_test_client(&[], &mut output).await;

        client
            .handle_event(IncomingEvent::Game(GameClientEvent::Write {
                message: String::from("Hello player!"),
            }))
            .await
            .unwrap();
        assert_eq!(client.running, true);
        assert_eq!(output, b"Hello player!\n")
    }
}
