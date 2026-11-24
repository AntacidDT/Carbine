"""Video in a world, on a screen you can walk around — and on the overlay.

    python examples/video.py [clip.mp4]

With no path, a random `.mp4` from your home directory is chosen. The MIT-licensed
carbine character from ~/Documents is worn as the player body; WASD and the mouse
walk around a procedural map, with a cinema screen standing in it and a smaller
copy on the HUD. The Packager paints the carbine logo into a UI image as well.

    space        play / pause both films
    l            loop on / off
    up / down    volume of the world screen
    r            pick another random film
    escape       hand the pointer back; click to take it again
    close        quit
"""

import random
import shutil
import subprocess
import sys
from itertools import islice
from pathlib import Path

import carbine

GENERATED = Path(__file__).resolve().parent / "generated"
CHARACTER = Path.home() / "Documents" / "carbinecharacterMITlicensed.glb"
LOGO = Path.home() / "Downloads" / "carbinelogo.png"


def random_film() -> str:
    """Any mp4 in the home directory, skipping hidden folders and tiny files."""
    candidates = []
    for path in islice(Path.home().rglob("*.mp4"), 2000):
        if not path.is_file() or any(part.startswith(".") for part in path.parts):
            continue
        try:
            if path.stat().st_size > 100_000:
                candidates.append(path)
        except OSError:
            continue
    if candidates:
        return str(random.choice(candidates))
    return made_up_clip()


def made_up_clip() -> str:
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        raise SystemExit("No mp4 found in your home and ffmpeg is not installed")
    GENERATED.mkdir(parents=True, exist_ok=True)
    path = GENERATED / "test_pattern.mp4"
    subprocess.run(
        [
            ffmpeg,
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30:duration=6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            str(path),
        ],
        check=True,
    )
    return str(path)


def pick_source() -> str:
    return sys.argv[1] if len(sys.argv) > 1 else random_film()


def make_player() -> carbine.Object:
    body = carbine.new("Model", name="Carbine Character")
    if CHARACTER.exists():
        body.model = str(CHARACTER)
        # The GLB stands about 4.4 units tall; the player is a roughly 2 m walker.
        body.scale = (0.45, 0.45, 0.45)
    else:
        body.model = None
        body.color = "#eb7a34"
        body.size = (0.6, 0.6, 1.8)
    return body


def main() -> None:
    carbine.fpsCap = 90
    carbine.monitorSync = True
    adapter = carbine.init(1280, 720, title="Carbine Cinema")
    print(f"{adapter}\nWASD walk - mouse look - space jump\n"
          "space play/pause - L loop - up/down volume - R new film - close to quit")

    ground = carbine.procedural.map(
        size=(80.0, 80.0), relief=2.5, quality="medium", seed="cinema"
    )
    ground.texture = None

    player = carbine.player()
    player.enabled = True
    player.mode = "third"
    player.speed = 6.0
    player.distance = 7.0
    player.position = (0.0, -10.0, 0.0)
    player.body = make_player()
    carbine.grabMouse = True

    source = pick_source()
    print(f"now showing: {source}")

    # A screen standing in the map, facing the player.
    screen = carbine.new("Video", name="Cinema Screen")
    screen.source = source
    screen.position = (0.0, 0.0, 3.5)
    screen.rotationEuler = (90.0, 0.0, 0.0)
    screen.looping = True
    screen.volume = 0.5
    screen.play()
    screen.scale = (14.0, 8.0, 1.0)

    # The same film smaller on the overlay, silent so there is one soundtrack.
    hud = carbine.ui.new("Video", name="HUD Video")
    hud.source = source
    hud.position = (420.0, -260.0, 0.0)
    hud.scale = (256.0, 144.0, 1.0)
    hud.volume = 0.0
    hud.looping = True
    hud.play()

    panel = carbine.ui.new("Image", name="Logo")
    panel.position = (-520.0, 280.0, 0.0)
    panel.scale = (140.0, 140.0, 1.0)
    if LOGO.exists():
        GENERATED.mkdir(parents=True, exist_ok=True)
        panel.texture = carbine.packager.image(str(LOGO), out=str(GENERATED))

    try:
        while carbine.running():
            carbine.render()
            if carbine.pressed("space"):
                for film in (screen, hud):
                    if film.playing:
                        film.stop()
                    else:
                        film.play()
            if carbine.pressed("l"):
                for film in (screen, hud):
                    film.looping = not film.looping
            if carbine.key("up"):
                screen.volume = min(1.0, screen.volume + 0.02)
            if carbine.key("down"):
                screen.volume = max(0.0, screen.volume - 0.02)
            if carbine.pressed("r"):
                source = random_film()
                for film in (screen, hud):
                    film.source = source
                    film.play()
                print(f"now showing: {source}")
    finally:
        carbine.end()


if __name__ == "__main__":
    main()
