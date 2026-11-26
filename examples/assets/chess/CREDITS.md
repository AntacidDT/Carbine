# Chess piece artwork

The twelve piece pictures in this folder were made from the public **SVG chess set by
User:Cburnett on Wikimedia Commons**, rasterised to 128×128 PNGs with ImageMagick so
Carbine's Packager can turn them into BC7 `.ktx2` skins.

- Source: <https://commons.wikimedia.org/wiki/Template:SVG_chess_pieces>
- Author: Cburnett
- Licence: CC BY-SA 3.0 / GFDL ( attribution and share-alike required )
- Files: `k/q/r/b/n/p` × `l` light / `d` dark, `t` transparent background.

Carbine itself does not ship the SVGs; the PNGs here are the derivative artwork used
by `examples/chess.py`. The mini game and its rules are part of Carbine and are not
derived from the piece artwork.
