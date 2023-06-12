mod client;
mod server;

use std::sync::mpsc;
use std::thread;

fn main() {
    println!("Hello from Rust Game Server, welcome to Tic Tac Toe!");

    // Channel for sending messages from the server thread to the client (main) thread
    let (tx_s, rx_c) = mpsc::channel();

    // Channel for sending messages from the client (main) thread to the server thread
    let (tx_c, rx_s) = mpsc::channel();

    // Spawn the server thread
    let server_handle = thread::spawn(move || {
        // Game set up
        let player_one = server::Player::new_player_one(&tx_s, &rx_s);
        let player_two = server::Player::new_player_two(&tx_s, &rx_s);

        let mut server = server::Server::new(&player_one, &player_two);
        server.play_game();
    });

    let client = client::Client::new(&tx_c);
    for event in rx_c {
        client.handle_event(event);
    }

    server_handle.join().unwrap();
}
