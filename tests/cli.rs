use std::process::Stdio;
use std::time::Duration;

use regex::Regex;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct InputOutput<I: AsyncWrite + Unpin, O: AsyncBufRead + Unpin> {
    stdin: I,
    stdout: O,
}

impl<I: AsyncWrite + Unpin, O: AsyncBufRead + Unpin> InputOutput<I, O> {
    fn new(stdin: I, stdout: O) -> InputOutput<I, O> {
        InputOutput { stdin, stdout }
    }

    async fn stream_contains(&mut self, string: &String) -> String {
        let mut buf = String::new();

        loop {
            self.stdout.read_line(&mut buf).await.unwrap();
            if buf.contains(string) {
                return buf;
            }
        }
    }

    async fn assert_stdout_contains(&mut self, string: &String) -> String {
        tokio::time::timeout(Duration::from_secs(10), self.stream_contains(string))
            .await
            .expect(
                format!(
                    "Timeout reached while asserting stream contents: {}",
                    string
                )
                .as_str(),
            )
    }

    async fn assert_stream_contains_board(&mut self, cells: [&str; 9]) {
        let expected_board = format!(
            "_________\n{} | {} | {}\n{} | {} | {}\n{} | {} | {}\n_________\n",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            cells[4],
            cells[5],
            cells[6],
            cells[7],
            cells[8]
        );
        self.assert_stdout_contains(&expected_board).await;
    }

    async fn assert_player_move(&mut self, prompt: &str, move_made: u8, board_cells: [&str; 9]) {
        // Player turn event being received
        self.assert_stdout_contains(&String::from(prompt)).await;
        self.assert_stdout_contains(&String::from(
            "Input a number between 0 and 8 to make your move:",
        ))
        .await;

        // Move made
        self.stdin
            .write_all(&[48 + move_made, b'\n']) // 48 is ascii '0'
            .await
            .unwrap();
        self.assert_stream_contains_board(board_cells).await;
    }

    async fn write_string(&mut self, string: &str) {
        self.stdin.write_all(string.as_bytes()).await.unwrap();
    }
}

fn get_io() -> (InputOutput<ChildStdin, BufReader<ChildStdout>>, Child) {
    // Set up command to run the program
    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    cmd.stdin(Stdio::piped()); // Open a pipe for stdin
    cmd.stdout(Stdio::piped()); // Open a pipe for stdout

    // Spawn the command and get handles to stdin and stdout
    let mut child = cmd.spawn().unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());

    (InputOutput::new(stdin, stdout), child)
}

#[tokio::test]
async fn local_game_runs_until_win() {
    // Set up command to run the program
    let (mut io, _) = get_io();

    // Prompt for the game mode
    io.assert_stdout_contains(&String::from(
        "Please select your game mode; local or online.",
    ))
    .await;

    // User selects the local game mode
    io.write_string("local\n").await;

    // Game begin event being received
    io.assert_stdout_contains(&String::from("Lets begin."))
        .await;

    // Initial board updated event being received
    io.assert_stream_contains_board([" "; 9]).await;

    // Player moves being made
    io.assert_player_move(
        "Player X's turn",
        1,
        [" ", "X", " ", " ", " ", " ", " ", " ", " "],
    )
    .await;
    io.assert_player_move(
        "Player O's turn",
        4,
        [" ", "X", " ", " ", "O", " ", " ", " ", " "],
    )
    .await;
    io.assert_player_move(
        "Player X's turn",
        0,
        ["X", "X", " ", " ", "O", " ", " ", " ", " "],
    )
    .await;
    io.assert_player_move(
        "Player O's turn",
        6,
        ["X", "X", " ", " ", "O", " ", "O", " ", " "],
    )
    .await;
    io.assert_player_move(
        "Player X's turn",
        2,
        ["X", "X", "X", " ", "O", " ", "O", " ", " "],
    )
    .await;

    // Game over event being received
    io.assert_stdout_contains(&String::from("Game over! Player X won!"))
        .await;
}

