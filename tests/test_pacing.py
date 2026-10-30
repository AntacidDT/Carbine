"""`carbine.fpsCap` and `carbine.monitorSync` — the two limits on how fast the window
is allowed to draw.

The lower of them is what runs, which is what lets one number travel with a player: a
cap of 90 holds 90 on their 144 Hz desktop so the GPU does not scream, and the same game
on the laptop at the café holds 60 because the screen is now the lower limit. Both are
set on the module, before a window exists, because that is when a game names them.

Neither is a limit until it is asked for. The engine starts with `fpsCap` as `None` and
`monitorSync` off, which leaves the frames to whatever the machine does with an unsynced
window, and every rate a test then reads is one it named itself.

What is left here is the rule with no screen to consult. A desktop hands out exactly one
window per process, so the case that needs a real refresh rate — and a cap timed against
it — is told by `test_a_real_window_renders_and_holds_the_lower_limit`."""

import subprocess
import sys

import carbine
import pytest


@pytest.fixture
def limits():
    """The frame limits are engine-wide, and a cap left at 15 would slow every test
    that draws afterwards, so the pair is handed back as it was found."""
    found = (carbine.fpsCap, carbine.monitorSync)
    try:
        yield found
    finally:
        carbine.fpsCap, carbine.monitorSync = found


def test_the_frame_runs_free_until_it_is_told_otherwise(limits):
    """Nothing set means nothing held: no cap, no sync, and so no rate at all."""
    cap, sync = limits

    assert (cap, sync) == (None, False)
    assert carbine.monitorHz == 0, "no window, so no screen to ask"
    assert carbine.fpsLimit is None


def test_a_cap_named_before_the_window_is_kept_for_it(limits):
    """`carbine.fpsCap = 90` is a thing a game writes while it is still setting up,
    and the number has to still mean 90 once there is a renderer to hold to it."""
    carbine.fpsCap = 90

    carbine.init(64, 64, window=False)
    try:
        assert carbine.fpsCap == 90
        assert carbine.monitorSync is False
    finally:
        carbine.close()


def test_an_offscreen_target_has_no_screen_to_sync_with(headless, limits):
    """An image rendered to memory has no refresh rate, so the cap is the only limit
    and turning the sync knob changes nothing about it."""
    assert headless
    assert carbine.monitorHz == 0

    carbine.fpsCap = 90
    assert carbine.fpsLimit == 90

    carbine.monitorSync = True
    assert carbine.fpsLimit == 90, "syncing with no screen leaves the cap alone"


def test_the_cap_alone_holds_with_no_screen_rate_to_trust(headless, limits):
    """The screen's rate is a fact about a window, not a wish: when the compositor
    will not name it, the cap is enforced by waiting rather than given up on."""
    carbine.fpsCap = 24
    carbine.monitorSync = True

    assert carbine.monitorHz == 0
    assert carbine.fpsLimit == 24


def test_sync_alone_is_a_pace_only_once_a_screen_names_itself(limits):
    """Asking for the screen's rate is not the same as inventing one, so with nothing
    to read the frames stay free — until a cap is named, which is readable."""
    carbine.monitorSync = True

    assert carbine.monitorHz == 0
    assert carbine.fpsLimit is None

    carbine.fpsCap = 144
    assert carbine.fpsLimit == 144


def test_a_cap_of_nothing_is_no_cap(headless, limits):
    """Two spellings of the same wish, because `None` is how Python says it and `0` is
    how a slider that reached the end says it."""
    carbine.fpsCap = 144
    assert carbine.fpsLimit == 144

    for off in (0, None):
        carbine.fpsCap = off
        assert carbine.fpsCap is None
        assert carbine.fpsLimit is None, "an uncapped frame is held by nobody"


def test_a_cap_is_a_whole_number_of_frames(limits):
    for awkward in (-1, 12.5, "ninety", 2**40):
        with pytest.raises((OverflowError, TypeError)):
            carbine.fpsCap = awkward

    for not_a_wish in ("yes", 1, None):
        with pytest.raises(TypeError):
            carbine.monitorSync = not_a_wish


def test_reading_the_limits_before_the_window_leaves_the_engine_closable():
    """Naming a cap is something a game does while it is still setting up, so the
    getters have to answer before there is a renderer - and answering must not cost
    the process its exit. Dropping a renderer after the GPU library has torn itself
    down aborts, which a script that never called `close()` would hit on the way out,
    so this runs a whole interpreter and reads its exit status."""
    script = """
import carbine

print(carbine.fpsCap, carbine.monitorSync, carbine.monitorHz, carbine.fpsLimit)
carbine.init(64, 64, window=False)
"""
    finished = subprocess.run(
        [sys.executable, "-c", script], capture_output=True, text=True, timeout=120
    )

    assert finished.stdout.split() == ["None", "False", "0", "None"]
    assert finished.returncode == 0, finished.stderr[-400:]
