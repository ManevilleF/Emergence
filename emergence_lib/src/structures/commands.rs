//! Methods to use [`Commands`] to manipulate structures.

use bevy::{
    ecs::system::{Command, SystemState},
    prelude::{warn, Commands, DespawnRecursiveExt, Mut, Query, Res, World},
};
use hexx::Direction;
use rand::{rngs::ThreadRng, seq::SliceRandom, thread_rng};

use crate::{
    asset_management::manifest::Id,
    graphics::InheritedMaterial,
    items::{item_manifest::ItemManifest, recipe::RecipeManifest},
    organisms::OrganismBundle,
    player_interaction::clipboard::ClipboardData,
    signals::Emitter,
    simulation::geometry::{Facing, MapGeometry, TilePos},
    terrain::terrain_manifest::Terrain,
};

use super::{
    construction::{GhostBundle, GhostKind, PreviewBundle},
    crafting::{CraftingBundle, StorageInventory},
    structure_assets::StructureHandles,
    structure_manifest::{StructureKind, StructureManifest},
    StructureBundle,
};

/// An extension trait for [`Commands`] for working with structures.
pub(crate) trait StructureCommandsExt {
    /// Spawns a structure defined by `data` at `tile_pos`.
    ///
    /// Has no effect if the tile position is already occupied by an existing structure.
    fn spawn_structure(&mut self, tile_pos: TilePos, data: ClipboardData);

    /// Spawns a structure with randomized `data` at `tile_pos`.
    ///
    /// Some fields of data will be randomized.
    /// This is intended to be used for world generation.
    fn spawn_randomized_structure(
        &mut self,
        tile_pos: TilePos,
        data: ClipboardData,
        rng: &mut ThreadRng,
    );

    /// Despawns any structure at the provided `tile_pos`.
    ///
    /// Has no effect if the tile position is already empty.
    fn despawn_structure(&mut self, tile_pos: TilePos);

    /// Spawns a ghost with data defined by `data` at `tile_pos`.
    ///
    /// Replaces any existing ghost.
    fn spawn_ghost(&mut self, tile_pos: TilePos, data: ClipboardData);

    /// Despawns any ghost at the provided `tile_pos`.
    ///
    /// Has no effect if the tile position is already empty.
    fn despawn_ghost(&mut self, tile_pos: TilePos);

    /// Spawns a preview with data defined by `item` at `tile_pos`.
    ///
    /// Replaces any existing preview.
    fn spawn_preview(&mut self, tile_pos: TilePos, data: ClipboardData);
}

impl<'w, 's> StructureCommandsExt for Commands<'w, 's> {
    fn spawn_structure(&mut self, tile_pos: TilePos, data: ClipboardData) {
        self.add(SpawnStructureCommand {
            tile_pos,
            data,
            randomized: false,
        });
    }

    fn spawn_randomized_structure(
        &mut self,
        tile_pos: TilePos,
        mut data: ClipboardData,
        rng: &mut ThreadRng,
    ) {
        let direction = *Direction::ALL_DIRECTIONS.choose(rng).unwrap();
        data.facing = Facing { direction };

        self.add(SpawnStructureCommand {
            tile_pos,
            data,
            randomized: true,
        });
    }

    fn despawn_structure(&mut self, tile_pos: TilePos) {
        self.add(DespawnStructureCommand { tile_pos });
    }

    fn spawn_ghost(&mut self, tile_pos: TilePos, data: ClipboardData) {
        self.add(SpawnGhostCommand { tile_pos, data });
    }

    fn despawn_ghost(&mut self, tile_pos: TilePos) {
        self.add(DespawnGhostCommand { tile_pos });
    }

    fn spawn_preview(&mut self, tile_pos: TilePos, data: ClipboardData) {
        self.add(SpawnPreviewCommand { tile_pos, data });
    }
}

/// A [`Command`] used to spawn a structure via [`StructureCommandsExt`].
struct SpawnStructureCommand {
    /// The tile position at which to spawn the structure.
    tile_pos: TilePos,
    /// Data about the structure to spawn.
    data: ClipboardData,
    /// Should the generated structure be randomized
    randomized: bool,
}