#[tokio::test]
async fn online_game_runs_until_win() {
    // Set up command to run the program
    let (mut player_one_io, _) = get_io();
    let (mut player_two_io, _) = get_io();

    // Assert online connections of both players
    assert_online_client_connections(&mut player_one_io, &mut player_two_io).await;

    // Game begin event being received
    player_one_io
        .assert_stdout_contains(&String::from("All players connected, lets begin."))
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("All players connected, lets begin."))
        .await;

    // Initial board updated event being received
    player_one_io.assert_stream_contains_board([" "; 9]).await;
    player_two_io.assert_stream_contains_board([" "; 9]).await;

    // Player one makes a valid move
    player_one_io
        .assert_player_move(
            "It's your turn!",
            1,
            [" ", "X", " ", " ", " ", " ", " ", " ", " "],
        )
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("Waiting for other player to make a move."))
        .await;
    player_two_io
        .assert_stream_contains_board([" ", "X", " ", " ", " ", " ", " ", " ", " "])
        .await;

    // Player two makes a valid move
    player_one_io
        .assert_stdout_contains(&String::from("Waiting for other player to make a move."))
        .await;
    player_two_io
        .assert_player_move(
            "It's your turn!",
            4,
            [" ", "X", " ", " ", "O", " ", " ", " ", " "],
        )
        .await;
    player_one_io
        .assert_stream_contains_board([" ", "X", " ", " ", "O", " ", " ", " ", " "])
        .await;

    // Player one makes an invalid move, followed by a valid one
    player_one_io
        .assert_stdout_contains(&String::from(
            "It's your turn!\nInput a number between 0 and 8 to make your move:",
        ))
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("Waiting for other player to make a move."))
        .await;
    player_one_io.write_string("10\n").await;
    player_one_io
        .assert_stdout_contains(&String::from(
            "Error: The input should be a number between 0 and 8.",
        ))
        .await;
    player_one_io
        .assert_player_move(
            "It's your turn!",
            0,
            ["X", "X", " ", " ", "O", " ", " ", " ", " "],
        )
        .await;
    player_two_io
        .assert_stream_contains_board(["X", "X", " ", " ", "O", " ", " ", " ", " "])
        .await;

    // Player two makes a valid move
    player_one_io
        .assert_stdout_contains(&String::from("Waiting for other player to make a move."))
        .await;
    player_two_io
        .assert_player_move(
            "It's your turn!",
            6,
            ["X", "X", " ", " ", "O", " ", "O", " ", " "],
        )
        .await;
    player_one_io
        .assert_stream_contains_board(["X", "X", " ", " ", "O", " ", "O", " ", " "])
        .await;

    // Player one makes a valid move
    player_one_io
        .assert_player_move(
            "It's your turn!",
            2,
            ["X", "X", "X", " ", "O", " ", "O", " ", " "],
        )
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("Waiting for other player to make a move."))
        .await;
    player_two_io
        .assert_stream_contains_board(["X", "X", "X", " ", "O", " ", "O", " ", " "])
        .await;

    // Game over event being received
    player_one_io
        .assert_stdout_contains(&String::from("Game over! Player X won!"))
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("Game over! Player X won!"))
        .await;
}

#[tokio::test]
async fn online_game_handles_client_disconnection() {
    env_logger::builder().is_test(true).try_init().unwrap();

    // Set up command to run the program
    let (mut player_one_io, _) = get_io();
    let (mut player_two_io, mut player_two_process) = get_io();

    // Assert online connections of both players
    assert_online_client_connections(&mut player_one_io, &mut player_two_io).await;

    // Game begin event being received
    player_one_io
        .assert_stdout_contains(&String::from("All players connected, lets begin."))
        .await;
    player_two_io
        .assert_stdout_contains(&String::from("All players connected, lets begin."))
        .await;

    // Player two disconnects
    player_two_process
        .kill()
        .await
        .expect("Failure killing process for player two");

    // Player one makes a move
    player_one_io
        .assert_player_move(
            "It's your turn!",
            1,
            [" ", "X", " ", " ", " ", " ", " ", " ", " "],
        )
        .await;

    // Player one receives Shutdown event
    player_one_io
        .assert_stdout_contains(&String::from(
            "An unrecoverable error has occurred, game terminating.",
        ))
        .await;
}

