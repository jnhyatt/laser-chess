use serde::{Deserialize, Serialize};

use crate::logic::{Board, Move};

pub mod logic;

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientRequest {
    InitialSetup { player_name: String },
    Move(Move),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMessage {
    InitialSetup {
        board: Board,
        player_order: usize,
        opponent_name: String,
    },
    OpponentMoved(Move),
}