impl Command for SpawnStructureCommand {
    fn write(self, world: &mut World) {
        let geometry = world.resource::<MapGeometry>();
        // Check that the tile is within the bounds of the map
        if !geometry.is_valid(self.tile_pos) {
            return;
        }

        let structure_id = self.data.structure_id;

        let mut system_state: SystemState<(
            Query<&Id<Terrain>>,
            Res<MapGeometry>,
            Res<StructureManifest>,
        )> = SystemState::new(world);

        let (terrain_query, geometry, manifest) = system_state.get(world);
        let structure_variety = manifest.get(structure_id).clone();

        // Check that the tiles needed are appropriate.
        if !geometry.can_build(
            self.tile_pos,
            structure_variety.footprint.rotated(self.data.facing),
            &terrain_query,
            structure_variety.allowed_terrain_types(),
        ) {
            return;
        }

        let structure_handles = world.resource::<StructureHandles>();

        let picking_mesh = structure_handles.picking_mesh.clone_weak();
        let scene_handle = structure_handles
            .scenes
            .get(&structure_id)
            .unwrap()
            .clone_weak();
        let world_pos = self.tile_pos.top_of_tile(world.resource::<MapGeometry>());

        let structure_entity = world
            .spawn(StructureBundle::new(
                self.tile_pos,
                self.data,
                picking_mesh,
                scene_handle,
                world_pos,
            ))
            .id();

        // PERF: these operations could be done in a single archetype move with more branching
        if let Some(organism_details) = &structure_variety.organism_variety {
            world
                .entity_mut(structure_entity)
                .insert(OrganismBundle::new(
                    organism_details.energy_pool.clone(),
                    organism_details.lifecycle.clone(),
                ));
        };

        match structure_variety.kind {
            StructureKind::Storage {
                max_slot_count,
                reserved_for,
            } => {
                world
                    .entity_mut(structure_entity)
                    .insert(StorageInventory::new(max_slot_count, reserved_for))
                    .insert(Emitter::default());
            }
            StructureKind::Crafting { starting_recipe } => {
                world.resource_scope(|world, recipe_manifest: Mut<RecipeManifest>| {
                    world.resource_scope(|world, item_manifest: Mut<ItemManifest>| {
                        world.resource_scope(|world, structure_manifest: Mut<StructureManifest>| {
                            let crafting_bundle = match self.randomized {
                                false => CraftingBundle::new(
                                    structure_id,
                                    starting_recipe,
                                    &recipe_manifest,
                                    &item_manifest,
                                    &structure_manifest,
                                ),
                                true => {
                                    let rng = &mut thread_rng();
                                    CraftingBundle::randomized(
                                        structure_id,
                                        starting_recipe,
                                        &recipe_manifest,
                                        &item_manifest,
                                        &structure_manifest,
                                        rng,
                                    )
                                }
                            };

                            world.entity_mut(structure_entity).insert(crafting_bundle);
                        })
                    })
                })
            }
        }

        let mut geometry = world.resource_mut::<MapGeometry>();
        geometry.add_structure(
            self.tile_pos,
            &structure_variety.footprint,
            structure_entity,
        );
    }
}

/// A [`Command`] used to despawn a structure via [`StructureCommandsExt`].
struct DespawnStructureCommand {
    /// The tile position at which the structure to be despawned is found.
    tile_pos: TilePos,
}

impl Command for DespawnStructureCommand {
    fn write(self, world: &mut World) {
        let mut geometry = world.resource_mut::<MapGeometry>();
        let maybe_entity = geometry.remove_structure(self.tile_pos);

        // Check that there's something there to despawn
        if maybe_entity.is_none() {
            return;
        }

        let structure_entity = maybe_entity.unwrap();
        // Make sure to despawn all children, which represent the meshes stored in the loaded gltf scene.
        world.entity_mut(structure_entity).despawn_recursive();
    }
}

/// A [`Command`] used to spawn a ghost via [`StructureCommandsExt`].
struct SpawnGhostCommand {
    /// The tile position at which to spawn the structure.
    tile_pos: TilePos,
    /// Data about the structure to spawn.
    data: ClipboardData,
}

