import atexit
import sys
from types import ModuleType

from . import _native
from ._native import (
    Atmosphere,
    Camera,
    DestroyedError,
    Disaster,
    Entity,
    Light,
    Mesh,
    Mouse,
    Object,
    Packager,
    Particle,
    Physics,
    Player,
    Procedural,
    Quat,
    Scene,
    Sky,
    Terrain,
    Ui,
    Vec3,
    __version__,
    camera,
    close,
    collisionTypes,
    destroy,
    dt,
    duplicate,
    end,
    grabMouse,
    handlingTypes,
    init,
    injectButton,
    injectKey,
    injectMouse,
    key,
    kinds,
    killEntity,
    keys,
    lightTypes,
    linkDuplicate,
    mouse,
    new,
    player,
    pressed,
    readPixels,
    released,
    render,
    running,
    spawnEntity,
    step,
    viewport,
)

scene = _native.get_scene()
ui = _native.get_ui()
physics = _native.physics()
atmosphere = _native.atmosphere()
procedural = _native.procedural()
packager = _native.packager()

# The engine closes itself when the script ends. A renderer left alive is dropped after
# the GPU library has already torn its own state down, which aborts the process on the
# way out; doing it here happens while everything is still up, so a game that never
# thought to call `close()` still leaves a window and a device cleanly.
atexit.register(close)


class _Carbine(ModuleType):
    """The frame limits, as plain attributes: `carbine.fpsCap = 90` and
    `carbine.monitorSync = True`. They are set on the module rather than on a window
    because a game names them before it has a window to name them to, and the lower
    of the two is what the frames settle at.

    Neither one exists until it is asked for. Left alone, `fpsCap` is `None`,
    `monitorSync` is `False` and nothing holds the frames.

    `carbine.lightType` lives here for the same reason: `carbine.lightType = "blocky"`
    asks for shade edges laid in steps, and `"straight"` — which is what the world
    starts with — weaves them. It is a look, not a cost; how much light is worked at
    all is `carbine.atmosphere.lightQuality`."""

    @property
    def fpsCap(self) -> int | None:
        return _native.fpsCap()

    @fpsCap.setter
    def fpsCap(self, fps: int | None) -> None:
        _native.setFpsCap(fps)

    @property
    def monitorSync(self) -> bool:
        return _native.monitorSync()

    @monitorSync.setter
    def monitorSync(self, sync: bool) -> None:
        _native.setMonitorSync(sync)

    @property
    def monitorHz(self) -> int:
        return _native.monitorHz()

    @property
    def fpsLimit(self) -> int | None:
        return _native.fpsLimit()

    @property
    def fps(self) -> float:
        """Measured frames per second, refreshed by the native clock every 0.25s."""
        return _native.fps()

    @property
    def lightType(self) -> str:
        return _native.lightType()

    @lightType.setter
    def lightType(self, style: str) -> None:
        _native.setLightType(style)


sys.modules[__name__].__class__ = _Carbine

__all__ = [
    "Atmosphere",
    "Camera",
    "DestroyedError",
    "Disaster",
    "Entity",
    "Light",
    "Mouse",
    "Mesh",
    "Object",
    "Packager",
    "Particle",
    "Physics",
    "Player",
    "Procedural",
    "Quat",
    "Scene",
    "Sky",
    "Terrain",
    "Ui",
    "Vec3",
    "__version__",
    "atmosphere",
    "camera",
    "close",
    "collisionTypes",
    "destroy",
    "dt",
    "duplicate",
    "end",
    "fps",
    "fpsCap",
    "fpsLimit",
    "grabMouse",
    "handlingTypes",
    "init",
    "injectButton",
    "injectKey",
    "injectMouse",
    "key",
    "kinds",
    "killEntity",
    "keys",
    "lightType",
    "lightTypes",
    "linkDuplicate",
    "mouse",
    "monitorHz",
    "monitorSync",
    "new",
    "packager",
    "player",
    "pressed",
    "procedural",
    "readPixels",
    "released",
    "render",
    "running",
    "scene",
    "spawnEntity",
    "step",
    "ui",
    "physics",
    "viewport",
]
