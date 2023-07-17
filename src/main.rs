use tokio::net::{TcpListener, TcpStream};

use crate::{client::Client, connection::Connection};

mod client;
mod connection;
mod server;

#[tokio::main]
async fn main() {
    println!("Hello from Rust Game Server, welcome to Tic Tac Toe!");

    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();

    // Spawn the server thread
    let server_handle = tokio::spawn(async move {
        // Wait for a connection of player one
        let (stream_one, _) = listener.accept().await.unwrap();

        // Wait for a connection of player two
        let (stream_two, _) = listener.accept().await.unwrap();

        // Create connections for the players
        let connection_one = Connection::new(1, stream_one);
        let connection_two = Connection::new(1, stream_two);

        // Game set up
        let player_one = server::Player::new_player_one(connection_one);
        let player_two = server::Player::new_player_two(connection_two);

        // Play the game
        let mut server = server::Server::new(player_one, player_two);
        server.play_game().await;
    });

    // Spawn a thread for a single player
    let client_handle = tokio::spawn(async {
        let stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
        let mut client = Client::new(Connection::new(0, stream));
        client.play_game().await;
    });

    // Set up connection for the other player
    let stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    let mut client = Client::new(Connection::new(1, stream));
    client.play_game().await;

    // Wait for the server and client to finish
    client_handle.await.unwrap();
    server_handle.await.unwrap();
}
