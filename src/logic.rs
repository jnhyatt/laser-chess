use std::fmt;

use bevy_math::{CompassOctant, CompassQuadrant, USizeVec2, usizevec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Board {
    pub cell: [[Option<Piece>; 8]; 8],
}

impl Board {
    pub fn game_over(&self) -> bool {
        self.cell
            .iter()
            .flat_map(|row| row)
            .filter(|x| {
                matches!(
                    x,
                    Some(Piece {
                        kind: PieceKind::King,
                        ..
                    })
                )
            })
            .count()
            < 2
    }

    pub fn try_move_piece(
        mut self,
        player_move: &Move,
        player: Player,
    ) -> Result<Self, InvalidMove> {
        let piece =
            self.cell[player_move.from.y][player_move.from.x].ok_or(InvalidMove::NoPieceAtFrom)?;
        if piece.allegiance != player {
            return Err(InvalidMove::NotYourPiece);
        }
        match player_move.kind {
            MoveKind::Move(direction) => {
                let to = add_compass_octant(player_move.from, direction)
                    .ok_or(InvalidMove::OutOfBounds)?;
                if self.cell[to.y][to.x].is_some() {
                    return Err(InvalidMove::DestinationOccupied);
                }
                self.cell[to.y][to.x] = self.cell[player_move.from.y][player_move.from.x];
                self.cell[player_move.from.y][player_move.from.x] = None;
            }
            MoveKind::Rotate(chirality) => {
                let new_kind = match piece.kind {
                    PieceKind::King | PieceKind::Block { .. } => {
                        return Err(InvalidMove::CannotRotate);
                    }
                    PieceKind::OneSide(x) => PieceKind::OneSide(x.rotate(chirality)),
                    PieceKind::TwoSide(x) => PieceKind::TwoSide(x.rotate(chirality)),
                };
                self.cell[player_move.from.y][player_move.from.x] = Some(Piece {
                    kind: new_kind,
                    allegiance: piece.allegiance,
                });
            }
        }
        Ok(self)
    }

    pub fn try_move(&mut self, player_move: &Move, player: Player) -> Result<(), InvalidMove> {
        *self = self.try_move_piece(player_move, player)?;

        // Now shoot the laser and blow crap up!!!!
        let laser = match player {
            Player::Player1 => Laser {
                position: usizevec2(7, 0),
                direction: CompassQuadrant::North,
            },
            Player::Player2 => Laser {
                position: usizevec2(0, 7),
                direction: CompassQuadrant::South,
            },
        };
        if let Some((hit_coord, new_piece_state)) = self.bounce_laser(laser) {
            self.cell[hit_coord.y][hit_coord.x] = new_piece_state;
        }
        Ok(())
    }

    /// Raycast a laser in a straight line until it hits a wall (return None) or a piece (return Some).
    pub fn cast_laser(&self, laser: Laser) -> Option<(USizeVec2, Piece)> {
        self.cell[laser.position.y][laser.position.x]
            .map(|cell| (laser.position, cell))
            .or_else(|| self.cast_laser(laser.advance()?))
    }

    /// Bounce a laser off mirrors until it hits a wall (return None) or hits a piece (return Some).
    /// If the piece is hit, the piece's replacement is returned -- `None` if the piece was
    /// destroyed, or `Some(piece)` if the piece was changed (e.g., a stacked block losing its top
    /// block).
    pub fn bounce_laser(&self, laser: Laser) -> Option<(USizeVec2, Option<Piece>)> {
        let (hit_coord, hit_piece) = self.cast_laser(laser)?; // We hit the wall
        match hit_piece.reflect(laser.direction) {
            Ok(new_direction) => self.bounce_laser(
                Laser {
                    position: hit_coord,
                    direction: new_direction,
                }
                .advance()?,
            ),
            Err(new_piece_state) => Some((hit_coord, new_piece_state)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InvalidMove {
    OutOfBounds,
    NoPieceAtFrom,
    NotYourPiece,
    DestinationOccupied,
    CannotRotate,
}

impl fmt::Display for InvalidMove {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidMove::OutOfBounds => write!(f, "Move goes out of bounds"),
            InvalidMove::NoPieceAtFrom => write!(f, "No piece at 'from' position"),
            InvalidMove::NotYourPiece => write!(f, "The piece at 'from' does not belong to you"),
            InvalidMove::DestinationOccupied => {
                write!(f, "The destination cell is already occupied")
            }
            InvalidMove::CannotRotate => write!(f, "This piece cannot be rotated"),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Move {
    pub from: USizeVec2,
    pub kind: MoveKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MoveKind {
    Move(CompassOctant),
    Rotate(Chirality),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Chirality {
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Player {
    Player1,
    Player2,
}

impl Player {
    pub fn index(&self) -> usize {
        match self {
            Player::Player1 => 0,
            Player::Player2 => 1,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Player::Player1),
            1 => Some(Player::Player2),
            _ => None,
        }
    }

    pub fn opponent(&self) -> Self {
        match self {
            Player::Player1 => Player::Player2,
            Player::Player2 => Player::Player1,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: PieceKind,
    pub allegiance: Player,
}

impl Piece {
    pub fn king(allegiance: Player) -> Self {
        Self {
            kind: PieceKind::King,
            allegiance,
        }
    }

    pub fn block(allegiance: Player) -> Self {
        Self {
            kind: PieceKind::Block { stacked: true },
            allegiance,
        }
    }

    pub fn mirror(allegiance: Player, orientation: Orientation) -> Self {
        Self {
            kind: PieceKind::OneSide(orientation),
            allegiance,
        }
    }

    pub fn two_sided(allegiance: Player, orientation: Orientation) -> Self {
        Self {
            kind: PieceKind::TwoSide(orientation),
            allegiance,
        }
    }

    pub fn opposing(self) -> Self {
        Self {
            kind: self.kind.mirrored(),
            allegiance: self.allegiance.opponent(),
        }
    }

    /// Reflect a laser off this piece. Returns the new direction if reflected, or the new piece
    /// state if the laser did not hit a reflective surface.
    pub fn reflect(&self, direction: CompassQuadrant) -> Result<CompassQuadrant, Option<Self>> {
        match self.kind.reflect(direction) {
            Ok(new_direction) => Ok(new_direction),
            Err(destroyed_kind) => Err(destroyed_kind.map(|kind| Self {
                kind,
                allegiance: self.allegiance,
            })),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PieceKind {
    King,
    Block { stacked: bool },
    OneSide(Orientation),
    TwoSide(Orientation),
}

impl PieceKind {
    fn mirrored(self) -> Self {
        match self {
            x @ (PieceKind::King | PieceKind::Block { .. }) => x,
            PieceKind::OneSide(orientation) => PieceKind::OneSide(orientation.mirrored()),
            PieceKind::TwoSide(orientation) => PieceKind::TwoSide(orientation.mirrored()),
        }
    }

    fn reflect(&self, direction: CompassQuadrant) -> Result<CompassQuadrant, Option<Self>> {
        use CompassQuadrant::*;
        use Orientation::*;
        match (self, direction) {
            (Self::OneSide(NE), South) => Ok(East),
            (Self::OneSide(NE), West) => Ok(North),
            (Self::OneSide(NW), South) => Ok(West),
            (Self::OneSide(NW), East) => Ok(North),
            (Self::OneSide(SE), North) => Ok(East),
            (Self::OneSide(SE), West) => Ok(South),
            (Self::OneSide(SW), North) => Ok(West),
            (Self::OneSide(SW), East) => Ok(South),
            (Self::OneSide(_), _) => Err(None),

            (Self::TwoSide(NE | SW), South) => Ok(East),
            (Self::TwoSide(NE | SW), West) => Ok(North),
            (Self::TwoSide(NE | SW), North) => Ok(West),
            (Self::TwoSide(NE | SW), East) => Ok(South),
            (Self::TwoSide(NW | SE), South) => Ok(West),
            (Self::TwoSide(NW | SE), East) => Ok(North),
            (Self::TwoSide(NW | SE), North) => Ok(East),
            (Self::TwoSide(NW | SE), West) => Ok(South),

            (Self::Block { stacked: true }, _) => Err(Some(Self::Block { stacked: false })),
            (Self::Block { stacked: false }, _) => Err(None),
            (Self::King, _) => Err(None),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Orientation {
    NE,
    NW,
    SE,
    SW,
}

impl Orientation {
    fn mirrored(self) -> Self {
        use Orientation::*;
        match self {
            NE => SW,
            NW => SE,
            SE => NW,
            SW => NE,
        }
    }

    fn rotate(self, chirality: Chirality) -> Self {
        use Chirality::*;
        use Orientation::*;
        match (self, chirality) {
            (NE, Clockwise) => SE,
            (NE, CounterClockwise) => NW,
            (NW, Clockwise) => NE,
            (NW, CounterClockwise) => SW,
            (SE, Clockwise) => SW,
            (SE, CounterClockwise) => NE,
            (SW, Clockwise) => NW,
            (SW, CounterClockwise) => SE,
        }
    }
}

/// Describes where a laser is. It's a combination of a position and a direction.
#[derive(Clone, Copy, Debug)]
pub struct Laser {
    pub position: USizeVec2,
    pub direction: CompassQuadrant,
}

impl Laser {
    pub fn advance(self) -> Option<Self> {
        Some(Self {
            position: add_compass_quadrant(self.position, self.direction)?,
            direction: self.direction,
        })
    }
}

fn add_compass_quadrant(pos: USizeVec2, dir: CompassQuadrant) -> Option<USizeVec2> {
    match dir {
        CompassQuadrant::North => pos.y.checked_add(1).and_then(|y| {
            if y < 8 {
                Some(USizeVec2::new(pos.x, y))
            } else {
                None
            }
        }),
        CompassQuadrant::East => pos.x.checked_add(1).and_then(|x| {
            if x < 8 {
                Some(USizeVec2::new(x, pos.y))
            } else {
                None
            }
        }),
        CompassQuadrant::South => pos.y.checked_sub(1).map(|y| USizeVec2::new(pos.x, y)),
        CompassQuadrant::West => pos.x.checked_sub(1).map(|x| USizeVec2::new(x, pos.y)),
    }
}

pub fn add_compass_octant(pos: USizeVec2, dir: CompassOctant) -> Option<USizeVec2> {
    match dir {
        CompassOctant::North => pos.y.checked_add(1).and_then(|y| {
            if y < 8 {
                Some(USizeVec2::new(pos.x, y))
            } else {
                None
            }
        }),
        CompassOctant::NorthEast => pos.x.checked_add(1).and_then(|x| {
            pos.y.checked_add(1).and_then(|y| {
                if x < 8 && y < 8 {
                    Some(USizeVec2::new(x, y))
                } else {
                    None
                }
            })
        }),
        CompassOctant::East => pos.x.checked_add(1).and_then(|x| {
            if x < 8 {
                Some(USizeVec2::new(x, pos.y))
            } else {
                None
            }
        }),
        CompassOctant::SouthEast => pos.x.checked_add(1).and_then(|x| {
            pos.y.checked_sub(1).and_then(|y| {
                if x < 8 {
                    Some(USizeVec2::new(x, y))
                } else {
                    None
                }
            })
        }),
        CompassOctant::South => pos.y.checked_sub(1).map(|y| USizeVec2::new(pos.x, y)),
        CompassOctant::SouthWest => pos
            .x
            .checked_sub(1)
            .and_then(|x| pos.y.checked_sub(1).map(|y| USizeVec2::new(x, y))),
        CompassOctant::West => pos.x.checked_sub(1).map(|x| USizeVec2::new(x, pos.y)),
        CompassOctant::NorthWest => pos.x.checked_sub(1).and_then(|x| {
            pos.y.checked_add(1).and_then(|y| {
                if y < 8 {
                    Some(USizeVec2::new(x, y))
                } else {
                    None
                }
            })
        }),
    }
}
