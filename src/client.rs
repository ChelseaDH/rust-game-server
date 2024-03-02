use std::io;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;

use crate::connection::{Connection, ReadError, WriteError};
use crate::game::{Game, GameClient, GameClientEvent};
use crate::server;
use crate::tic_tac_toe::TicTacToeClient;

pub trait ClientType {}

pub struct LocalClient {}

impl ClientType for LocalClient {}

pub struct OnlineClient {
    pub(crate) id: u8,
}

impl ClientType for OnlineClient {}

pub struct Client<'a, O>
where
    O: io::Write + Send + Sync + 'a,
{
    running: bool,
    server_connection: Connection,
    user_output: Arc<Mutex<O>>,
    game: Box<dyn GameClient + 'a>,
    game_receiver: Receiver<GameClientEvent>,
}

impl<'a, O> Client<'a, O>
where
    O: io::Write + Send + Sync + 'a,
{
    pub fn new_local<I: io::BufRead + Send + Sync + 'a>(
        connection: Connection,
        input: I,
        output: O,
        game: Game,
    ) -> Client<'a, O> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let output = Arc::new(Mutex::new(output));
        let output_clone = Arc::clone(&output);

        let game = match game {
            Game::TicTacToe => Box::new(TicTacToeClient::new(
                input,
                output_clone,
                game_sender,
                LocalClient {},
            )),
        };

        Client {
            running: true,
            server_connection: connection,
            user_output: output,
            game,
            game_receiver,
        }
    }

    pub fn new_online_tic_tac_toe<I: io::BufRead + Send + Sync + 'a>(
        connection: Connection,
        id: u8,
        input: I,
        output: O,
        game: Game,
    ) -> Client<'a, O> {
        let (game_sender, game_receiver) = mpsc::channel(10);
        let output = Arc::new(Mutex::new(output));
        let output_clone = Arc::clone(&output);

        let game = match game {
            Game::TicTacToe => Box::new(TicTacToeClient::new(
                input,
                output_clone,
                game_sender,
                OnlineClient { id },
            )),
        };

        Client {
            running: true,
            server_connection: connection,
            user_output: output,
            game,
            game_receiver,
        }
    }

    async fn get_next_incoming_event(&mut self) -> Result<IncomingEvent, ReadError> {
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

    pub async fn handle_event(&mut self, event: IncomingEvent) -> Result<(), Error> {
        match event {
            IncomingEvent::Server(server_event) => match server_event {
                server::OutgoingEvent::ErrorOccurred(error) => self.handle_error(error),
                server::OutgoingEvent::GameStarted => self.game.handle_game_started_event().await,
                server::OutgoingEvent::Shutdown => self.handle_shutdown().await,
                server::OutgoingEvent::Game { event } => self.game.handle_event(event).await?,
            },
            IncomingEvent::Game(game_event) => match game_event {
                GameClientEvent::DispatchToServer { event } => {
                    self.server_connection.write_event(&event).await?
                }
                GameClientEvent::GameOver => self.shutdown().await,
            },
        };

        Ok(())
    }

    fn handle_error(&self, error: server::Error) {
        writeln!(&mut self.user_output.lock().unwrap(), "Error: {}", error).unwrap();
    }

    async fn handle_shutdown(&mut self) {
        let _ = writeln!(
            &mut self.user_output.lock().unwrap(),
            "An unrecoverable error has occurred, game terminating."
        );

        self.shutdown().await
    }

    async fn shutdown(&mut self) {
        self.running = false;
        let _ = self.server_connection.shutdown().await;
    }
}

pub enum IncomingEvent {
    Server(server::OutgoingEvent),
    Game(GameClientEvent),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to write to stream")]
    Write(#[from] WriteError),
    #[error("Failed to read or write from user")]
    Read(#[from] io::Error),
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

    async fn get_local_test_client<'a>(output: &'a mut Vec<u8>) -> Client<'a, &'a mut Vec<u8>> {
        Client::<'a, &'a mut Vec<u8>>::new_local(
            Connection::new(get_test_stream().await),
            &b""[..],
            output,
            Game::TicTacToe,
        )
    }

    #[tokio::test]
    async fn generic_client_handles_shutdown_event_from_server() {
        let mut output = Vec::new();

        {
            let mut client = get_local_test_client(&mut output).await;
            client
                .handle_event(IncomingEvent::Server(server::OutgoingEvent::Shutdown))
                .await
                .unwrap();
            assert_eq!(client.running, false);
        }

        assert_eq!(
            output,
            b"An unrecoverable error has occurred, game terminating.\n"
        )
    }

    #[tokio::test]
    async fn generic_client_handles_error_event_from_server() {
        let mut output = Vec::new();

        {
            let mut client = get_local_test_client(&mut output).await;
            client
                .handle_event(IncomingEvent::Server(server::OutgoingEvent::ErrorOccurred(
                    server::Error::InvalidMessage,
                )))
                .await
                .unwrap();
            assert_eq!(client.running, true);
        }

        assert_eq!(output, b"Error: Invalid message sent.\n")
    }
}
