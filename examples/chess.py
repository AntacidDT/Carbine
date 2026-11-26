"""Chess in Carbine, using real rules from the Rust core and Cburnett's piece SVGs.

    python examples/chess.py

White moves first. Click a piece, then a highlighted square. The Rust board refuses
anything illegal, spots checks, and calls checkmate or stalemate. Castling, en passant
and promotion are in; a pawn reaching the far rank promotes to a queen.

The piece PNGs in `examples/assets/chess/` are rasterised from Cburnett's public SVG
set (see the CREDITS file beside them) and packed to `.ktx2` on the way in.

    click     select a piece / play to a highlighted square
    r         start over
    close     quit
"""

from pathlib import Path

import carbine

PIECES = Path(__file__).resolve().parent / "assets" / "chess"
GENERATED = Path(__file__).resolve().parent / "generated" / "chess"
BOARD = 576
CELL = BOARD // 8

LIGHT = "#d9b27c"
DARK = "#7a5638"
SELECTED = "#f4e06d"
TARGET = "#74c476"
THREAT = "#d95f5f"

CODES = {
    ("white", "king"): "klt",
    ("white", "queen"): "qlt",
    ("white", "rook"): "rlt",
    ("white", "bishop"): "blt",
    ("white", "knight"): "nlt",
    ("white", "pawn"): "plt",
    ("black", "king"): "kdt",
    ("black", "queen"): "qdt",
    ("black", "rook"): "rdt",
    ("black", "bishop"): "bdt",
    ("black", "knight"): "ndt",
    ("black", "pawn"): "pdt",
}


def square_name(index):
    return "abcdefgh"[index % 8] + "12345678"[index // 8]


def screen_square(x, y, origin_x, origin_y):
    column = int((x - origin_x) // CELL)
    row = int((y - origin_y) // CELL)
    if not (0 <= column < 8 and 0 <= row < 8):
        return None
    # The top row of the picture is rank eight.
    return (7 - row) * 8 + column


def centre(index, origin_x, origin_y):
    column, rank = index % 8, index // 8
    row = 7 - rank
    return (
        origin_x + column * CELL + CELL * 0.5,
        origin_y + row * CELL + CELL * 0.5,
        0.0,
    )


def main() -> None:
    carbine.fpsCap = 90
    adapter = carbine.init(960, 720, title="Carbine Chess")
    width, height = carbine.viewport()
    origin_x, origin_y = (width - BOARD) // 2, (height - BOARD) // 2
    GENERATED.mkdir(parents=True, exist_ok=True)

    # The Packager turns each authored picture into the one format the engine reads.
    skins = {}
    for piece, code in CODES.items():
        skins[piece] = carbine.packager.image(
            str(PIECES / f"{code}.png"), out=str(GENERATED / f"{code}.ktx2")
        )

    game = carbine.chess()
    squares = []
    for index in range(64):
        tile = carbine.ui.new("square", name=square_name(index))
        tile.position = centre(index, origin_x, origin_y)
        tile.scale = (CELL, CELL, 1.0)
        squares.append(tile)

    status = carbine.ui.new("Text", name="Status")
    status.position = (24.0, 22.0, 0.0)

    pieces = []
    selected = None
    legal = []
    held = False

    def clear_pieces():
        for piece in pieces:
            piece.destroy()
        pieces.clear()

    def refresh():
        nonlocal legal
        clear_pieces()
        rows = game.board()
        legal = game.moves(square_name(selected)) if selected is not None else []

        for index in range(64):
            base = LIGHT if (index // 8 + index % 8) % 2 == 0 else DARK
            if index == selected:
                base = SELECTED
            elif square_name(index) in legal:
                base = TARGET
            squares[index].color = base

        if game.inCheck(game.turn):
            for rank, row in enumerate(rows):
                for file, square in enumerate(row):
                    if square == (game.turn, "king"):
                        squares[(7 - rank) * 8 + file].color = THREAT

        for rank, row in enumerate(rows):
            for file, square in enumerate(row):
                if square is None:
                    continue
                index = (7 - rank) * 8 + file
                piece = carbine.ui.new("Image", name=f"{square[0]} {square[1]}")
                piece.position = centre(index, origin_x, origin_y)
                piece.scale = (CELL * 0.88, CELL * 0.88, 1.0)
                piece.texture = skins[square]
                pieces.append(piece)

        if game.status == "checkmate":
            line = f"Checkmate — {game.winner} wins. Press R."
        elif game.status == "stalemate":
            line = "Stalemate. Press R."
        elif game.inCheck(game.turn):
            line = f"{game.turn} is in check."
        else:
            line = f"{game.turn} to move."
        status.content = line
        return line

    print(f"{adapter}\nclick a piece then a green square — R restarts — close to quit")
    line = refresh()

    try:
        while carbine.running():
            carbine.render()
            mouse = carbine.mouse()
            down = mouse.left
            clicked = down and not held
            held = down

            if carbine.pressed("r"):
                game.reset()
                selected = None
                line = refresh()
                print("new game")

            if clicked:
                square = screen_square(mouse.x, mouse.y, origin_x, origin_y)
                if square is not None:
                    name = square_name(square)
                    rows = game.board()
                    piece = rows[7 - square // 8][square % 8]
                    if selected is None:
                        if piece is not None and piece[0] == game.turn:
                            selected = square
                            line = refresh()
                    elif name in legal:
                        promotion = "queen" if name + "q" in legal else None
                        game.play(square_name(selected), name, promotion)
                        selected = None
                        line = refresh()
                        print(line)
                    elif piece is not None and piece[0] == game.turn:
                        selected = square
                        line = refresh()
                    else:
                        selected = None
                        line = refresh()
    finally:
        carbine.end()


if __name__ == "__main__":
    main()
