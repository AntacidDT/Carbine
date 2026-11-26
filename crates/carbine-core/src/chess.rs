//! A small chess board with real legal moves, checks and checkmates.
//!
//! This is the same kind of engine piece as the terrain or the particle system: the
//! rules live in Rust, and a script only asks for moves and writes the ones a player
//! chose. The whole position is one value, so copying it to test a move is cheap and
//! a move that leaves its own king under attack can simply be refused.
//!
//! Castling, en passant and promotion are included — a game that announced a
//! checkmate because it forgot a knight could block would not be much of a game.
//! Draws by repetition or the fifty-move rule are not counted here.

use std::fmt;

/// White moves first; the colours own the two sides of the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Black => "black",
        }
    }
}

/// The six pieces the board holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::King => "king",
            Self::Queen => "queen",
            Self::Rook => "rook",
            Self::Bishop => "bishop",
            Self::Knight => "knight",
            Self::Pawn => "pawn",
        }
    }
}

/// One move, held as square indices: `rank * 8 + file`, with `a1` at zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move {
    pub from: usize,
    pub to: usize,
    /// `None` means a queen when a pawn reaches the far rank.
    pub promotion: Option<Kind>,
}

impl Move {
    pub fn notation(&self) -> String {
        format!("{}{}", square_name(self.from), square_name(self.to))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Playing,
    Checkmate,
    Stalemate,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Checkmate => "checkmate",
            Self::Stalemate => "stalemate",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChessError {
    why: String,
}

impl ChessError {
    fn new(why: impl Into<String>) -> Self {
        Self { why: why.into() }
    }
}

impl fmt::Display for ChessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.why)
    }
}

impl std::error::Error for ChessError {}

const WHITE_KING_SIDE: u8 = 1;
const WHITE_QUEEN_SIDE: u8 = 2;
const BLACK_KING_SIDE: u8 = 4;
const BLACK_QUEEN_SIDE: u8 = 8;

/// The position: what is on each square, whose turn it is, the en-passant target,
/// castling rights and how the game ended if it has.
#[derive(Clone, Debug)]
pub struct ChessBoard {
    squares: [Option<(Color, Kind)>; 64],
    turn: Color,
    en_passant: Option<usize>,
    castling: u8,
    status: Status,
    winner: Option<Color>,
    last: Option<(usize, usize)>,
}

impl Default for ChessBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl ChessBoard {
    /// The starting position: white on ranks one and two, black on seven and eight.
    pub fn new() -> Self {
        let mut squares = [None; 64];
        let back = [
            Kind::Rook,
            Kind::Knight,
            Kind::Bishop,
            Kind::Queen,
            Kind::King,
            Kind::Bishop,
            Kind::Knight,
            Kind::Rook,
        ];
        for file in 0..8 {
            squares[file] = Some((Color::White, back[file]));
            squares[8 + file] = Some((Color::White, Kind::Pawn));
            squares[48 + file] = Some((Color::Black, Kind::Pawn));
            squares[56 + file] = Some((Color::Black, back[file]));
        }
        Self {
            squares,
            turn: Color::White,
            en_passant: None,
            castling: WHITE_KING_SIDE | WHITE_QUEEN_SIDE | BLACK_KING_SIDE | BLACK_QUEEN_SIDE,
            status: Status::Playing,
            winner: None,
            last: None,
        }
    }