impl Command for SpawnGhostCommand {
    fn write(self, world: &mut World) {
        let structure_id = self.data.structure_id;
        let geometry = world.resource::<MapGeometry>();

        // Check that the tile is within the bounds of the map
        if !geometry.is_valid(self.tile_pos) {
            return;
        }

        let mut system_state: SystemState<(
            Query<&Id<Terrain>>,
            Res<MapGeometry>,
            Res<StructureManifest>,
        )> = SystemState::new(world);

        let (terrain_query, geometry, manifest) = system_state.get(world);
        let structure_variety = manifest.get(structure_id).clone();

        // Check that the tiles needed are appropriate.
        if !geometry.can_build(
            self.tile_pos,
            structure_variety.footprint.rotated(self.data.facing),
            &terrain_query,
            structure_variety.allowed_terrain_types(),
        ) {
            return;
        }

        // Remove any existing ghosts
        let mut geometry = world.resource_mut::<MapGeometry>();
        let maybe_existing_ghost = geometry.remove_ghost(self.tile_pos);

        if let Some(existing_ghost) = maybe_existing_ghost {
            world.entity_mut(existing_ghost).despawn_recursive();
        }

        let structure_manifest = world.resource::<StructureManifest>();

        // Spawn a ghost
        let structure_handles = world.resource::<StructureHandles>();

        let picking_mesh = structure_handles.picking_mesh.clone_weak();
        let scene_handle = structure_handles
            .scenes
            .get(&structure_id)
            .unwrap()
            .clone_weak();
        let ghostly_handle = structure_handles
            .ghost_materials
            .get(&GhostKind::Ghost)
            .unwrap();
        let inherited_material = InheritedMaterial(ghostly_handle.clone_weak());

        let world_pos = self.tile_pos.top_of_tile(world.resource::<MapGeometry>());

        let ghost_entity = world
            .spawn(GhostBundle::new(
                self.tile_pos,
                self.data,
                structure_manifest,
                picking_mesh,
                scene_handle,
                inherited_material,
                world_pos,
            ))
            .id();

        // Update the index to reflect the new state
        world.resource_scope(|world, mut map_geometry: Mut<MapGeometry>| {
            let structure_manifest = world.resource::<StructureManifest>();
            let structure_variety = structure_manifest.get(structure_id);
            let footprint = &structure_variety.footprint;

            map_geometry.add_ghost(self.tile_pos, footprint, ghost_entity);
        });
    }
}

/// A [`Command`] used to despawn a ghost via [`StructureCommandsExt`].
struct DespawnGhostCommand {
    /// The tile position at which the structure to be despawned is found.
    tile_pos: TilePos,
}

impl Command for DespawnGhostCommand {
    fn write(self, world: &mut World) {
        let mut geometry = world.resource_mut::<MapGeometry>();
        let maybe_entity = geometry.remove_ghost(self.tile_pos);

        // Check that there's something there to despawn
        if maybe_entity.is_none() {
            return;
        }

        let ghost_entity = maybe_entity.unwrap();
        // Make sure to despawn all children, which represent the meshes stored in the loaded gltf scene.
        world.entity_mut(ghost_entity).despawn_recursive();
    }
}

/// A [`Command`] used to spawn a preview via [`StructureCommandsExt`].
struct SpawnPreviewCommand {
    /// The tile position at which to spawn the structure.
    tile_pos: TilePos,
    /// Data about the structure to spawn.
    data: ClipboardData,
}

impl Command for SpawnPreviewCommand {
    fn write(self, world: &mut World) {
        let structure_id = self.data.structure_id;
        let map_geometry = world.resource::<MapGeometry>();

        // Check that the tile is within the bounds of the map
        if !map_geometry.is_valid(self.tile_pos) {
            warn!("Preview position {:?} not valid.", self.tile_pos);
            return;
        }

        // Compute the world position
        let world_pos = self.tile_pos.top_of_tile(map_geometry);

        let mut system_state: SystemState<(
            Query<&Id<Terrain>>,
            Res<MapGeometry>,
            Res<StructureManifest>,
        )> = SystemState::new(world);

        let (terrain_query, geometry, manifest) = system_state.get(world);
        let structure_variety = manifest.get(structure_id).clone();

        // Check that the tiles needed are appropriate.
        let forbidden = !geometry.can_build(
            self.tile_pos,
            structure_variety.footprint.rotated(self.data.facing),
            &terrain_query,
            structure_variety.allowed_terrain_types(),
        );

        // Fetch the scene and material to use
        let structure_handles = world.resource::<StructureHandles>();
        let scene_handle = structure_handles
            .scenes
            .get(&self.data.structure_id)
            .unwrap()
            .clone_weak();

        let ghost_kind = match forbidden {
            true => GhostKind::ForbiddenPreview,
            false => GhostKind::Preview,
        };

        let preview_handle = structure_handles.ghost_materials.get(&ghost_kind).unwrap();
        let inherited_material = InheritedMaterial(preview_handle.clone_weak());

        // Spawn a preview
        world.spawn(PreviewBundle::new(
            self.tile_pos,
            self.data,
            scene_handle,
            inherited_material,
            world_pos,
        ));
    }
}
