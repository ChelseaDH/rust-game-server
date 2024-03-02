use std::io;
use std::net::Ipv4Addr;

use tokio::net::{TcpListener, TcpStream};

use crate::client::Client;
use crate::connection::Connection;
use crate::game::Game;
use crate::lobby::Lobby;
use crate::server::LocalConnection;

mod client;
mod connection;
mod game;
mod lobby;
mod server;
mod tic_tac_toe;

const DEFAULT_PORT: u16 = 22222;

#[tokio::main]
async fn main() {
    println!("Hello from Rust Game Server, welcome to Tic Tac Toe!");

    match get_game_mode() {
        GameMode::Local => {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, DEFAULT_PORT))
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
                let mut server =
                    server::Server::<LocalConnection>::new(connection, Game::TicTacToe);
                server.init().await;
            });

            // Set up client connection
            let stream = TcpStream::connect(address).await.unwrap();
            let mut client = Client::<io::Stdout>::new_local(
                Connection::new(stream),
                io::BufReader::new(io::stdin()),
                io::stdout(),
                Game::TicTacToe,
            );
            client.play_game().await;

            // Wait for server thread to finish
            server_handle.await.unwrap();
        }
        GameMode::OnlineHost => {
            println!(
                "Do you wish to specify a port to bind to (the default is {}) y/N?",
                DEFAULT_PORT
            );
            let port = loop {
                match read_string().as_str() {
                    "" | "n" | "no" => break DEFAULT_PORT,
                    "y" | "yes" => {
                        println!("Please provide the port:");
                        match read_string().parse::<u16>() {
                            Err(_) => println!("That is not a valid number, try again."),
                            Ok(port) => break port,
                        }
                    }
                    _ => println!("That is not a valid option, please try again!"),
                };
            };

            let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            // In the case that the supplied port was "0", a random port will be allocated
            if port == 0 {
                println!("People can join you on port {}!", address.port());
            }

            // Spawn the server thread
            let server_handle = tokio::spawn(async move {
                let mut lobby = Lobby::new(listener);
                let mut server = lobby.set_up_online_server().await;
                server.init().await;
            });

            // Set up client connection
            let connection = lobby::connect_to_game(address).await.unwrap();
            let mut client = Client::<io::Stdout>::new_online(
                connection,
                server::PLAYER_ONE_ID,
                io::BufReader::new(io::stdin()),
                io::stdout(),
                Game::TicTacToe,
            );
            client.play_game().await;

            // Wait for server thread to finish
            server_handle.await.unwrap();
        }
        GameMode::OnlineJoin => {
            println!("Please enter the address of the game to join:");
            let address = read_string();

            match lobby::connect_to_game(address).await {
                Ok(connection) => {
                    let mut client = Client::<io::Stdout>::new_online(
                        connection,
                        server::PLAYER_TWO_ID,
                        io::BufReader::new(io::stdin()),
                        io::stdout(),
                        Game::TicTacToe,
                    );
                    client.play_game().await;
                }
                Err(_) => eprintln!("Error connecting to game. Aborting."),
            }
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
    io::stdin()
        .read_line(&mut input_text)
        .expect("Failed to read user input");

    input_text.trim().to_string().to_ascii_lowercase()
}