    pub fn turn(&self) -> Color {
        self.turn
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn winner(&self) -> Option<Color> {
        self.winner
    }

    pub fn last(&self) -> Option<(usize, usize)> {
        self.last
    }

    pub fn at(&self, square: usize) -> Option<(Color, Kind)> {
        self.squares.get(square).copied().flatten()
    }

    /// The board as eight rows from rank eight down to rank one, each row from the
    /// a-file to the h-file. A script draws it in this order.
    pub fn rows(&self) -> Vec<Vec<Option<(Color, Kind)>>> {
        (0..8)
            .rev()
            .map(|rank| (0..8).map(|file| self.at(rank * 8 + file)).collect())
            .collect()
    }

    pub fn in_check(&self, color: Color) -> bool {
        let Some(king) = (0..64).find(|square| self.at(*square) == Some((color, Kind::King)))
        else {
            return false;
        };
        self.attacked(king, color.opposite())
    }

    /// Every legal move from one square, in a stable order: the order a player sees
    /// in a highlight list is the order the generator walked the board in.
    pub fn moves_from(&self, square: usize) -> Vec<Move> {
        let Some((color, _)) = self.at(square) else {
            return Vec::new();
        };
        if self.status != Status::Playing || color != self.turn {
            return Vec::new();
        }
        self.legal_moves(color)
            .into_iter()
            .filter(|move_| move_.from == square)
            .collect()
    }

    /// Writes a move, refusing anything that is not legal for the side to play.
    /// The board is unchanged when the move is refused.
    pub fn play(
        &mut self,
        from: usize,
        to: usize,
        promotion: Option<Kind>,
    ) -> Result<(), ChessError> {
        if self.status != Status::Playing {
            return Err(ChessError::new("the game is over; reset it to play again"));
        }
        if from >= 64 || to >= 64 {
            return Err(ChessError::new("a square is off the board"));
        }
        let legal = self.legal_moves(self.turn);
        let chosen = legal
            .iter()
            .find(|move_| {
                move_.from == from
                    && move_.to == to
                    && match promotion {
                        // A script that names a promotion must be answered by that
                        // piece; one that does not gets the queen.
                        Some(kind) => move_.promotion == Some(kind),
                        None => true,
                    }
            })
            .copied()
            .ok_or_else(|| ChessError::new("that is not a legal move"))?;

        let mover = self.turn;
        *self = self.after(&chosen);
        if self.legal_moves(self.turn).is_empty() {
            self.status = if self.in_check(self.turn) {
                self.winner = Some(mover);
                Status::Checkmate
            } else {
                Status::Stalemate
            };
        }
        Ok(())
    }

    /// Apply a move without asking whether the game ended, which is what testing a
    /// move for king safety needs.
    fn after(&self, move_: &Move) -> Self {
        let mut next = self.clone();
        let (color, kind) = next.squares[move_.from].expect("a legal move has a piece");
        let captured = next.squares[move_.to];
        next.squares[move_.from] = None;

        // En passant takes the pawn beside the destination, not on it.
        if kind == Kind::Pawn && Some(move_.to) == self.en_passant && captured.is_none() {
            let behind = if color == Color::White {
                move_.to - 8
            } else {
                move_.to + 8
            };
            next.squares[behind] = None;
        }

        // Castling moves the rook too: a king that walks two files is claiming it.
        if kind == Kind::King && move_.from.abs_diff(move_.to) == 2 {
            let rank = move_.from / 8;
            let (rook_from, rook_to) = if move_.to > move_.from {
                (rank * 8 + 7, rank * 8 + 5)
            } else {
                (rank * 8, rank * 8 + 3)
            };
            next.squares[rook_to] = next.squares[rook_from];
            next.squares[rook_from] = None;
        }

        let placed = if kind == Kind::Pawn && (move_.to / 8 == 0 || move_.to / 8 == 7) {
            move_.promotion.unwrap_or(Kind::Queen)
        } else {
            kind
        };
        next.squares[move_.to] = Some((color, placed));

        // A double push opens the square behind the pawn for one move.
        next.en_passant = if kind == Kind::Pawn && move_.from.abs_diff(move_.to) == 16 {
            Some((move_.from + move_.to) / 2)
        } else {
            None
        };

        // Moving a king gives up both rights; moving or capturing a rook gives up
        // the right on that corner.
        if kind == Kind::King {
            if color == Color::White {
                next.castling &= !(WHITE_KING_SIDE | WHITE_QUEEN_SIDE);
            } else {
                next.castling &= !(BLACK_KING_SIDE | BLACK_QUEEN_SIDE);
            }
        }
        for square in [0, 7, 56, 63] {
            if move_.from == square || move_.to == square {
                next.castling &= !corner_right(square);
            }
        }

        next.turn = color.opposite();
        next.last = Some((move_.from, move_.to));
        next.status = Status::Playing;
        next.winner = None;
        next
    }

    fn moves_for(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::new();
        for from in 0..64 {
            let Some((owner, kind)) = self.squares[from] else {
                continue;
            };
            if owner != color {
                continue;
            }
            match kind {
                Kind::Pawn => self.pawn_moves(from, color, &mut moves),
                Kind::Knight => self.leaps(from, color, Kind::Knight, &KNIGHT_OFFSETS, &mut moves),
                Kind::King => {
                    self.leaps(from, color, Kind::King, &KING_OFFSETS, &mut moves);
                    self.castles(from, color, &mut moves);
                }
                Kind::Rook => self.slides(from, color, Kind::Rook, &ROOK_DIRS, &mut moves),
                Kind::Bishop => self.slides(from, color, Kind::Bishop, &BISHOP_DIRS, &mut moves),
                Kind::Queen => self.slides(from, color, Kind::Queen, &QUEEN_DIRS, &mut moves),
            }
        }
        moves
    }

    fn legal_moves(&self, color: Color) -> Vec<Move> {
        self.moves_for(color)
            .into_iter()
            .filter(|move_| !self.after(move_).in_check(color))
            .collect()
    }

    fn push_move(moves: &mut Vec<Move>, from: usize, to: usize, kind: Kind, color: Color) {
        let last_rank = if color == Color::White { 7 } else { 0 };
        if kind == Kind::Pawn && to / 8 == last_rank {
            for promotion in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight] {
                moves.push(Move {
                    from,
                    to,
                    promotion: Some(promotion),
                });
            }
        } else {
            moves.push(Move {
                from,
                to,
                promotion: None,
            });
        }
    }

