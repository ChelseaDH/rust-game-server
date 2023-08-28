use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

struct InputOutput<I: AsyncWrite + Unpin, O: AsyncBufRead + Unpin> {
    stdin: I,
    stdout: O,
}

impl<I: AsyncWrite + Unpin, O: AsyncBufRead + Unpin> InputOutput<I, O> {
    fn new(stdin: I, stdout: O) -> InputOutput<I, O> {
        InputOutput { stdin, stdout }
    }

    async fn stream_contains(&mut self, string: &String) {
        let mut buf = String::new();

        loop {
            self.stdout.read_line(&mut buf).await.unwrap();
            if buf.contains(string) {
                return;
            }
        }
    }

    async fn assert_stdout_contains(&mut self, string: &String) {
        tokio::time::timeout(Duration::from_secs(1), self.stream_contains(string))
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

fn get_io() -> InputOutput<ChildStdin, BufReader<ChildStdout>> {
    // Set up command to run the program
    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    cmd.stdin(Stdio::piped()); // Open a pipe for stdin
    cmd.stdout(Stdio::piped()); // Open a pipe for stdout

    // Spawn the command and get handles to stdin and stdout
    let mut child = cmd.spawn().unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());

    InputOutput::new(stdin, stdout)
}

#[tokio::test]
async fn local_game_runs_until_win() {
    // Set up command to run the program
    let mut io = get_io();

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
    let mut player_one_io = get_io();
    let mut player_two_io = get_io();

    // Player one selects to host the game
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
    player_two_io.write_string("0.0.0.0:22222\n").await;

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
            "Error: The input should be a number between 0 and 8, try again.",
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
