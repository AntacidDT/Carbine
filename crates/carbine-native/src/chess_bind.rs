//! `carbine.chess()` — a board whose rules live in the core.
//!
//! A script asks for legal moves and writes back the one a player chose. Nothing about
//! a square, a king or a knight is decided here; this class only turns Python strings
//! into square indices and Python errors out of the core's refusals.

use std::sync::Mutex;

use carbine_core::{ChessBoard, ChessError, Color, PieceKind, square_index, square_name};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A chess game. Several can exist at once; each owns its own board.
#[pyclass(module = "carbine", name = "Chess")]
pub struct PyChess {
    board: Mutex<ChessBoard>,
}

impl PyChess {
    pub(crate) fn new() -> Self {
        Self {
            board: Mutex::new(ChessBoard::new()),
        }
    }
}

fn refused(error: ChessError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn wanted_color(name: Option<&str>) -> PyResult<Color> {
    match name.map(str::to_ascii_lowercase).as_deref() {
        None | Some("white") => Ok(Color::White),
        Some("black") => Ok(Color::Black),
        Some(other) => Err(PyValueError::new_err(format!(
            "\"{other}\" is not a colour; say white or black"
        ))),
    }
}

fn wanted_piece(name: Option<&str>) -> PyResult<Option<PieceKind>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let found = [
        PieceKind::Queen,
        PieceKind::Rook,
        PieceKind::Bishop,
        PieceKind::Knight,
    ]
    .into_iter()
    .find(|kind| kind.name() == name.to_ascii_lowercase());
    found
        .map(Some)
        .ok_or_else(|| PyValueError::new_err("a promotion is queen, rook, bishop or knight"))
}

#[pymethods]
impl PyChess {
    /// Puts every piece back where it started and hands the move to white.
    fn reset(&self) {
        *self.board.lock().unwrap() = ChessBoard::new();
    }

    /// The board as eight rows from rank eight down, each square either `None` or a
    /// `(colour, piece)` pair — the order a script paints it in.
    fn board(&self) -> Vec<Vec<Option<(String, String)>>> {
        self.board
            .lock()
            .unwrap()
            .rows()
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|square| {
                        square
                            .map(|(color, kind)| (color.name().to_owned(), kind.name().to_owned()))
                    })
                    .collect()
            })
            .collect()
    }

    #[getter]
    fn turn(&self) -> String {
        self.board.lock().unwrap().turn().name().to_owned()
    }

    #[getter]
    fn status(&self) -> String {
        self.board.lock().unwrap().status().name().to_owned()
    }

    #[getter]
    fn winner(&self) -> Option<String> {
        self.board
            .lock()
            .unwrap()
            .winner()
            .map(|color| color.name().to_owned())
    }

    #[getter]
    fn last(&self) -> Option<String> {
        self.board
            .lock()
            .unwrap()
            .last()
            .map(|(from, to)| format!("{}{}", square_name(from), square_name(to)))
    }

    /// Whether a colour's king is attacked; left alone it asks about the player
    /// whose turn it is.
    #[pyo3(name = "inCheck")]
    #[pyo3(signature = (colour=None))]
    fn in_check(&self, colour: Option<&str>) -> PyResult<bool> {
        let color = wanted_color(colour)?;
        Ok(self.board.lock().unwrap().in_check(color))
    }

    /// The moves a square may legally make, as `e2e4` strings. A promotion carries
    /// its piece: `e7e8q`.
    fn moves(&self, square: &str) -> PyResult<Vec<String>> {
        let from = square_index(square).ok_or_else(|| {
            PyValueError::new_err(format!("\"{square}\" is not a square on a chessboard"))
        })?;
        Ok(self
            .board
            .lock()
            .unwrap()
            .moves_from(from)
            .into_iter()
            .map(|move_| match move_.promotion {
                Some(kind) => format!(
                    "{}{}{}",
                    square_name(move_.from),
                    square_name(move_.to),
                    kind.name().chars().next().unwrap()
                ),
                None => move_.notation(),
            })
            .collect())
    }

    /// Writes a move, refusing one that is not legal with `ValueError`. `a2a4` is
    /// enough for most moves; a promotion takes `e7e8q` or a `promotion` name.
    #[pyo3(signature = (from, to, promotion=None))]
    fn play(&self, from: &str, to: &str, promotion: Option<&str>) -> PyResult<()> {
        let from = square_index(from).ok_or_else(|| {
            PyValueError::new_err(format!("\"{from}\" is not a square on a chessboard"))
        })?;
        let to = square_index(to).ok_or_else(|| {
            PyValueError::new_err(format!("\"{to}\" is not a square on a chessboard"))
        })?;
        let promotion = wanted_piece(promotion)?;
        self.board
            .lock()
            .unwrap()
            .play(from, to, promotion)
            .map_err(refused)
    }

    fn __repr__(&self) -> String {
        let board = self.board.lock().unwrap();
        format!(
            "<Chess {} to move, {}>",
            board.turn().name(),
            board.status().name()
        )
    }
}
