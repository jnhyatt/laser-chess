use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedSender};
use tracing::{error, info, warn};

use laser_chess::{
    ClientRequest, ServerMessage,
    logic::{Board, Move, Piece, Player},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init();

    // Create matchmaking channel
    let (matchmaking_tx, matchmaking_rx) = mpsc::unbounded_channel::<WebSocket>();

    // Start the matchmaking task
    tokio::spawn(matchmaking_loop(matchmaking_rx));

    // Build the router
    let app = Router::new()
        .route("/game", get(websocket_handler))
        .with_state(matchmaking_tx);

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    info!("Server running on http://0.0.0.0:3000");
    info!("WebSocket endpoint: ws://localhost:3000/game");

    axum::serve(listener, app).await?;

    Ok(())
}

// WebSocket handler that accepts connections and sends them to matchmaking.
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(matchmaking_tx): State<UnboundedSender<WebSocket>>,
) -> Result<Response, StatusCode> {
    Ok(ws.on_upgrade(move |socket| async move {
        info!("New WebSocket connection established");
        if let Err(e) = matchmaking_tx.send(socket) {
            error!("Failed to send connection to matchmaking: {}", e);
        }
    }))
}

struct ConnectedPlayer {
    connection: WebSocket,
    name: String,
}

/// Awaits a player connection, awaits a setup packet, then returns either the [`ConnectedPlayer`]
/// or the setup error.
async fn connect_player(mut connection: WebSocket) -> anyhow::Result<ConnectedPlayer> {
    match connection.recv().await {
        Some(Ok(Message::Text(text))) => {
            let setup: ClientRequest = serde_json::from_str(&text)?;
            match setup {
                ClientRequest::InitialSetup { player_name } => Ok(ConnectedPlayer {
                    connection,
                    name: player_name,
                }),
                _ => Err(anyhow::anyhow!(
                    "Expected InitialSetup message, got different message"
                )),
            }
        }
        Some(Ok(_)) => Err(anyhow::anyhow!(
            "Expected text message for setup, got different message"
        )),
        Some(Err(e)) => Err(anyhow::anyhow!("WebSocket error during setup: {}", e)),
        None => Err(anyhow::anyhow!("Connection closed during setup")),
    }
}

/// Matchmaking loop that pairs up players. When a player opens a connection to the server, it gets
/// tossed into the channel sender (matchmaking queue -- only two players long). This function just
/// reads pairs of players and starts a game for them by passing the websocket connections to the
/// game logic.
async fn matchmaking_loop(mut matchmaking_rx: mpsc::UnboundedReceiver<WebSocket>) {
    info!("Matchmaking loop started");

    loop {
        let player1 = match matchmaking_rx.recv().await {
            Some(conn) => {
                info!("Player 1 connected, awaiting setup");
                // spawn new task to await setup (connect_player)
                tokio::spawn(connect_player(conn))
            }
            None => {
                warn!("Matchmaking channel closed");
                break;
            }
        };
        let player2 = match matchmaking_rx.recv().await {
            Some(conn) => {
                info!("Player 2 connected, awaiting setup");
                // spawn new task to await setup (connect_player)
                tokio::spawn(connect_player(conn))
            }
            None => {
                warn!("Matchmaking channel closed");
                break;
            }
        };
        // await both
        let (player1, player2) = tokio::try_join!(player1, player2).unwrap();
        let (Ok(player1), Ok(player2)) = (player1, player2) else {
            info!("Player setup failed, restarting matchmaking");
            continue;
        };

        tokio::spawn(start_game([player1, player2]));
    }

    info!("Matchmaking loop ended");
}

async fn start_game([mut player1, mut player2]: [ConnectedPlayer; 2]) -> anyhow::Result<()> {
    info!(
        "Starting new game between {} and {}",
        player1.name, player2.name
    );

    let mut board_state = Board {
        cell: [[None; 8]; 8],
    };
    board_state.cell[0][4] = Some(Piece::king(Player::Player2));
    board_state.cell[7][3] = Some(Piece::king(Player::Player1));

    let player0_setup = player1.connection.send(Message::text(
        serde_json::to_string(&ServerMessage::InitialSetup {
            board: board_state.clone(),
            player_order: 0,
            opponent_name: player2.name.clone(),
        })
        .unwrap(),
    ));
    let player1_setup = player2.connection.send(Message::text(
        serde_json::to_string(&ServerMessage::InitialSetup {
            board: board_state,
            player_order: 1,
            opponent_name: player1.name.clone(),
        })
        .unwrap(),
    ));

    tokio::try_join!(player0_setup, player1_setup).unwrap();

    // Everything is officially set up!

    while !board_state.game_over() {
        // await player 1's move
        let player_move = loop {
            match client_request(&mut player1).await? {
                ClientRequest::Move(player_move) => {
                    match board_state.try_move(&player_move, Player::Player1) {
                        Ok(()) => break player_move,
                        Err(e) => {
                            warn!("Invalid move from player 1: {}", e);
                        }
                    }
                }
                _ => {
                    warn!("Expected Move message from player 1, got different message");
                }
            }
        };

        // notify other player, update board state
        player2
            .connection
            .send(Message::text(serde_json::to_string(
                &ServerMessage::OpponentMoved(player_move),
            )?))
            .await?;

        // TODO abstract over the duplicate code here
        if board_state.game_over() {
            break;
        }

        // await player 2's move
        let player_move = loop {
            match client_request(&mut player2).await? {
                ClientRequest::Move(player_move) => {
                    match board_state.try_move(&player_move, Player::Player2) {
                        Ok(()) => break player_move,
                        Err(e) => {
                            warn!("Invalid move from player 2: {}", e);
                        }
                    }
                }
                _ => {
                    warn!("Expected Move message from player 2, got different message");
                }
            }
        };

        // notify other player, update board state
        player1
            .connection
            .send(Message::text(serde_json::to_string(
                &ServerMessage::OpponentMoved(player_move),
            )?))
            .await?;
    }

    Ok(())
}

async fn client_request(player: &mut ConnectedPlayer) -> anyhow::Result<ClientRequest> {
    match player.connection.recv().await {
        Some(Ok(Message::Text(text))) => Ok(serde_json::from_str(&text)?),
        Some(Ok(_)) => Err(anyhow::anyhow!(
            "Expected text message for move, got different message"
        )),
        Some(Err(e)) => Err(anyhow::anyhow!("WebSocket error during game: {}", e)),
        None => Err(anyhow::anyhow!("Connection closed during game")),
    }
}

// Notify a player that their opponent disconnected
async fn _notify_disconnect(_player: &mut WebSocket, _message: &str) -> anyhow::Result<()> {
    // let disconnect_msg = GameMessage::GameOver {
    //     winner: None,
    //     reason: message.to_string(),
    // };

    // let json = serde_json::to_string(&disconnect_msg)?;
    // player.send(Message::Text(json.into())).await?;
    // player.close().await?;

    Ok(())
}
