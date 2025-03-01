//! Generating and representing terrain as game objects.

use bevy::ecs::system::Command;
use bevy::prelude::*;
use bevy_mod_raycast::RaycastMesh;

use crate::asset_management::manifest::plugin::ManifestPlugin;
use crate::asset_management::manifest::Id;
use crate::asset_management::AssetCollectionExt;
use crate::player_interaction::selection::ObjectInteraction;
use crate::player_interaction::zoning::Zoning;
use crate::simulation::geometry::{Height, MapGeometry, TilePos};
use crate::simulation::SimulationSet;

use self::terrain_assets::TerrainHandles;
use self::terrain_manifest::{RawTerrainManifest, Terrain};

pub(crate) mod terrain_assets;
pub mod terrain_manifest;

/// All logic and initialization needed for terrain.
pub(crate) struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugin(ManifestPlugin::<RawTerrainManifest>::new())
            .add_asset_collection::<TerrainHandles>()
            .add_system(
                respond_to_height_changes
                    .in_set(SimulationSet)
                    .in_schedule(CoreSchedule::FixedUpdate),
            );
    }
}

/// All of the components needed to define a piece of terrain.
#[derive(Bundle)]
struct TerrainBundle {
    /// The type of terrain
    terrain_id: Id<Terrain>,
    /// The location of this terrain hex
    tile_pos: TilePos,
    /// The height of this terrain hex
    height: Height,
    /// Makes the tiles pickable
    raycast_mesh: RaycastMesh<Terrain>,
    /// The mesh used for raycasting
    mesh: Handle<Mesh>,
    /// How is the terrain being interacted with?
    object_interaction: ObjectInteraction,
    /// The structure that should be built here.
    zoning: Zoning,
    /// The scene used to construct the terrain tile.
    scene_bundle: SceneBundle,
}

impl TerrainBundle {
    /// Creates a new Terrain entity.
    fn new(
        terrain_id: Id<Terrain>,
        tile_pos: TilePos,
        scene: Handle<Scene>,
        mesh: Handle<Mesh>,
        map_geometry: &MapGeometry,
    ) -> Self {
        let world_pos = tile_pos.into_world_pos(map_geometry);
        let scene_bundle = SceneBundle {
            scene,
            transform: Transform::from_translation(world_pos),
            ..Default::default()
        };

        let height = map_geometry.get_height(tile_pos).unwrap();

        TerrainBundle {
            terrain_id,
            tile_pos,
            height,
            raycast_mesh: RaycastMesh::<Terrain>::default(),
            mesh,
            object_interaction: ObjectInteraction::None,
            zoning: Zoning::None,
            scene_bundle,
        }
    }
}

/// Updates the game state appropriately whenever the height of a tile is changed.
fn respond_to_height_changes(
    mut terrain_query: Query<(Ref<Height>, &TilePos, &mut Transform, &Children)>,
    mut column_query: Query<&mut Transform, (With<Parent>, Without<Height>)>,
    mut map_geometry: ResMut<MapGeometry>,
) {
    for (height, &tile_pos, mut transform, children) in terrain_query.iter_mut() {
        if height.is_changed() {
            map_geometry.update_height(tile_pos, *height);
            transform.translation.y = height.into_world_pos();
            // During terrain initialization we ensure that the column is always the 0th child
            let column_child = children[0];
            let mut column_transform = column_query.get_mut(column_child).unwrap();
            *column_transform = height.column_transform();
        }
    }
}

/// Constructs a new [`Terrain`] entity.
///
/// The order of the chidlren *must* be:
/// 0: column
/// 1: overlay
/// 2: scene root
pub(crate) struct SpawnTerrainCommand {
    /// The position to spawn the tile
    pub(crate) tile_pos: TilePos,
    /// The height of the tile
    pub(crate) height: Height,
    /// The type of tile
    pub(crate) terrain_id: Id<Terrain>,
}

impl Command for SpawnTerrainCommand {
    fn write(self, world: &mut World) {
        let handles = world.resource::<TerrainHandles>();
        let scene_handle = handles.scenes.get(&self.terrain_id).unwrap().clone_weak();
        let mesh = handles.topper_mesh.clone_weak();
        let mut map_geometry = world.resource_mut::<MapGeometry>();

        // Store the height, so it can be used below
        map_geometry.update_height(self.tile_pos, self.height);

        // Drop the borrow so the borrow checker is happy
        let map_geometry = world.resource::<MapGeometry>();

        // Spawn the terrain entity
        let terrain_entity = world
            .spawn(TerrainBundle::new(
                self.terrain_id,
                self.tile_pos,
                scene_handle,
                mesh,
                map_geometry,
            ))
            .id();

        // Spawn the column as the 0th child of the tile entity
        // The scene bundle will be added as the first child
        let handles = world.resource::<TerrainHandles>();
        let column_bundle = PbrBundle {
            mesh: handles.column_mesh.clone_weak(),
            material: handles.column_material.clone_weak(),
            ..Default::default()
        };

        let hex_column = world.spawn(column_bundle).id();
        world.entity_mut(terrain_entity).add_child(hex_column);

        let handles = world.resource::<TerrainHandles>();
        /// Makes the overlays ever so slightly larger than their base to avoid z-fighting.
        ///
        /// This value should be very slightly larger than 1.0
        const OVERLAY_OVERSIZE_SCALE: f32 = 1.001;

        let overlay_bundle = PbrBundle {
            mesh: handles.topper_mesh.clone_weak(),
            visibility: Visibility::Hidden,
            transform: Transform::from_scale(Vec3 {
                x: OVERLAY_OVERSIZE_SCALE,
                y: OVERLAY_OVERSIZE_SCALE,
                z: OVERLAY_OVERSIZE_SCALE,
            }),
            ..Default::default()
        };
        let overlay = world.spawn(overlay_bundle).id();
        world.entity_mut(terrain_entity).add_child(overlay);

        // Update the index of what terrain is where
        let mut map_geometry = world.resource_mut::<MapGeometry>();
        map_geometry.add_terrain(self.tile_pos, terrain_entity);
    }
}
