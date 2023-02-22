//! Tools for the player to interact with the world

use bevy::prelude::*;
use leafwing_input_manager::{
    prelude::{ActionState, DualAxis, InputManagerPlugin, InputMap, VirtualDPad},
    user_input::{Modifier, UserInput},
    Actionlike,
};

pub(crate) mod abilities;
pub(crate) mod camera;
pub(crate) mod clipboard;
pub(crate) mod cursor;
pub(crate) mod intent;
pub(crate) mod selection;
pub(crate) mod zoning;

/// All of the code needed for users to interact with the simulation.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugin(InputManagerPlugin::<PlayerAction>::default())
            .init_resource::<ActionState<PlayerAction>>()
            .insert_resource(PlayerAction::default_input_map())
            .add_plugin(camera::CameraPlugin)
            .add_plugin(abilities::AbilitiesPlugin)
            .add_plugin(cursor::CursorPlugin)
            .add_plugin(intent::IntentPlugin)
            .add_plugin(selection::SelectionPlugin)
            .add_plugin(clipboard::ClipboardPlugin)
            .add_plugin(zoning::ZoningPlugin);

        #[cfg(feature = "debug_tools")]
        app.add_plugin(debug_tools::DebugToolsPlugin);
    }
}

/// Public system sets for player interaction, used for system ordering and config
#[derive(SystemLabel, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum InteractionSystem {
    /// Moves the camera
    MoveCamera,
    /// Cursor position is set
    ComputeCursorPos,
    /// Tiles are selected
    SelectTiles,
    /// Held structure(s) are selected
    SetClipboard,
    /// Replenishes the [`IntentPool`](intent::IntentPool) of the hive mind
    ReplenishIntent,
    /// Apply zoning to tiles
    ApplyZoning,
    /// Use intent-spending abilities
    UseAbilities,
    /// Spawn and despawn ghosts
    ManagePreviews,
    /// Updates information about the hovered entities
    HoverDetails,
}

/// Actions that the player can take to modify the game world or their view of it.
///
/// This should only store actions that need a dedicated keybinding.
#[derive(Actionlike, Clone, Debug)]
pub(crate) enum PlayerAction {
    /// Selects a tile or group of tiles.
    Select,
    /// Deselects a tile or group of tiles.
    Deselect,
    /// Increases the radius of the selection by one tile.
    IncreaseSelectionRadius,
    /// Decreases the radius of the selection by one tile.
    DecreaseSelectionRadius,
    /// Modifies the selection / deselection to be sequential.
    Multiple,
    /// Modifies the selection to cover a hexagonal area.
    Area,
    /// Modifies the selection to cover a line between the start and end of the selection.
    Line,
    /// Selects a structure from a wheel menu.
    SelectStructure,
    /// Selects the structure on the tile under the player's cursor.
    ///
    /// If there is no structure there, the player's selection is cleared.
    Pipette,
    /// Sets the zoning of all currently selected tiles to the currently selected structure.
    ///
    /// If no structure is selected, any zoning will be removed.
    Zone,
    /// Sets the zoning of all currently selected tiles to [`Zoning::None`](zoning::Zoning::None).
    ///
    /// If no structure is selected, any zoning will be removed.
    ClearZoning,
    /// Rotates the conents of the clipboard counterclockwise.
    RotateClipboardLeft,
    /// Rotates the contents of the clipboard clockwise.
    RotateClipboardRight,
    /// Snaps the camera to the selected object
    SnapToSelection,
    /// Move the camera from side to side
    Pan,
    /// Move the cursor around the screen
    MoveCursor,
    /// Reveal less of the map by moving the camera closer
    ZoomIn,
    /// Reveal more of the map by pulling the camera away
    ZoomOut,
    /// Rotates the camera counterclockwise
    RotateCameraLeft,
    /// Rotates the camera clockwise
    RotateCameraRight,
}

impl PlayerAction {
    /// The default keybindings for mouse and keyboard.
    fn kbm_binding(&self) -> UserInput {
        use PlayerAction::*;
        match self {
            Select => MouseButton::Left.into(),
            Deselect => MouseButton::Right.into(),
            // Plus and Equals are swapped. See: https://github.com/rust-windowing/winit/issues/2682
            IncreaseSelectionRadius => UserInput::modified(Modifier::Control, KeyCode::Equals),
            DecreaseSelectionRadius => UserInput::modified(Modifier::Control, KeyCode::Minus),
            Multiple => Modifier::Shift.into(),
            Area => Modifier::Control.into(),
            Line => Modifier::Alt.into(),
            SelectStructure => KeyCode::E.into(),
            Pipette => KeyCode::Q.into(),
            Zone => KeyCode::Space.into(),
            ClearZoning => KeyCode::Back.into(),
            RotateClipboardLeft => UserInput::modified(Modifier::Shift, KeyCode::R),
            RotateClipboardRight => KeyCode::R.into(),
            SnapToSelection => KeyCode::Return.into(),
            Pan => VirtualDPad::wasd().into(),
            MoveCursor => VirtualDPad::arrow_keys().into(),
            // Plus and Equals are swapped. See: https://github.com/rust-windowing/winit/issues/2682
            ZoomIn => KeyCode::Equals.into(),
            ZoomOut => KeyCode::Minus.into(),
            RotateCameraLeft => KeyCode::Z.into(),
            RotateCameraRight => KeyCode::C.into(),
        }
    }

    /// The default keybindings for gamepads.
    fn gamepad_binding(&self) -> UserInput {
        use GamepadButtonType::*;
        use PlayerAction::*;

        let camera_modifier = RightTrigger2;
        let radius_modifier = LeftTrigger;

        match self {
            PlayerAction::Select => South.into(),
            Deselect => East.into(),
            Multiple => RightTrigger.into(),
            IncreaseSelectionRadius => UserInput::chord([radius_modifier, DPadUp]),
            DecreaseSelectionRadius => UserInput::chord([radius_modifier, DPadDown]),
            Area => LeftTrigger.into(),
            Line => LeftTrigger2.into(),
            SelectStructure => RightThumb.into(),
            Pipette => West.into(),
            Zone => North.into(),
            ClearZoning => DPadUp.into(),
            RotateClipboardLeft => DPadLeft.into(),
            RotateClipboardRight => DPadRight.into(),
            SnapToSelection => GamepadButtonType::LeftThumb.into(),
            Pan => DualAxis::left_stick().into(),
            MoveCursor => DualAxis::right_stick().into(),
            ZoomIn => UserInput::chord([camera_modifier, DPadUp]),
            ZoomOut => UserInput::chord([camera_modifier, DPadDown]),
            RotateCameraLeft => UserInput::chord([camera_modifier, DPadLeft]),
            RotateCameraRight => UserInput::chord([camera_modifier, DPadRight]),
        }
    }

    /// The default key bindings
    fn default_input_map() -> InputMap<PlayerAction> {
        let mut input_map = InputMap::default();

        for variant in PlayerAction::variants() {
            input_map.insert(variant.kbm_binding(), variant.clone());
            input_map.insert(variant.gamepad_binding(), variant);
        }
        input_map
    }
}