#[tokio::test]
async fn online_game_handles_client_sending_malicious_message() {
    // Set up command to run the program
    let (mut player_one_io, _) = get_io();

    // Set up player one to host the game
    player_one_io
        .assert_stdout_contains(&String::from(
            "Please select your game mode; local or online.",
        ))
        .await;
    player_one_io.write_string("online\n").await;
    player_one_io
        .assert_stdout_contains(&String::from("Do you want to host or join a game?"))
        .await;
    player_one_io.write_string("host\n").await;
    player_one_io
        .assert_stdout_contains(&String::from(
            "Do you wish to specify a port to bind to (the default is 22222) y/N?",
        ))
        .await;
    player_one_io.write_string("yes\n").await;
    player_one_io
        .assert_stdout_contains(&String::from("Please provide the port:"))
        .await;
    player_one_io.write_string("0\n").await;
    // Grab the bound port for later connecting, ensuring the port is always available
    let buf = player_one_io
        .assert_stdout_contains(&String::from("People can join you on port"))
        .await;
    let re = Regex::new(r"People can join you on port\s*(\d*)\s*!").unwrap();
    let port = re
        .captures(buf.as_str())
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .parse::<u16>()
        .unwrap();

    // Set up malicious connection of player two
    let mut stream = TcpStream::connect(("0.0.0.0", port)).await.unwrap();
    stream.write_all(&17_u16.to_be_bytes()[..]).await.unwrap();
    stream.write_all(b"{\"game_id\":12345}").await.unwrap();

    // Game begin event being received
    player_one_io
        .assert_stdout_contains(&String::from("All players connected, lets begin."))
        .await;

    // Client two sends malicious message
    stream
        .write_all(b"insert malicious message here")
        .await
        .unwrap();

    // Client two receives invalid message error and shutdown request
    let mut buf = String::new();
    stream.read_to_string(&mut buf).await.unwrap();
    assert!(buf.contains("InvalidMessage"));
    assert!(buf.contains("Shutdown"));

    // Player one makes their move as normal
    player_one_io.stdin.write_all(&[49, b'\n']).await.unwrap();

    // Player one receives Shutdown event
    player_one_io
        .assert_stdout_contains(&String::from(
            "An unrecoverable error has occurred, game terminating.",
        ))
        .await;
}

async fn assert_online_client_connections(
    player_one_io: &mut InputOutput<ChildStdin, BufReader<ChildStdout>>,
    player_two_io: &mut InputOutput<ChildStdin, BufReader<ChildStdout>>,
) {
    // Player one selects to host the game, specifying port 0
    player_one_io
        .assert_stdout_contains(&String::from(
            "Please select your game mode; local or online.",
        ))
        .await;
    player_one_io.write_string("online\n").await;
    player_one_io
        .assert_stdout_contains(&String::from("Do you want to host or join a game?"))
        .await;
    player_one_io.write_string("host\n").await;
    player_one_io
        .assert_stdout_contains(&String::from(
            "Do you wish to specify a port to bind to (the default is 22222) y/N?",
        ))
        .await;
    player_one_io.write_string("yes\n").await;
    player_one_io
        .assert_stdout_contains(&String::from("Please provide the port:"))
        .await;
    player_one_io.write_string("0\n").await;
    // Grab the bound port for later connecting, ensuring the port is always available
    let buf = player_one_io
        .assert_stdout_contains(&String::from("People can join you on port"))
        .await;
    let re = Regex::new(r"People can join you on port\s*(\d*)\s*!").unwrap();
    let port = re.captures(buf.as_str()).unwrap().get(1).unwrap().as_str();

    // Player two selects to join the game
    player_two_io
        .assert_stdout_contains(&String::from(
            "Please select your game mode; local or online.",
        ))
        .await;
    player_two_io.write_string("online\n").await;
    player_two_io
        .assert_stdout_contains(&String::from("Do you want to host or join a game?"))
        .await;
    player_two_io.write_string("join\n").await;
    player_two_io
        .assert_stdout_contains(&String::from(
            "Please enter the address of the game to join:",
        ))
        .await;
    player_two_io
        .write_string(format!("0.0.0.0:{}\n", port).as_str())
        .await;
}
