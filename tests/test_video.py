"""`carbine.new("Video")` - an `.mp4` on a rectangle, with its own sound.

The frames are decoded by the system's ffmpeg and the audio track in Rust. These
tests author tiny films on the fly; a machine without an encoder skips rather than
fails. They step the world for the headless clock and draw a frame for the GPU path.
"""

import shutil
import subprocess

import carbine
import pytest

SECOND = 1.0 / 60.0


def encode(path, *, seconds=0.4, rate=10, size="160x90", sound=False):
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        pytest.skip("the system has no ffmpeg")
    command = [
        ffmpeg,
        "-y",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        f"testsrc=size={size}:rate={rate}:duration={seconds}",
    ]
    if sound:
        command += [
            "-f",
            "lavfi",
            "-i",
            f"sine=frequency=440:duration={seconds}",
            "-c:a",
            "aac",
        ]
    command += ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-shortest", str(path)]
    result = subprocess.run(command, capture_output=True)
    if result.returncode != 0:
        pytest.skip(f"this ffmpeg cannot encode an mp4: {result.stderr.decode()[-200:]}")
    return str(path)


@pytest.fixture
def quiet():
    def sweep():
        carbine.destroy("all")
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


@pytest.fixture
def clip(tmp_path, quiet):
    return encode(tmp_path / "clip.mp4")


@pytest.fixture
def voiced(tmp_path, quiet):
    return encode(tmp_path / "voiced.mp4", sound=True)


def test_video_is_a_node(quiet, clip):
    video = carbine.new("Video", name="Screen")
    video.source = clip

    assert isinstance(video, carbine.Video)
    assert video.kind == "video"
    assert video.name == "Screen"
    assert video in carbine.scene.root.children
    assert video.parent == carbine.scene.root


def test_video_kind_is_listed(quiet):
    assert "video" in carbine.kinds()


def test_video_defaults(quiet):
    video = carbine.new("video")

    assert video.name == "Video"
    assert video.source is None
    assert video.volume == pytest.approx(1.0)
    assert video.looping is False
    assert video.playing is False
    assert video.time == pytest.approx(0.0)
    assert video.duration == pytest.approx(0.0)
    assert video.width == 0
    assert video.height == 0


def test_source_accepts_mp4_and_nothing_else(quiet, clip, tmp_path):
    video = carbine.new("video")

    video.source = clip
    assert video.source == clip
    video.source = None
    assert video.source is None

    with pytest.raises(ValueError, match="mp4"):
        video.source = "clip.mkv"
    with pytest.raises(ValueError, match="file"):
        video.source = str(tmp_path / "nowhere.mp4")


def test_play_reads_the_film_and_runs_out(quiet, clip):
    video = carbine.new("video")
    video.source = clip
    video.play()

    assert video.playing is True
    assert video.width == 160
    assert video.height == 90
    assert video.fps == pytest.approx(10.0)
    assert video.duration == pytest.approx(0.4, abs=1e-2)

    carbine.step(0.2)
    assert video.playing is True
    assert video.time == pytest.approx(0.2, abs=1e-2)

    carbine.step(0.4)
    assert video.playing is False
    assert video.time == pytest.approx(video.duration, abs=1e-2)


def test_play_with_a_sound_track(quiet, voiced):
    video = carbine.new("video")
    video.source = voiced
    video.volume = 0.25
    video.play()

    assert video.playing is True
    assert video.duration > 0.0
    carbine.step(0.1)
    assert video.time > 0.0
    video.stop()
    assert video.playing is False
    assert video.time == pytest.approx(0.0)


def test_stop_and_restart(quiet, clip):
    video = carbine.new("video")
    video.source = clip
    video.play()
    carbine.step(0.1)
    video.stop()
    assert video.playing is False

    video.play()
    assert video.playing is True
    assert video.time == pytest.approx(0.0)


def test_looping_does_not_end(quiet, clip):
    video = carbine.new("video")
    video.source = clip
    video.looping = True
    video.play()

    carbine.step(0.6)
    assert video.playing is True
    assert video.time < video.duration


def test_playing_can_be_written_as_a_switch(quiet, clip):
    video = carbine.new("video")
    video.source = clip

    video.playing = True
    assert video.playing is True
    video.playing = False
    assert video.playing is False


def test_a_broken_mp4_is_refused_when_played(quiet, tmp_path):
    broken = tmp_path / "broken.mp4"
    broken.write_bytes(b"not really a film")
    video = carbine.new("video")
    video.source = str(broken)

    with pytest.raises(ValueError, match="ffprobe|video"):
        video.play()
    assert video.playing is False


def test_parenting_equality_and_duplication(quiet, clip):
    holder = carbine.new("cube", name="Wall")
    video = carbine.new("video", name="Screen", parent=holder)
    video.source = clip

    assert video.parent == holder
    assert video in holder.children
    found = next(node for node in holder.children if node.name == "Screen")
    assert found == video
    assert hash(found) == hash(video)

    copy = carbine.duplicate(video)[0]
    assert isinstance(copy, carbine.Video)
    assert copy.source == clip
    assert copy.playing is False

    twin = carbine.linkDuplicate(video)[0]
    video.volume = 0.4
    assert twin.volume == pytest.approx(0.4)
    video.playing = True
    assert twin.playing is False


def test_a_frame_reaches_the_gpu(headless, tmp_path):
    clip = encode(tmp_path / "drawn.mp4", seconds=0.3)
    video = carbine.new("video")
    video.source = clip
    video.volume = 0.0
    video.play()

    for _ in range(4):
        carbine.render()
    assert video.playing is True
    assert video.time > 0.0

    pixels = carbine.readPixels()
    assert len(pixels) == 64 * 64 * 4
    assert any(pixels), "the film rectangle drew something"


def test_destroy_takes_the_film_with_it(quiet, clip):
    video = carbine.new("video")
    video.source = clip
    video.play()
    node_id = video.id

    video.destroy()
    assert all(node.id != node_id for node in carbine.scene.root.children)
    with pytest.raises(carbine.DestroyedError):
        _ = video.volume
