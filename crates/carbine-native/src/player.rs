//! The player controller: a body that walks, looks and falls under its own
//! input, with the camera riding it.

use carbine_core::PlayerMode;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::apply_pointer_grab;
use crate::input::key_from_str;
use crate::math::{PyVec3, extract_vec3};
use crate::object::Object;
use crate::{node_id_of, with_scene};

fn mode_name(mode: PlayerMode) -> &'static str {
    mode.name()
}

/// The controller `carbine.player()` returns.
///
/// Turn it on and the engine takes over: WASD walks, the mouse looks, gravity
/// pulls, and the camera follows. Every number is per second, so `speed = 5` is
/// five meters a second.
#[pyclass(frozen, module = "carbine", name = "Player")]
pub struct PyPlayer;

impl PyPlayer {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyPlayer {
    /// Whether the engine drives the camera and the walk. False leaves the
    /// camera exactly where the script put it.
    #[getter]
    fn enabled(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.player_is_enabled()))
    }

    #[setter]
    fn set_enabled(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_enabled(value);
            Ok(())
        })?;
        sync_grab()
    }

    /// `"first"` puts the camera in the eyes, `"third"` parks it behind the body.
    #[getter]
    fn mode(&self) -> PyResult<String> {
        with_scene(|scene| Ok(mode_name(scene.player_mode()).to_owned()))
    }

    #[setter]
    fn set_mode(&self, value: &str) -> PyResult<()> {
        let mode = PlayerMode::from_name(value).ok_or_else(|| {
            PyValueError::new_err(format!(
                "\"{value}\" is not a player mode; use \"first\" or \"third\""
            ))
        })?;
        with_scene(|scene| {
            scene.set_player_mode(mode);
            Ok(())
        })
    }

    /// The player's feet, in meters. `z` is height above the ground. A write
    /// arrives at rest, so the walk does not carry on from the new place.
    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        with_scene(|scene| Ok(PyVec3::plain(scene.player_position())))
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| {
            scene.set_player_position(value);
            Ok(())
        })
    }

    /// Which way the player faces, in degrees about the vertical axis.
    #[getter]
    fn yaw(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_yaw()))
    }

    #[setter]
    fn set_yaw(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_yaw(value as f32);
            Ok(())
        })
    }

    /// How far up or down the player looks, in degrees. It never passes the
    /// vertical, which would flip the camera.
    #[getter]
    fn pitch(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_pitch()))
    }

    #[setter]
    fn set_pitch(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_pitch(value as f32);
            Ok(())
        })
    }

    /// Meters per second at full tilt.
    #[getter]
    fn speed(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_speed()))
    }

    #[setter]
    fn set_speed(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_speed(value as f32);
            Ok(())
        })
    }

    /// Degrees turned per pixel the mouse moves.
    #[getter]
    fn sensitivity(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_sensitivity()))
    }

    #[setter]
    fn set_sensitivity(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_sensitivity(value as f32);
            Ok(())
        })
    }

    /// How far above the feet the eyes, and the first person camera, sit.
    #[getter(eyeHeight)]
    fn eye_height(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_eye_height()))
    }

    #[setter(eyeHeight)]
    fn set_eye_height(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_eye_height(value as f32);
            Ok(())
        })
    }

    /// How far behind the body the third person camera parks, in meters.
    #[getter]
    fn distance(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_distance()))
    }

    #[setter]
    fn set_distance(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_distance(value as f32);
            Ok(())
        })
    }

    /// Metres per second squared. 10 is Earth, 1.6 would be the Moon.
    #[getter(gravityPull)]
    fn gravity_pull(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_gravity_pull()))
    }

    #[setter(gravityPull)]
    fn set_gravity_pull(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_gravity_pull(value as f32);
            Ok(())
        })
    }

    /// Meters per second a jump leaves the ground with.
    #[getter(jumpStrength)]
    fn jump_strength(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.player_jump_strength()))
    }

    #[setter(jumpStrength)]
    fn set_jump_strength(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_player_jump_strength(value as f32);
            Ok(())
        })
    }

    /// Whether the feet are on the ground, which is also whether a jump can start.
    #[getter]
    fn grounded(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.player_is_grounded()))
    }

    /// The object drawn as the player. The engine hides it in first person and
    /// shows it in third, and moves it with every step.
    #[getter]
    fn body(&self) -> PyResult<Option<Object>> {
        let body = with_scene(|scene| Ok(scene.player_body()))?;
        Ok(body.map(Object::from_id))
    }

    #[setter]
    fn set_body(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        let body = match value.as_ref() {
            Some(handle) => Some(node_id_of(handle)?),
            None => None,
        };
        with_scene(|scene| scene.set_player_body(body))
    }

    /// Whether the engine still holds the pointer for looking. Escape gives it
    /// back and a click asks for it again.
    #[getter(grabMouse)]
    fn grab_mouse(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.wants_pointer_grab()))
    }

    #[setter(grabMouse)]
    fn set_grab_mouse(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_pointer_grab(value);
            Ok(())
        })?;
        sync_grab()
    }

    /// Pushes off the ground, if the feet are on it.
    fn jump(&self) -> PyResult<()> {
        with_scene(|scene| {
            scene.player_jump();
            Ok(())
        })
    }

    /// Whether a named key is down right now, for a script that reads the
    /// keyboard itself instead of leaving the walk to the engine.
    #[pyo3(name = "isKeyDown")]
    fn is_key_down(&self, name: &str) -> PyResult<bool> {
        let key = key_from_str(name)?;
        with_scene(|scene| Ok(scene.input().key(key)))
    }

    /// Meters per second, including the fall. A HUD reads it for a speedometer.
    #[getter]
    fn velocity(&self) -> PyResult<PyVec3> {
        with_scene(|scene| Ok(PyVec3::plain(scene.player_velocity())))
    }

    fn __repr__(&self) -> PyResult<String> {
        let (enabled, mode, position, yaw, pitch) = with_scene(|scene| {
            Ok((
                scene.player_is_enabled(),
                scene.player_mode(),
                scene.player_position(),
                scene.player_yaw(),
                scene.player_pitch(),
            ))
        })?;
        Ok(format!(
            "<Player {} enabled={} at=({}, {}, {}) yaw={:.1} pitch={:.1}>",
            mode_name(mode),
            enabled,
            position.x,
            position.y,
            position.z,
            yaw,
            pitch
        ))
    }
}

/// Asks the window for the pointer the scene currently wants.
fn sync_grab() -> PyResult<()> {
    let wants = with_scene(|scene| Ok(scene.wants_pointer_grab()))?;
    apply_pointer_grab(wants);
    Ok(())
}
