"""`carbine.new("Sound")` - audio as a node.

Carbine reads `.wav` itself and hands `.mp3` to a decoder; there is no conversion
step, so a script points a sound at the file it has. The mixer keeps one voice per
node. These tests step the world rather than draw a frame, which means no output
device is opened: the mixer advances on the game clock, so a sound still starts,
moves its playhead and ends exactly as it would in a window.
"""

import math
import shutil
import struct
import subprocess
import wave

import carbine
import pytest

SECOND = 1.0 / 60.0


def write_wav(path, *, seconds=0.25, rate=8_000, frequency=440.0, silent=False):
    """A mono 16-bit WAV, small enough to generate in the test process."""
    frames = int(rate * seconds)
    with wave.open(str(path), "wb") as file:
        file.setnchannels(1)
        file.setsampwidth(2)
        file.setframerate(rate)
        file.writeframes(
            b"".join(
                struct.pack(
                    "<h", 0 if silent else int(10_000 * math.sin(2 * math.pi * frequency * i / rate))
                )
                for i in range(frames)
            )
        )
    return str(path)


@pytest.fixture
def quiet():
    """A world with nothing in it. The scene is one per process, so a sound left
    playing would keep its voice for the next test."""

    def sweep():
        carbine.destroy("all")
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


@pytest.fixture
def tone(tmp_path, quiet):
    return write_wav(tmp_path / "tone.wav", seconds=0.25)


@pytest.fixture
def short(tmp_path, quiet):
    return write_wav(tmp_path / "short.wav", seconds=0.1)


def test_sound_is_a_node(quiet, tone):
    sound = carbine.new("Sound", name="Music")
    sound.source = tone

    assert isinstance(sound, carbine.Sound)
    assert sound.kind == "sound"
    assert sound.name == "Music"
    assert sound in carbine.scene.root.children
    assert sound.parent == carbine.scene.root


def test_sound_kind_is_listed(quiet):
    assert "sound" in carbine.kinds()


def test_sound_defaults(quiet):
    sound = carbine.new("sound")

    assert sound.name == "Sound"
    assert sound.source is None
    assert sound.volume == pytest.approx(1.0)
    assert sound.looping is False
    assert sound.playing is False
    assert sound.time == pytest.approx(0.0)
    assert sound.duration == pytest.approx(0.0)


def test_source_accepts_wav_and_mp3_and_nothing_else(quiet, tone, tmp_path):
    sound = carbine.new("sound")

    sound.source = tone
    assert sound.source == tone
    sound.source = None
    assert sound.source is None

    with pytest.raises(ValueError, match="wav"):
        sound.source = "music.ogg"
    with pytest.raises(ValueError, match="file"):
        sound.source = str(tmp_path / "nowhere.mp3")


def test_volume_is_held_between_silence_and_full(quiet):
    sound = carbine.new("sound")

    sound.volume = 4.0
    assert sound.volume == pytest.approx(1.0)
    sound.volume = -2.0
    assert sound.volume == pytest.approx(0.0)
    sound.volume = 0.25
    assert sound.volume == pytest.approx(0.25)


def test_changing_the_source_stops_the_playhead(quiet, tone):
    sound = carbine.new("sound")
    sound.source = tone
    sound.play()
    carbine.step(0.05)
    assert sound.time > 0.0

    sound.source = tone
    assert sound.playing is False
    assert sound.time == pytest.approx(0.0)


def test_play_steps_and_ends(quiet, short):
    sound = carbine.new("sound")
    sound.source = short
    assert sound.duration == pytest.approx(0.1, abs=1e-3)

    sound.play()
    assert sound.playing is True
    carbine.step(0.05)
    assert sound.time == pytest.approx(0.05, abs=1e-3)
    assert sound.playing is True

    carbine.step(0.1)
    assert sound.playing is False
    assert sound.time == pytest.approx(0.1, abs=1e-3)


def test_stop_returns_to_the_start(quiet, tone):
    sound = carbine.new("sound")
    sound.source = tone
    sound.play()
    carbine.step(0.1)
    sound.stop()

    assert sound.playing is False
    assert sound.time == pytest.approx(0.0)
    # Stopping a stopped sound is harmless.
    sound.stop()


