"""Sound: a `.wav` played as a node.

    python examples/sound.py

There is no conversion step. Carbine reads WAV itself, mixes one voice per node and
hands the result to the output device. This example writes a short chord into
`examples/generated` and plays it on a loop, so there is something to hear without
downloading an asset.

The same API takes an `.mp3`: `sound.source = "music.mp3"`.

    space        play / pause
    up / down    louder / quieter
    l            loop on / off
    close        quit
"""

import math
import struct
import wave
from pathlib import Path

import carbine

GENERATED = Path(__file__).resolve().parent / "generated"
SECONDS = 2.0
RATE = 44_100


def write_chord(path: Path) -> str:
    """A slow major chord with a little vibrato, written as 16-bit mono PCM."""
    frames = int(RATE * SECONDS)
    with wave.open(str(path), "wb") as file:
        file.setnchannels(1)
        file.setsampwidth(2)
        file.setframerate(RATE)
        samples = []
        for index in range(frames):
            t = index / RATE
            fade = min(1.0, t * 4.0, (SECONDS - t) * 4.0)
            voice = (
                math.sin(2 * math.pi * 261.63 * t)
                + 0.7 * math.sin(2 * math.pi * 329.63 * t)
                + 0.5 * math.sin(2 * math.pi * 392.00 * t)
            ) / 2.2
            wobble = 1.0 + 0.02 * math.sin(2 * math.pi * 5.0 * t)
            samples.append(int(12_000 * voice * fade * wobble))
        file.writeframes(b"".join(struct.pack("<h", value) for value in samples))
    return str(path)


def main() -> None:
    GENERATED.mkdir(parents=True, exist_ok=True)
    path = GENERATED / "chord.wav"
    if not path.exists():
        write_chord(path)

    adapter = carbine.init(960, 540, title="Carbine Sound")
    print(
        f"{adapter}\nplaying {path.name}, a looping chord\n"
        "space play/pause - up/down volume - L loop - close to quit"
    )

    sound = carbine.new("Sound", name="Chord")
    sound.source = str(path)
    sound.looping = True
    sound.volume = 0.6
    sound.play()

    try:
        while carbine.running():
            carbine.render()
            if carbine.pressed("space"):
                if sound.playing:
                    sound.stop()
                else:
                    sound.play()
                print("playing" if sound.playing else "stopped")
            if carbine.key("up"):
                sound.volume = min(1.0, sound.volume + 0.02)
            if carbine.key("down"):
                sound.volume = max(0.0, sound.volume - 0.02)
            if carbine.pressed("l"):
                sound.looping = not sound.looping
                print(f"loop {'on' if sound.looping else 'off'}")
    finally:
        carbine.end()


if __name__ == "__main__":
    main()
