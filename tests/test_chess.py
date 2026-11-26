"""`carbine.chess()` — the board in Rust, driven from Python.

A mini game needs the rules to be real: a move that leaves its own king attacked is
not a move, and a position with no legal answer is a mate or a stalemate. These tests
play a few known games through the Python surface.
"""

import carbine
import pytest


def test_a_fresh_board_is_what_chess_starts_with():
    game = carbine.chess()
    rows = game.board()

    assert len(rows) == 8
    assert all(len(row) == 8 for row in rows)
    assert rows[7] == [
        ("white", "rook"), ("white", "knight"), ("white", "bishop"),
        ("white", "queen"), ("white", "king"), ("white", "bishop"),
        ("white", "knight"), ("white", "rook"),
    ]
    assert rows[6] == [("white", "pawn")] * 8
    assert rows[1] == [("black", "pawn")] * 8
    assert rows[0][4] == ("black", "king")
    assert game.turn == "white"
    assert game.status == "playing"
    assert game.winner is None
    assert game.last is None


def test_a_square_offers_only_legal_moves():
    game = carbine.chess()
    assert game.moves("e2") == ["e2e3", "e2e4"]
    assert game.moves("d2") == ["d2d3", "d2d4"]
    assert game.moves("e4") == []
    with pytest.raises(ValueError, match="not a square"):
        game.moves("z9")


def test_moves_alternate_colours_and_can_be_refused():
    game = carbine.chess()
    game.play("e2", "e4")
    assert game.turn == "black"
    assert game.last == "e2e4"
    with pytest.raises(ValueError, match="not a legal move"):
        game.play("d2", "d4")
    game.play("e7", "e5")
    assert game.turn == "white"


def test_fools_mate_is_detected():
    game = carbine.chess()
    for source, target in [("f2", "f3"), ("e7", "e5"), ("g2", "g4"), ("d8", "h4")]:
        game.play(source, target)

    assert game.status == "checkmate"
    assert game.winner == "black"
    assert game.inCheck() is True
    assert game.inCheck("white") is True
    assert game.inCheck("black") is False
    with pytest.raises(ValueError, match="game is over"):
        game.play("e1", "e2")


def test_reset_starts_a_new_game():
    game = carbine.chess()
    game.play("e2", "e4")
    game.reset()
    assert game.turn == "white"
    assert game.status == "playing"
    assert game.last is None
    assert game.board()[6] == [("white", "pawn")] * 8


def test_castling_moves_the_rook_too():
    game = carbine.chess()
    for source, target in [
        ("e2", "e4"), ("e7", "e5"), ("g1", "f3"), ("b8", "c6"),
        ("f1", "e2"), ("g8", "f6"), ("e1", "g1"),
    ]:
        game.play(source, target)
    assert game.board()[7][6] == ("white", "king")
    assert game.board()[7][5] == ("white", "rook")

    assert carbine.chess().board()[6][4] == ("white", "pawn")
