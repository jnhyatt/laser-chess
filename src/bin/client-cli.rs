use std::io::{self, Write};

use bevy_math::{Dir2, USizeVec2, usizevec2};
use futures_util::{SinkExt, StreamExt};
use laser_chess::{
    ClientRequest, ServerMessage,
    logic::{Board, Chirality, Move, MoveKind, PieceKind, Player},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() {
    println!("🎮 Laser Chess Debug Client");
    println!("=============================");

    // Get player name
    let player_name = prompt_for_input("Enter your username: ");

    // Connect to server
    println!("📡 Connecting to ws://localhost:3000/game...");

    let (ws_stream, _) = match connect_async("ws://localhost:3000/game").await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("❌ Failed to connect: {}", e);
            return;
        }
    };

    println!("✅ Connected!");

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Send initial setup
    let setup_msg = ClientRequest::InitialSetup {
        player_name: player_name.clone(),
    };

    let setup_json = serde_json::to_string(&setup_msg).unwrap();
    ws_sender
        .send(Message::Text(setup_json.into()))
        .await
        .unwrap();

    println!("📨 Sent setup with username: {}", player_name);
    println!("⏳ Waiting for game to start...");

    // Await initial setup from server
    let (mut board, opponent_name, player_order) = {
        loop {
            let Some(Ok(Message::Text(text))) = ws_receiver.next().await else {
                eprintln!("❌ Server closed connection");
                return;
            };
            if let Ok(ServerMessage::InitialSetup {
                board: initial_board,
                player_order,
                opponent_name,
            }) = serde_json::from_str::<ServerMessage>(&text)
            {
                break (
                    initial_board,
                    opponent_name,
                    Player::from_index(player_order).unwrap(),
                );
            } else {
                return;
            }
        }
    };

    display_board(&board, &opponent_name, player_order == Player::Player1);

    // If we go first, do one turn before jumping into the loop (loop handles opponent first)
    if player_order == Player::Player1 {
        ws_sender
            .send(player_turn(&mut board, player_order, &opponent_name))
            .await
            .unwrap();
    }

    // Repeatedly await opponent move, then prompt for and send player move
    loop {
        let message = ws_receiver.next().await.unwrap().unwrap();
        board
            .try_move(&opponent_turn(message), player_order.opponent())
            .unwrap();
        if board.game_over() {
            break;
        }

        display_board(&board, &opponent_name, true);

        ws_sender
            .send(player_turn(&mut board, player_order, &opponent_name))
            .await
            .unwrap();
        if board.game_over() {
            break;
        }
    }

    println!("🏁 Game over! Thanks for playing.");
}

fn display_board(board: &Board, opp_name: &str, my_turn: bool) {
    println!("\n  Current Board:");
    println!("    A B C D E F G H");
    for (y, row) in board.cell.iter().enumerate() {
        print!(" {} ", 8 - y);
        for cell in row {
            match cell {
                None => print!(" ."),
                Some(piece) => {
                    let symbol = match (&piece.kind, &piece.allegiance) {
                        (PieceKind::King, Player::Player1) => "♔",
                        (PieceKind::King, Player::Player2) => "♚",
                        (PieceKind::Block { stacked: false }, Player::Player1) => "□",
                        (PieceKind::Block { stacked: false }, Player::Player2) => "■",
                        (PieceKind::Block { stacked: true }, Player::Player1) => "⧠",
                        (PieceKind::Block { stacked: true }, Player::Player2) => "⧛",
                        (PieceKind::OneSide(_), Player::Player1) => "◢",
                        (PieceKind::OneSide(_), Player::Player2) => "◥",
                        (PieceKind::TwoSide(_), Player::Player1) => "◊",
                        (PieceKind::TwoSide(_), Player::Player2) => "♦",
                    };
                    print!(" {}", symbol);
                }
            }
        }
        println!();
    }
    println!();

    let turn_indicator = if my_turn {
        "Your".into()
    } else {
        format!("{}'s", opp_name)
    };
    println!("  {} turn", turn_indicator);
}