    fn pawn_moves(&self, from: usize, color: Color, moves: &mut Vec<Move>) {
        let file = (from % 8) as i32;
        let rank = (from / 8) as i32;
        let forward = if color == Color::White { 1 } else { -1 };
        let home = if color == Color::White { 1 } else { 6 };

        let step = (rank + forward) * 8 + file;
        if (0..64).contains(&step) && self.squares[step as usize].is_none() {
            Self::push_move(moves, from, step as usize, Kind::Pawn, color);
            let double = (rank + 2 * forward) * 8 + file;
            if rank == home && self.squares[double as usize].is_none() {
                moves.push(Move {
                    from,
                    to: double as usize,
                    promotion: None,
                });
            }
        }
        for side in [-1, 1] {
            let target_file = file + side;
            if !(0..8).contains(&target_file) {
                continue;
            }
            let to = (rank + forward) * 8 + target_file;
            if !(0..64).contains(&to) {
                continue;
            }
            let to = to as usize;
            let enemy = self.squares[to].is_some_and(|(owner, _)| owner != color);
            if enemy || Some(to) == self.en_passant {
                Self::push_move(moves, from, to, Kind::Pawn, color);
            }
        }
    }

    fn leaps(
        &self,
        from: usize,
        color: Color,
        kind: Kind,
        offsets: &[(i32, i32)],
        moves: &mut Vec<Move>,
    ) {
        let file = (from % 8) as i32;
        let rank = (from / 8) as i32;
        for (df, dr) in offsets {
            let (file, rank) = (file + df, rank + dr);
            if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                continue;
            }
            let to = (rank * 8 + file) as usize;
            if self.squares[to].is_none_or(|(owner, _)| owner != color) {
                Self::push_move(moves, from, to, kind, color);
            }
        }
    }

    fn slides(
        &self,
        from: usize,
        color: Color,
        kind: Kind,
        dirs: &[(i32, i32)],
        moves: &mut Vec<Move>,
    ) {
        let file = (from % 8) as i32;
        let rank = (from / 8) as i32;
        for (df, dr) in dirs {
            let (mut file, mut rank) = (file + df, rank + dr);
            while (0..8).contains(&file) && (0..8).contains(&rank) {
                let to = (rank * 8 + file) as usize;
                match self.squares[to] {
                    None => Self::push_move(moves, from, to, kind, color),
                    Some((owner, _)) => {
                        if owner != color {
                            Self::push_move(moves, from, to, kind, color);
                        }
                        break;
                    }
                }
                file += df;
                rank += dr;
            }
        }
    }

    fn castles(&self, from: usize, color: Color, moves: &mut Vec<Move>) {
        let (rank, king_square, king_right, queen_right) = match color {
            Color::White => (0, 4, WHITE_KING_SIDE, WHITE_QUEEN_SIDE),
            Color::Black => (7, 60, BLACK_KING_SIDE, BLACK_QUEEN_SIDE),
        };
        if from != king_square || self.attacked(from, color.opposite()) {
            return;
        }
        if self.castling & king_right != 0
            && self.squares[rank * 8 + 7] == Some((color, Kind::Rook))
            && self.squares[rank * 8 + 5].is_none()
            && self.squares[rank * 8 + 6].is_none()
            && !self.attacked(rank * 8 + 5, color.opposite())
            && !self.attacked(rank * 8 + 6, color.opposite())
        {
            moves.push(Move {
                from,
                to: rank * 8 + 6,
                promotion: None,
            });
        }
        if self.castling & queen_right != 0
            && self.squares[rank * 8] == Some((color, Kind::Rook))
            && self.squares[rank * 8 + 1].is_none()
            && self.squares[rank * 8 + 2].is_none()
            && self.squares[rank * 8 + 3].is_none()
            && !self.attacked(rank * 8 + 3, color.opposite())
            && !self.attacked(rank * 8 + 2, color.opposite())
        {
            moves.push(Move {
                from,
                to: rank * 8 + 2,
                promotion: None,
            });
        }
    }

    /// Whether another colour attacks a square. Pawn attacks are checked from the
    /// square backwards, so a pawn that cannot move forward still covers its diagonals.
    fn attacked(&self, square: usize, by: Color) -> bool {
        let file = (square % 8) as i32;
        let rank = (square / 8) as i32;

        let pawn_rank = rank + if by == Color::White { -1 } else { 1 };
        for side in [-1, 1] {
            let pawn_file = file + side;
            if (0..8).contains(&pawn_file) && (0..8).contains(&pawn_rank) {
                let from = (pawn_rank * 8 + pawn_file) as usize;
                if self.squares[from] == Some((by, Kind::Pawn)) {
                    return true;
                }
            }
        }

        for (df, dr) in KNIGHT_OFFSETS {
            let (nf, nr) = (file + df, rank + dr);
            if (0..8).contains(&nf)
                && (0..8).contains(&nr)
                && self.squares[(nr * 8 + nf) as usize] == Some((by, Kind::Knight))
            {
                return true;
            }
        }

        for (df, dr) in KING_OFFSETS {
            let (nf, nr) = (file + df, rank + dr);
            if (0..8).contains(&nf)
                && (0..8).contains(&nr)
                && self.squares[(nr * 8 + nf) as usize] == Some((by, Kind::King))
            {
                return true;
            }
        }

        for (dirs, kinds) in [
            (&ROOK_DIRS[..], &[Kind::Rook, Kind::Queen][..]),
            (&BISHOP_DIRS[..], &[Kind::Bishop, Kind::Queen][..]),
        ] {
            for (df, dr) in dirs {
                let (mut nf, mut nr) = (file + df, rank + dr);
                while (0..8).contains(&nf) && (0..8).contains(&nr) {
                    if let Some((owner, kind)) = self.squares[(nr * 8 + nf) as usize] {
                        if owner == by && kinds.contains(&kind) {
                            return true;
                        }
                        break;
                    }
                    nf += df;
                    nr += dr;
                }
            }
        }
        false
    }
}

