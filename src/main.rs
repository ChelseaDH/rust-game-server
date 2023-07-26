use std::net::Ipv4Addr;
use tokio::net::{TcpListener, TcpStream};

use crate::server::{LocalConnection, OnlineConnection};
use crate::{client::Client, connection::Connection};

mod client;
mod connection;
mod server;

const PORT: u16 = 22222;

#[tokio::main]
async fn main() {
    println!("Hello from Rust Game Server, welcome to Tic Tac Toe!");

    match get_game_mode() {
        GameMode::Local => {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, PORT))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();

            // Spawn the server thread
            let server_handle = tokio::spawn(async move {
                // Wait for a client connection
                let (stream_one, _) = listener.accept().await.unwrap();

                // Create connection
                let connection = Connection::new(stream_one);

                // Play the game
                let mut server = server::Server::<LocalConnection>::new(connection);
                server.init().await;
            });

            // Set up client connection
            let stream = TcpStream::connect(address).await.unwrap();
            let mut client = Client::new(Connection::new(stream));
            client.play_game().await;

            // Wait for server thread to finish
            server_handle.await.unwrap();
        }
        GameMode::OnlineHost => {
            let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            println!("People can join you on port {}!", PORT);

            // Spawn the server thread
            let server_handle = tokio::spawn(async move {
                // Wait for a connection of player one
                let (stream_one, _) = listener.accept().await.unwrap();

                // Wait for a connection of player two
                let (stream_two, _) = listener.accept().await.unwrap();

                // Create connections for the players
                let connection_one = Connection::new(stream_one);
                let connection_two = Connection::new(stream_two);

                // Game set up
                let player_one = server::Player::new_player_one(connection_one);
                let player_two = server::Player::new_player_two(connection_two);

                // Play the game
                let mut server = server::Server::<OnlineConnection>::new(player_one, player_two);
                server.init().await;
            });

            // Set up client connection
            let stream = TcpStream::connect(address).await.unwrap();
            let mut client = Client::new(Connection::new(stream));
            client.play_game().await;

            // Wait for server thread to finish
            server_handle.await.unwrap();
        }
        GameMode::OnlineJoin => {
            println!("Please enter the address of the game to join:");
            let address = read_string();

            let stream = TcpStream::connect(address).await.unwrap();
            let mut client = Client::new(Connection::new(stream));
            client.play_game().await;
        }
    }
}

enum GameMode {
    Local,
    OnlineHost,
    OnlineJoin,
}

fn get_game_mode() -> GameMode {
    loop {
        println!("Please select your game mode; local or online.");
        match read_string().as_str() {
            "local" => {
                return GameMode::Local;
            }
            "online" => {
                println!("Do you want to host or join a game?");
                match read_string().as_str() {
                    "host" => return GameMode::OnlineHost,
                    "join" => return GameMode::OnlineJoin,
                    _ => {
                        println!("That is not a valid option, please try again!");
                        continue;
                    }
                }
            }
            _ => {
                println!("That is not a valid option, please try again!");
                continue;
            }
        }
    }
}

fn read_string() -> String {
    let mut input_text = String::new();
    std::io::stdin()
        .read_line(&mut input_text)
        .expect("Failed to read user input");

    input_text.trim().to_string().to_ascii_lowercase()
}
