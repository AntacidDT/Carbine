"""Video: an `.mp4` shown on a rectangle, with its sound.

    python examples/video.py [clip.mp4]

With no path, the example asks `ffmpeg` for a three-second test pattern and a tone,
written into `examples/generated`. The picture is decoded on a worker thread and
uploaded as a texture; the film's AAC track is decoded in Rust and mixed like any
other sound.

    space        play / pause
    l            loop on / off
    up / down    volume
    close        quit
"""

import shutil
import subprocess
import sys
from pathlib import Path

import carbine

GENERATED = Path(__file__).resolve().parent / "generated"


def make_clip(path: Path) -> str:
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        raise SystemExit(
            "No clip given and ffmpeg is not installed. Pass an .mp4 path instead."
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            ffmpeg,
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=3",
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


def main() -> None:
    if len(sys.argv) > 1:
        source = sys.argv[1]
    else:
        source = make_clip(GENERATED / "test_pattern.mp4")

    adapter = carbine.init(960, 540, title="Carbine Video")
    camera = carbine.camera()
    camera.position = (0.0, 0.0, 6.0)
    camera.target = (0.0, 0.0, 0.0)

    print(
        f"{adapter}\nshowing {source}\n"
        "space play/pause - L loop - up/down volume - close to quit"
    )

    screen = carbine.new("Video", name="Screen")
    screen.source = source
    screen.position = (0.0, 0.0, 0.0)
    screen.looping = True
    screen.volume = 0.5
    screen.play()

    try:
        while carbine.running():
            carbine.render()
            if carbine.pressed("space"):
                if screen.playing:
                    screen.stop()
                else:
                    screen.play()
                print("playing" if screen.playing else "stopped")
            if carbine.pressed("l"):
                screen.looping = not screen.looping
                print(f"loop {'on' if screen.looping else 'off'}")
            if carbine.key("up"):
                screen.volume = min(1.0, screen.volume + 0.02)
            if carbine.key("down"):
                screen.volume = max(0.0, screen.volume - 0.02)
    finally:
        carbine.end()


if __name__ == "__main__":
    main()