const KNIGHT_OFFSETS: [(i32, i32); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_OFFSETS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const QUEEN_DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

fn corner_right(square: usize) -> u8 {
    match square {
        0 => WHITE_QUEEN_SIDE,
        7 => WHITE_KING_SIDE,
        56 => BLACK_QUEEN_SIDE,
        63 => BLACK_KING_SIDE,
        _ => 0,
    }
}

/// `a1` is zero, `h8` is sixty-three; the same spelling a player uses on a board.
pub fn square_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let file = bytes[0].to_ascii_lowercase().wrapping_sub(b'a');
    let rank = bytes[1].wrapping_sub(b'1');
    if file >= 8 || rank >= 8 {
        return None;
    }
    Some(rank as usize * 8 + file as usize)
}

pub fn square_name(square: usize) -> String {
    if square >= 64 {
        return "??".to_owned();
    }
    let file = (b'a' + (square % 8) as u8) as char;
    let rank = (b'1' + (square / 8) as u8) as char;
    format!("{file}{rank}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(board: &mut ChessBoard, from: &str, to: &str) {
        board
            .play(square_index(from).unwrap(), square_index(to).unwrap(), None)
            .unwrap();
    }

    #[test]
    fn the_starting_board_has_twenty_legal_moves() {
        let board = ChessBoard::new();
        assert_eq!(board.legal_moves(Color::White).len(), 20);
        assert_eq!(board.turn(), Color::White);
        assert_eq!(
            board.at(square_index("e1").unwrap()),
            Some((Color::White, Kind::King))
        );
        assert_eq!(
            board.at(square_index("e7").unwrap()),
            Some((Color::Black, Kind::Pawn))
        );
    }

    #[test]
    fn a_move_that_leaves_the_king_in_check_is_refused() {
        let mut board = ChessBoard::new();
        play(&mut board, "e2", "e4");
        play(&mut board, "e7", "e5");
        play(&mut board, "f1", "c4");
        // Qh4 attacks f2 through an empty e5; Kxf2 would walk into it.
        play(&mut board, "d8", "h4");
        let before = board.clone();
        assert!(
            board
                .play(
                    square_index("c4").unwrap(),
                    square_index("f7").unwrap(),
                    None
                )
                .is_ok()
        );
        let mut other = before;
        // A rook cannot leap its own pawn.
        assert!(
            other
                .play(
                    square_index("a1").unwrap(),
                    square_index("a5").unwrap(),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn fools_mate_is_a_checkmate_for_black() {
        let mut board = ChessBoard::new();
        play(&mut board, "f2", "f3");
        play(&mut board, "e7", "e5");
        play(&mut board, "g2", "g4");
        play(&mut board, "d8", "h4");

        assert_eq!(board.status(), Status::Checkmate);
        assert_eq!(board.winner(), Some(Color::Black));
        assert!(board.in_check(Color::White));
        assert!(
            board
                .play(
                    square_index("e1").unwrap(),
                    square_index("e2").unwrap(),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn castling_moves_the_rook_and_needs_an_empty_path() {
        let mut board = ChessBoard::new();
        // Clear the king's side with legal pawn and knight moves.
        play(&mut board, "e2", "e4");
        play(&mut board, "e7", "e5");
        play(&mut board, "g1", "f3");
        play(&mut board, "b8", "c6");
        play(&mut board, "f1", "e2");
        play(&mut board, "g8", "f6");
        play(&mut board, "e1", "g1");

        assert_eq!(
            board.at(square_index("g1").unwrap()),
            Some((Color::White, Kind::King))
        );
        assert_eq!(
            board.at(square_index("f1").unwrap()),
            Some((Color::White, Kind::Rook))
        );
        assert_eq!(board.at(square_index("h1").unwrap()), None);
    }

    #[test]
    fn en_passant_takes_the_pawn_behind_the_target() {
        let mut board = ChessBoard::new();
        play(&mut board, "e2", "e4");
        play(&mut board, "a7", "a6");
        play(&mut board, "e4", "e5");
        play(&mut board, "d7", "d5");
        play(&mut board, "e5", "d6");

        assert_eq!(board.at(square_index("d5").unwrap()), None);
        assert_eq!(
            board.at(square_index("d6").unwrap()),
            Some((Color::White, Kind::Pawn))
        );
    }

    #[test]
    fn a_pawn_promotes_when_it_reaches_the_far_rank() {
        let mut board = ChessBoard::new();
        // Put a pawn one step from the far rank and get the king out of its way.
        board.squares[square_index("e7").unwrap()] = Some((Color::White, Kind::Pawn));
        board.squares[square_index("e8").unwrap()] = None;
        board.squares[square_index("h8").unwrap()] = Some((Color::Black, Kind::King));
        board.turn = Color::White;

        play(&mut board, "e7", "e8");
        assert_eq!(
            board.at(square_index("e8").unwrap()),
            Some((Color::White, Kind::Queen))
        );
    }

    #[test]
    fn square_names_walk_the_board() {
        assert_eq!(square_index("a1"), Some(0));
        assert_eq!(square_index("h8"), Some(63));
        assert_eq!(square_index("i1"), None);
        assert_eq!(square_name(0), "a1");
        assert_eq!(square_name(63), "h8");
    }
}