fn parse_coordinate(coord: &str) -> Option<USizeVec2> {
    if coord.len() != 2 {
        return None;
    }

    let mut chars = coord.chars();
    let col_char = chars.next()?.to_ascii_uppercase();
    let row_char = chars.next()?;

    let col = match col_char {
        'A' => 0,
        'B' => 1,
        'C' => 2,
        'D' => 3,
        'E' => 4,
        'F' => 5,
        'G' => 6,
        'H' => 7,
        _ => return None,
    };

    let row = match row_char {
        '1' => 7,
        '2' => 6,
        '3' => 5,
        '4' => 4,
        '5' => 3,
        '6' => 2,
        '7' => 1,
        '8' => 0,
        _ => return None,
    };

    Some(usizevec2(col, row))
}

fn format_coord(coord: USizeVec2) -> String {
    let col = char::from(b'A' + coord.x as u8);
    let row = 8 - coord.y;
    format!("{}{}", col, row)
}

fn prompt_for_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn parse_move_input(input: &str) -> Option<Move> {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();

    if parts.len() != 2 {
        println!("  Invalid format. Use: E1 E2 (move) or E1 L/R (rotate)");
        return None;
    }

    let from = parse_coordinate(parts[0])?;

    match parts[1].to_uppercase().as_str() {
        "L" => Some(Move {
            from,
            kind: MoveKind::Rotate(Chirality::CounterClockwise),
        }),
        "R" => Some(Move {
            from,
            kind: MoveKind::Rotate(Chirality::Clockwise),
        }),
        coord => {
            // Try to parse as coordinate (move to position)
            if let Some(to) = parse_coordinate(coord) {
                if to.chebyshev_distance(from) != 1 {
                    println!("  Invalid move: destination must be adjacent to source");
                    return None;
                }
                Some(Move {
                    from,
                    kind: MoveKind::Move(
                        Dir2::try_from(to.as_vec2() - from.as_vec2())
                            .unwrap() // We checked chebyshev distance is not zero
                            .into(),
                    ),
                })
            } else {
                println!("  Invalid destination: {}", coord);
                None
            }
        }
    }
}

fn player_turn(board: &mut Board, player_order: Player, opponent_name: &str) -> Message {
    loop {
        let player_move = prompt_move();
        // Validate move locally before sending
        if board.try_move(&player_move, player_order).is_ok() {
            // Send move to server
            let move_msg = ClientRequest::Move(player_move);
            let move_json = serde_json::to_string(&move_msg).unwrap();

            // Update local board state
            display_board(&board, opponent_name, false);
            break Message::text(move_json);
        } else {
            println!("❌ Invalid move, please try again.");
        }
    }
}

fn opponent_turn(msg: Message) -> Move {
    loop {
        let msg = msg.to_text().unwrap();
        let Ok(ServerMessage::OpponentMoved(opponent_move)) =
            serde_json::from_str::<ServerMessage>(msg)
        else {
            eprintln!("❌ Expected OpponentMoved message, got different message");
            continue;
        };
        let move_kind = match opponent_move.kind {
            MoveKind::Move(_) => "→ (moved)".to_string(),
            MoveKind::Rotate(Chirality::Clockwise) => "↻ (rotated clockwise)".to_string(),
            MoveKind::Rotate(Chirality::CounterClockwise) => {
                "↺ (rotated counter-clockwise)".to_string()
            }
        };
        println!(
            "📨 Opponent moved: {} {}",
            format_coord(opponent_move.from),
            move_kind
        );
        break opponent_move;
    }
}

fn prompt_move() -> Move {
    println!("💭 Your turn! Enter your move:");
    println!("   Format: FROM TO   (e.g., E1 E2 to move from E1 to E2)");
    println!("   Format: FROM L/R  (e.g., E1 L to rotate piece at E1 counter-clockwise)");
    print!("🎯 Move: ");
    io::stdout().flush().unwrap();

    loop {
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            if let Some(player_move) = parse_move_input(&input) {
                break player_move;
            }
        }
    }
}
