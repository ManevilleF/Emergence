//! Loads and manages asset state for in-game UI

use bevy::{asset::LoadState, prelude::*, utils::HashMap};
use core::fmt::Debug;
use core::hash::Hash;

use crate::{
    asset_management::{manifest::Id, AssetState, Loadable},
    player_interaction::terraform::TerraformingChoice,
    structures::structure_manifest::{Structure, StructureManifest},
    terrain::terrain_manifest::TerrainManifest,
};

/// Stores all structural elements of the UI: buttons, frames, widgets and so on
#[derive(Resource)]
pub(crate) struct UiElements {
    /// The background image used by hex menus
    pub(crate) hex_menu_background: Handle<Image>,
}

impl Loadable for UiElements {
    const STAGE: AssetState = AssetState::LoadAssets;

    fn initialize(world: &mut World) {
        let asset_server = world.resource::<AssetServer>();
        world.insert_resource(UiElements {
            hex_menu_background: asset_server.load("ui/hex-menu-background.png"),
        });
    }

    fn load_state(&self, asset_server: &AssetServer) -> bevy::asset::LoadState {
        asset_server.get_load_state(&self.hex_menu_background)
    }
}

/// Stores the icons of type `D`.
#[derive(Resource)]
pub(crate) struct Icons<D: Send + Sync + 'static> {
    /// The map used to look-up handles
    map: HashMap<D, Handle<Image>>,
}

impl<D: Send + Sync + 'static + Hash + Eq> Icons<D> {
    /// Returns a weakly cloned handle to the image of the icon corresponding to `structure_id`.
    pub(crate) fn get(&self, structure_id: D) -> Handle<Image> {
        self.map.get(&structure_id).unwrap().clone_weak()
    }
}

impl FromWorld for Icons<Id<Structure>> {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();
        let structure_manifest = world.resource::<StructureManifest>();
        let structure_names = structure_manifest.prototype_names();

        let mut map = HashMap::new();

        for id in structure_names {
            let structure_id = Id::from_name(id);
            let structure_path = format!("icons/structures/{id}.png");
            let icon = asset_server.load(structure_path);
            map.insert(structure_id, icon);
        }

        Icons { map }
    }
}

impl FromWorld for Icons<TerraformingChoice> {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();
        let mut map = HashMap::new();

        let terrain_names = world.resource::<TerrainManifest>().names();

        for id in terrain_names {
            let terrain_id = Id::from_name(id);
            let terrain_path = format!("icons/terrain/{id}.png");
            let icon = asset_server.load(terrain_path);

            let choice = TerraformingChoice::Change(terrain_id);
            map.insert(choice, icon);
        }

        map.insert(
            TerraformingChoice::Lower,
            asset_server.load("icons/terraforming/lower.png"),
        );

        map.insert(
            TerraformingChoice::Raise,
            asset_server.load("icons/terraforming/raise.png"),
        );

        Icons { map }
    }
}

impl<D: Send + Sync + Debug + 'static> Loadable for Icons<D>
where
    Icons<D>: FromWorld,
{
    const STAGE: AssetState = AssetState::LoadAssets;

    fn initialize(world: &mut World) {
        let icons = Self::from_world(world);
        world.insert_resource(icons);
    }

    fn load_state(&self, asset_server: &AssetServer) -> bevy::asset::LoadState {
        for (data, icon_handle) in &self.map {
            let load_state = asset_server.get_load_state(icon_handle);

            if load_state != LoadState::Loaded {
                info!("{data:?}'s icon is {load_state:?}");
                return load_state;
            }
        }

        LoadState::Loaded
    }
}