def test_looping_does_not_end(quiet, short):
    sound = carbine.new("sound")
    sound.source = short
    sound.looping = True
    sound.play()

    carbine.step(0.35)
    assert sound.playing is True
    assert sound.time < sound.duration


def test_playing_can_be_written_as_a_switch(quiet, tone):
    sound = carbine.new("sound")
    sound.source = tone

    sound.playing = True
    assert sound.playing is True
    sound.playing = False
    assert sound.playing is False


def test_a_broken_wav_is_refused_when_played(quiet, tmp_path):
    broken = tmp_path / "broken.wav"
    broken.write_bytes(b"RIFF not really a wave")
    sound = carbine.new("sound")
    sound.source = str(broken)

    with pytest.raises(ValueError, match="decode"):
        sound.play()
    assert sound.playing is False


def test_an_mp3_decodes_and_plays(quiet, tmp_path):
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        pytest.skip("the system has no ffmpeg to author an mp3")
    source = write_wav(tmp_path / "source.wav", seconds=0.2, silent=True)
    encoded = tmp_path / "tone.mp3"
    result = subprocess.run(
        [ffmpeg, "-y", "-loglevel", "error", "-i", source, str(encoded)],
        capture_output=True,
    )
    if result.returncode != 0:
        pytest.skip("this ffmpeg build has no mp3 encoder")

    sound = carbine.new("sound")
    sound.source = str(encoded)
    assert sound.duration > 0.0
    sound.play()
    carbine.step(0.05)
    assert sound.time > 0.0
    assert sound.playing is True


def test_parenting_and_equality(quiet, tone):
    holder = carbine.new("cube", name="Speaker")
    sound = carbine.new("sound", name="Mono", parent=holder)
    sound.source = tone

    assert sound.parent == holder
    assert sound in holder.children
    found = next(node for node in holder.children if node.name == "Mono")
    assert found == sound
    assert hash(found) == hash(sound)
    assert len({found, sound}) == 1

    holder.removeChild(sound)
    assert sound.parent == carbine.scene.root


def test_duplicate_carries_the_sound_and_linked_twins_share_it(quiet, tone):
    sound = carbine.new("sound")
    sound.source = tone
    sound.volume = 0.4
    sound.looping = True

    copy = carbine.duplicate(sound)[0]
    assert isinstance(copy, carbine.Sound)
    assert copy.source == tone
    assert copy.volume == pytest.approx(0.4)
    assert copy.looping is True

    twin = carbine.linkDuplicate(sound)[0]
    sound.volume = 0.7
    assert twin.volume == pytest.approx(0.7)
    sound.playing = True
    assert twin.playing is False


def test_destroy_takes_the_voice_with_it(quiet, tone):
    sound = carbine.new("sound")
    sound.source = tone
    sound.play()
    node_id = sound.id

    sound.destroy()
    assert all(node.id != node_id for node in carbine.scene.root.children)
    with pytest.raises(carbine.DestroyedError):
        _ = sound.volume


def test_loop_alias_and_stop(quiet, short):
    sound = carbine.new("sound")
    sound.source = short

    assert sound.loop is False
    sound.play()
    carbine.step(0.2)
    assert sound.playing is False, "loop=False plays once"

    sound.loop = True
    assert sound.looping is True
    sound.loop = False
    assert sound.looping is False

    sound.loop = True
    sound.play()
    carbine.step(0.2)
    assert sound.playing is True, "loop=True keeps going"

    sound.stop()
    assert sound.playing is False
    assert sound.time == pytest.approx(0.0)


def test_seeking_a_sound(quiet, short):
    sound = carbine.new("sound")
    sound.source = short
    sound.loop = True
    sound.play()
    carbine.step(0.08)
    sound.time = 0.01
    assert sound.time == pytest.approx(0.01, abs=1e-2)
    carbine.step(0.04)
    assert sound.time > 0.01

    sound.stop()
    sound.seek(0.04)
    assert sound.time == pytest.approx(0.04, abs=1e-2)
    sound.play()
    assert sound.time == pytest.approx(0.04, abs=1e-2)


def test_spatial_is_a_sound_switch(quiet):
    sound = carbine.new("sound")
    assert sound.spatial is False
    sound.spatial = True
    assert sound.spatial is True
