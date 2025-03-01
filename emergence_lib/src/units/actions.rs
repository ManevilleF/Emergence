//! What are units currently doing?

use bevy::{ecs::query::WorldQuery, prelude::*};
use leafwing_abilities::prelude::Pool;
use rand::{rngs::ThreadRng, seq::SliceRandom, thread_rng};

use crate::{
    asset_management::manifest::Id,
    items::{
        item_manifest::{Item, ItemManifest},
        ItemCount,
    },
    organisms::{energy::EnergyPool, lifecycle::Lifecycle},
    signals::{SignalStrength, SignalType, Signals},
    simulation::geometry::{Facing, MapGeometry, RotationDirection, TilePos},
    structures::{
        commands::StructureCommandsExt,
        construction::{DemolitionQuery, MarkedForDemolition},
        crafting::{
            CraftingState, InputInventory, OutputInventory, StorageInventory, WorkersPresent,
            WorkplaceQuery,
        },
        structure_manifest::Structure,
    },
    terrain::terrain_manifest::{Terrain, TerrainManifest},
};

use super::{
    goals::Goal,
    impatience::ImpatiencePool,
    item_interaction::UnitInventory,
    unit_manifest::{Unit, UnitManifest},
};

/// Ticks the timer for each [`CurrentAction`].
pub(super) fn advance_action_timer(
    mut units_query: Query<&mut CurrentAction>,
    time: Res<FixedTime>,
) {
    let delta = time.period;

    for mut current_action in units_query.iter_mut() {
        current_action.timer.tick(delta);
    }
}

/// Choose the unit's action for this turn
pub(super) fn choose_actions(
    mut units_query: Query<
        (&TilePos, &Facing, &Goal, &mut CurrentAction, &UnitInventory),
        With<Id<Unit>>,
    >,
    // We shouldn't be dropping off new stuff at structures that are about to be destroyed!
    input_inventory_query: Query<
        AnyOf<(&InputInventory, &StorageInventory)>,
        Without<MarkedForDemolition>,
    >,
    // But we can take their items away
    output_inventory_query: Query<AnyOf<(&OutputInventory, &StorageInventory)>>,
    workplace_query: WorkplaceQuery,
    demolition_query: DemolitionQuery,
    map_geometry: Res<MapGeometry>,
    signals: Res<Signals>,
    terrain_query: Query<&Id<Terrain>>,
    terrain_manifest: Res<TerrainManifest>,
    item_manifest: Res<ItemManifest>,
) {
    let rng = &mut thread_rng();
    let map_geometry = map_geometry.into_inner();

    for (&unit_tile_pos, facing, goal, mut action, unit_inventory) in units_query.iter_mut() {
        if action.finished() {
            *action = match goal {
                // Alternate between spinning and moving forward.
                Goal::Wander { .. } => match action.action() {
                    UnitAction::Spin { .. } => CurrentAction::move_forward(
                        unit_tile_pos,
                        facing,
                        map_geometry,
                        &terrain_query,
                        &terrain_manifest,
                    ),
                    _ => CurrentAction::random_spin(rng),
                },
                Goal::Pickup(item_id) => {
                    if unit_inventory.is_some() && unit_inventory.unwrap() != *item_id {
                        CurrentAction::abandon()
                    } else {
                        CurrentAction::find_item(
                            *item_id,
                            unit_tile_pos,
                            facing,
                            goal,
                            &output_inventory_query,
                            &signals,
                            rng,
                            &terrain_query,
                            &terrain_manifest,
                            map_geometry,
                        )
                    }
                }
                Goal::Store(item_id) => {
                    if unit_inventory.is_some() && unit_inventory.unwrap() != *item_id {
                        CurrentAction::abandon()
                    } else {
                        CurrentAction::find_storage(
                            *item_id,
                            unit_tile_pos,
                            facing,
                            goal,
                            &input_inventory_query,
                            &signals,
                            rng,
                            &terrain_query,
                            &terrain_manifest,
                            &item_manifest,
                            map_geometry,
                        )
                    }
                }
                Goal::Deliver(item_id) => {
                    if unit_inventory.is_some() && unit_inventory.unwrap() != *item_id {
                        CurrentAction::abandon()
                    } else {
                        CurrentAction::find_delivery(
                            *item_id,
                            unit_tile_pos,
                            facing,
                            goal,
                            &input_inventory_query,
                            &signals,
                            rng,
                            &terrain_query,
                            &terrain_manifest,
                            map_geometry,
                        )
                    }
                }
                Goal::Eat(item_id) => {
                    if let Some(held_item) = unit_inventory.held_item {
                        if held_item == *item_id {
                            CurrentAction::eat()
                        } else {
                            CurrentAction::abandon()
                        }
                    } else {
                        CurrentAction::find_item(
                            *item_id,
                            unit_tile_pos,
                            facing,
                            goal,
                            &output_inventory_query,
                            &signals,
                            rng,
                            &terrain_query,
                            &terrain_manifest,
                            map_geometry,
                        )
                    }
                }
                Goal::Work(structure_id) => CurrentAction::find_workplace(
                    *structure_id,
                    unit_tile_pos,
                    facing,
                    &workplace_query,
                    &signals,
                    rng,
                    &terrain_query,
                    &terrain_manifest,
                    map_geometry,
                ),
                Goal::Demolish(structure_id) => CurrentAction::find_demolition_site(
                    *structure_id,
                    unit_tile_pos,
                    facing,
                    &demolition_query,
                    &signals,
                    rng,
                    &terrain_query,
                    &terrain_manifest,
                    map_geometry,
                ),
            }
        }
    }
}

/// Exhaustively handles the setup for each planned action
pub(super) fn start_actions(
    mut unit_query: Query<&mut CurrentAction>,
    mut workplace_query: Query<&mut WorkersPresent>,
) {
    for mut action in unit_query.iter_mut() {
        if action.just_started {
            if let Some(workplace_entity) = action.action().workplace() {
                if let Ok(mut workers_present) = workplace_query.get_mut(workplace_entity) {
                    // This has a side effect of adding the worker to the workplace
                    let result = workers_present.add_worker();
                    if result.is_err() {
                        *action = CurrentAction::idle();
                    }
                } else {
                    warn!("Unit tried to start working at an entity that is not a workplace!");
                }
            }

            action.just_started = false;
        }
    }
}

/// Exhaustively handles the cleanup for each planned action
pub(super) fn finish_actions(
    mut unit_query: Query<ActionDataQuery>,
    mut inventory_query: Query<
        AnyOf<(
            &mut InputInventory,
            &mut OutputInventory,
            &mut StorageInventory,
        )>,
    >,
    mut workplace_query: Query<(&CraftingState, &mut WorkersPresent)>,
    // This must be compatible with unit_query
    structure_query: Query<&TilePos, (With<Id<Structure>>, Without<Goal>)>,
    map_geometry: Res<MapGeometry>,
    item_manifest: Res<ItemManifest>,
    unit_manifest: Res<UnitManifest>,
    signals: Res<Signals>,
    mut commands: Commands,
) {
    let item_manifest = &*item_manifest;

    for mut unit in unit_query.iter_mut() {
        if unit.action.finished() {
            // Take workers off of the job once actions complete
            if let Some(workplace_entity) = unit.action.action().workplace() {
                if let Ok(workplace) = workplace_query.get_mut(workplace_entity) {
                    let (.., mut workers_present) = workplace;
                    // FIXME: this isn't robust to units dying
                    workers_present.remove_worker();
                } else {
                    warn!("Unit was working at an entity that is not a workplace!");
                }
            }

            match unit.action.action() {
                UnitAction::Idle => {
                    unit.impatience.increment();
                }
                UnitAction::PickUp {
                    item_id,
                    output_entity,
                } => {
                    if let Ok((_, maybe_output_inventory, maybe_storage_inventory)) =
                        inventory_query.get_mut(*output_entity)
                    {
                        *unit.goal = match unit.unit_inventory.held_item {
                            // We shouldn't be holding anything yet, but if we are get rid of it
                            Some(held_item_id) => Goal::Store(held_item_id),
                            None => {
                                let item_count = ItemCount::new(*item_id, 1);
                                let transfer_result = if let Some(mut output_inventory) =
                                    maybe_output_inventory
                                {
                                    output_inventory.remove_item_all_or_nothing(&item_count)
                                } else if let Some(mut storage_inventory) = maybe_storage_inventory
                                {
                                    storage_inventory.remove_item_all_or_nothing(&item_count)
                                } else {
                                    unreachable!()
                                };

                                // If our unit's all loaded, swap to delivering it
                                match transfer_result {
                                    Ok(()) => {
                                        unit.unit_inventory.held_item = Some(*item_id);
                                        if signals.get(SignalType::Pull(*item_id), *unit.tile_pos)
                                            > SignalStrength::ZERO
                                        {
                                            // If we can see any `Pull` signals of the right type, deliver the item.
                                            Goal::Deliver(*item_id)
                                        } else {
                                            // Otherwise, simply store it
                                            Goal::Store(*item_id)
                                        }
                                    }
                                    Err(..) => Goal::Pickup(*item_id),
                                }
                            }
                        }
                    } else {
                        // If the target isn't there, pick a new goal
                        *unit.goal = Goal::default();
                    }
                }
                UnitAction::DropOff {
                    item_id,
                    input_entity,
                } => {
                    if let Ok((maybe_input_inventory, _, maybe_storage_inventory)) =
                        inventory_query.get_mut(*input_entity)
                    {
                        *unit.goal = match unit.unit_inventory.held_item {
                            // We should be holding something, if we're not find something else to do
                            None => Goal::default(),
                            Some(held_item_id) => {
                                if held_item_id == *item_id {
                                    let item_count = ItemCount::new(held_item_id, 1);
                                    let transfer_result =
                                        if let Some(mut input_inventory) = maybe_input_inventory {
                                            input_inventory
                                                .add_item_all_or_nothing(&item_count, item_manifest)
                                        } else if let Some(mut storage_inventory) =
                                            maybe_storage_inventory
                                        {
                                            storage_inventory
                                                .add_item_all_or_nothing(&item_count, item_manifest)
                                        } else {
                                            unreachable!()
                                        };

                                    // If our unit is unloaded, swap to wandering to find something else to do
                                    match transfer_result {
                                        Ok(()) => {
                                            unit.unit_inventory.held_item = None;
                                            Goal::default()
                                        }
                                        Err(..) => Goal::Store(held_item_id),
                                    }
                                } else {
                                    // Somehow we're holding the wrong thing
                                    Goal::Store(held_item_id)
                                }
                            }
                        }
                    } else {
                        // If the target isn't there, pick a new goal
                        *unit.goal = Goal::default();
                    }
                }
                UnitAction::Spin { rotation_direction } => match rotation_direction {
                    RotationDirection::Left => unit.facing.rotate_left(),
                    RotationDirection::Right => unit.facing.rotate_right(),
                },
                UnitAction::MoveForward => {
                    let direction = unit.facing.direction;
                    let target_tile = unit.tile_pos.neighbor(direction);

                    *unit.tile_pos = target_tile;
                    unit.transform.translation = target_tile.top_of_tile(&map_geometry);
                }
                UnitAction::Work { structure_entity } => {
                    let mut success = false;

                    if let Ok((CraftingState::InProgress { .. }, workers_present)) =
                        workplace_query.get_mut(*structure_entity)
                    {
                        if workers_present.needs_more() {
                            success = true;
                        }
                    }

                    if !success {
                        *unit.goal = Goal::default();
                    }
                }
                UnitAction::Demolish { structure_entity } => {
                    if let Ok(&structure_tile_pos) = structure_query.get(*structure_entity) {
                        // TODO: this should probably take time and use work?
                        commands.despawn_structure(structure_tile_pos);
                    }

                    // Whether we succeeded or failed, pick something else to do
                    *unit.goal = Goal::default();
                }
                UnitAction::Eat => {
                    if let Some(held_item) = unit.unit_inventory.held_item {
                        let unit_data = unit_manifest.get(*unit.unit_id);

                        let diet = &unit_data.diet;

                        if held_item == diet.item() {
                            let proposed = unit.energy_pool.current() + diet.energy();
                            unit.energy_pool.set_current(proposed);
                            unit.lifecycle.record_energy_gained(diet.energy());
                        }
                    }

                    unit.unit_inventory.held_item = None;
                }
                UnitAction::Abandon => {
                    // TODO: actually put these dropped items somewhere
                    unit.unit_inventory.held_item = None;
                }
            }
        }
    }
}

/// All of the data needed to handle unit actions correctly
#[derive(WorldQuery)]
#[world_query(mutable)]
pub(super) struct ActionDataQuery {
    /// The [`Id`] of the unit type
    unit_id: &'static Id<Unit>,
    /// The unit's goal
    goal: &'static mut Goal,
    /// The unit's action
    action: &'static CurrentAction,
    /// The unit's progress towards any transformations
    lifecycle: &'static mut Lifecycle,
    /// What the unit is holding
    unit_inventory: &'static mut UnitInventory,
    /// The unit's spatial position for rendering
    transform: &'static mut Transform,
    /// The tile that the unit is on
    tile_pos: &'static mut TilePos,
    /// How much energy the unit has
    energy_pool: &'static mut EnergyPool,
    /// How frustrated this unit is about not being able to progress towards its goal
    impatience: &'static mut ImpatiencePool,
    /// The direction this unit is facing
    facing: &'static mut Facing,
}

/// An action that a unit can take.
#[derive(Default, Clone, Debug)]
pub(super) enum UnitAction {
    /// Do nothing for now
    #[default]
    Idle,
    /// Pick up the `item_id` from the `output_entity.
    PickUp {
        /// The item to pickup.
        item_id: Id<Item>,
        /// The entity to grab it from, which must have an [`OutputInventory`] or [`StorageInventory`] component.
        output_entity: Entity,
    },
    /// Drops off the `item_id` at the `output_entity`.
    DropOff {
        /// The item that this unit is carrying that we should drop off.
        item_id: Id<Item>,
        /// The entity to drop it off at, which must have an [`InputInventory`] or [`StorageInventory`] component.
        input_entity: Entity,
    },
    /// Perform work at the provided `structure_entity`
    Work {
        /// The structure to work at.
        structure_entity: Entity,
    },
    /// Attempt to deconstruct the provided `structure_entity`
    Demolish {
        /// The structure to work at.
        structure_entity: Entity,
    },
    /// Spin left or right.
    Spin {
        /// The direction to turn in.
        rotation_direction: RotationDirection,
    },
    /// Move one tile forward, as determined by the unit's [`Facing`].
    MoveForward,
    /// Eats one of the currently held object
    Eat,
    /// Abandon whatever you are currently holding
    Abandon,
}

impl UnitAction {
    /// Gets the workplace [`Entity`] that this action is targeting, if any.
    fn workplace(&self) -> Option<Entity> {
        match self {
            UnitAction::Work { structure_entity }
            | UnitAction::Demolish { structure_entity }
            | UnitAction::DropOff {
                item_id: _,
                input_entity: structure_entity,
            }
            | UnitAction::PickUp {
                item_id: _,
                output_entity: structure_entity,
            } => Some(*structure_entity),
            _ => None,
        }
    }

    /// Pretty formatting for this type
    pub(crate) fn display(&self, item_manifest: &ItemManifest) -> String {
        match self {
            UnitAction::Idle => "Idling".to_string(),
            UnitAction::PickUp {
                item_id,
                output_entity,
            } => format!(
                "Picking up {} from {output_entity:?}",
                item_manifest.name(*item_id)
            ),
            UnitAction::DropOff {
                item_id,
                input_entity,
            } => format!(
                "Dropping off {} at {input_entity:?}",
                item_manifest.name(*item_id)
            ),
            UnitAction::Work { structure_entity } => format!("Working at {structure_entity:?}"),
            UnitAction::Demolish { structure_entity } => {
                format!("Demolishing {structure_entity:?}")
            }
            UnitAction::Spin { rotation_direction } => format!("Spinning {rotation_direction}"),
            UnitAction::MoveForward => "Moving forward".to_string(),
            UnitAction::Eat => "Eating".to_string(),
            UnitAction::Abandon => "Abandoning held object".to_string(),
        }
    }
}

#[derive(Component, Clone, Debug)]
/// The action a unit is undertaking.
pub(crate) struct CurrentAction {
    /// The type of action being undertaken.
    action: UnitAction,
    /// The amount of time left to complete the action.
    timer: Timer,
    /// Did this action just start?
    just_started: bool,
}

impl Default for CurrentAction {
    fn default() -> Self {
        CurrentAction::idle()
    }
}

impl CurrentAction {
    /// Pretty formatting for this type
    pub(crate) fn display(&self, item_manifest: &ItemManifest) -> String {
        let action = &self.action;
        let time_remaining = self.timer.remaining_secs();

        format!(
            "{}\nRemaining: {time_remaining:.2} s.",
            action.display(item_manifest)
        )
    }

    /// Get the action that the unit is currently undertaking.
    pub(super) fn action(&self) -> &UnitAction {
        &self.action
    }

    /// Have we waited long enough to perform this action?
    pub(super) fn finished(&self) -> bool {
        self.timer.finished()
    }

    /// Attempt to locate a source of the provided `item_id`.
    fn find_item(
        item_id: Id<Item>,
        unit_tile_pos: TilePos,
        facing: &Facing,
        goal: &Goal,
        output_inventory_query: &Query<AnyOf<(&OutputInventory, &StorageInventory)>>,
        signals: &Signals,
        rng: &mut ThreadRng,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        map_geometry: &MapGeometry,
    ) -> CurrentAction {
        let neighboring_tiles = unit_tile_pos.all_neighbors(map_geometry);
        let mut sources: Vec<(Entity, TilePos)> = Vec::new();

        for tile_pos in neighboring_tiles {
            if let Some(structure_entity) = map_geometry.get_structure(tile_pos) {
                if let Ok((maybe_output_inventory, maybe_storage_inventory)) =
                    output_inventory_query.get(structure_entity)
                {
                    if let Some(output_inventory) = maybe_output_inventory {
                        if output_inventory.item_count(item_id) > 0 {
                            sources.push((structure_entity, tile_pos));
                        }
                    } else if let Some(storage_inventory) = maybe_storage_inventory {
                        if storage_inventory.item_count(item_id) > 0 {
                            sources.push((structure_entity, tile_pos));
                        }
                    } else {
                        error!("output_inventory_query contained an object with neither an output nor storage inventory.")
                    }
                }
            }
        }

        if let Some((output_entity, output_tile_pos)) = sources.choose(rng) {
            CurrentAction::pickup(
                item_id,
                *output_entity,
                facing,
                unit_tile_pos,
                *output_tile_pos,
            )
        } else if let Some(upstream) = signals.upstream(unit_tile_pos, goal, map_geometry) {
            CurrentAction::move_or_spin(
                unit_tile_pos,
                upstream,
                facing,
                terrain_query,
                terrain_manifest,
                map_geometry,
            )
        } else {
            CurrentAction::idle()
        }
    }

    /// Attempt to locate a place to put an item of type `item_id`.
    #[allow(clippy::collapsible_match)]
    fn find_storage(
        item_id: Id<Item>,
        unit_tile_pos: TilePos,
        facing: &Facing,
        goal: &Goal,
        input_inventory_query: &Query<
            AnyOf<(&InputInventory, &StorageInventory)>,
            Without<MarkedForDemolition>,
        >,
        signals: &Signals,
        rng: &mut ThreadRng,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        item_manifest: &ItemManifest,
        map_geometry: &MapGeometry,
    ) -> CurrentAction {
        let neighboring_tiles = unit_tile_pos.all_neighbors(map_geometry);
        let mut receptacles: Vec<(Entity, TilePos)> = Vec::new();

        for tile_pos in neighboring_tiles {
            // Ghosts
            if let Some(ghost_entity) = map_geometry.get_ghost(tile_pos) {
                if let Ok((maybe_input_inventory, ..)) = input_inventory_query.get(ghost_entity) {
                    if let Some(input_inventory) = maybe_input_inventory {
                        if input_inventory.remaining_reserved_space_for_item(item_id) > 0 {
                            receptacles.push((ghost_entity, tile_pos));
                        }
                    }
                }
            }

            // Structures
            if let Some(structure_entity) = map_geometry.get_structure(tile_pos) {
                if let Ok((maybe_input_inventory, maybe_storage_inventory)) =
                    input_inventory_query.get(structure_entity)
                {
                    if let Some(input_inventory) = maybe_input_inventory {
                        if input_inventory.remaining_reserved_space_for_item(item_id) > 0 {
                            receptacles.push((structure_entity, tile_pos));
                        }
                    } else if let Some(storage_inventory) = maybe_storage_inventory {
                        if storage_inventory.remaining_space_for_item(item_id, item_manifest) > 0 {
                            receptacles.push((structure_entity, tile_pos));
                        }
                    } else {
                        error!("input_inventory_query contained an object with neither an input nor storage inventory.")
                    }
                }
            }
        }

        if let Some((input_entity, input_tile_pos)) = receptacles.choose(rng) {
            CurrentAction::dropoff(
                item_id,
                *input_entity,
                facing,
                unit_tile_pos,
                *input_tile_pos,
            )
        } else if let Some(upstream) = signals.upstream(unit_tile_pos, goal, map_geometry) {
            CurrentAction::move_or_spin(
                unit_tile_pos,
                upstream,
                facing,
                terrain_query,
                terrain_manifest,
                map_geometry,
            )
        } else {
            CurrentAction::idle()
        }
    }

    /// Attempt to locate a place to put an item of type `item_id`.
    #[allow(clippy::collapsible_match)]
    fn find_delivery(
        item_id: Id<Item>,
        unit_tile_pos: TilePos,
        facing: &Facing,
        goal: &Goal,
        input_inventory_query: &Query<
            AnyOf<(&InputInventory, &StorageInventory)>,
            Without<MarkedForDemolition>,
        >,
        signals: &Signals,
        rng: &mut ThreadRng,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        map_geometry: &MapGeometry,
    ) -> CurrentAction {
        let neighboring_tiles = unit_tile_pos.all_neighbors(map_geometry);
        let mut receptacles: Vec<(Entity, TilePos)> = Vec::new();

        for tile_pos in neighboring_tiles {
            // Ghosts
            if let Some(ghost_entity) = map_geometry.get_ghost(tile_pos) {
                if let Ok((maybe_input_inventory, ..)) = input_inventory_query.get(ghost_entity) {
                    if let Some(input_inventory) = maybe_input_inventory {
                        if input_inventory.remaining_reserved_space_for_item(item_id) > 0 {
                            receptacles.push((ghost_entity, tile_pos));
                        }
                    }
                }
            }

            // Structures
            if let Some(structure_entity) = map_geometry.get_structure(tile_pos) {
                // We deliberately avoid storage locations here, our goal is to complete a delivery!
                if let Ok((maybe_input_inventory, _maybe_storage_inventory)) =
                    input_inventory_query.get(structure_entity)
                {
                    if let Some(input_inventory) = maybe_input_inventory {
                        if input_inventory.remaining_reserved_space_for_item(item_id) > 0 {
                            receptacles.push((structure_entity, tile_pos));
                        }
                    }
                }
            }
        }

        if let Some((input_entity, input_tile_pos)) = receptacles.choose(rng) {
            CurrentAction::dropoff(
                item_id,
                *input_entity,
                facing,
                unit_tile_pos,
                *input_tile_pos,
            )
        } else if let Some(upstream) = signals.upstream(unit_tile_pos, goal, map_geometry) {
            CurrentAction::move_or_spin(
                unit_tile_pos,
                upstream,
                facing,
                terrain_query,
                terrain_manifest,
                map_geometry,
            )
        } else {
            CurrentAction::idle()
        }
    }

    /// Attempt to find a structure of type `structure_id` to perform work
    fn find_workplace(
        structure_id: Id<Structure>,
        unit_tile_pos: TilePos,
        facing: &Facing,
        workplace_query: &WorkplaceQuery,
        signals: &Signals,
        rng: &mut ThreadRng,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        map_geometry: &MapGeometry,
    ) -> CurrentAction {
        let ahead = unit_tile_pos.neighbor(facing.direction);
        if let Some(workplace) = workplace_query.needs_work(ahead, structure_id, map_geometry) {
            CurrentAction::work(workplace)
        // Let units work even if they're standing on the structure
        // This is particularly relevant in the case of ghosts, where it's easy enough to end up on top of the structure trying to work on it
        } else if let Some(workplace) =
            workplace_query.needs_work(unit_tile_pos, structure_id, map_geometry)
        {
            CurrentAction::work(workplace)
        } else {
            let neighboring_tiles = unit_tile_pos.all_neighbors(map_geometry);
            let mut workplaces: Vec<(Entity, TilePos)> = Vec::new();

            for neighbor in neighboring_tiles {
                if let Some(workplace) =
                    workplace_query.needs_work(neighbor, structure_id, map_geometry)
                {
                    workplaces.push((workplace, neighbor));
                }
            }

            if let Some(chosen_workplace) = workplaces.choose(rng) {
                CurrentAction::move_or_spin(
                    unit_tile_pos,
                    chosen_workplace.1,
                    facing,
                    terrain_query,
                    terrain_manifest,
                    map_geometry,
                )
            } else if let Some(upstream) =
                signals.upstream(unit_tile_pos, &Goal::Work(structure_id), map_geometry)
            {
                CurrentAction::move_or_spin(
                    unit_tile_pos,
                    upstream,
                    facing,
                    terrain_query,
                    terrain_manifest,
                    map_geometry,
                )
            } else {
                CurrentAction::idle()
            }
        }
    }

    /// Attempt to find a structure of type `structure_id` to perform work
    fn find_demolition_site(
        structure_id: Id<Structure>,
        unit_tile_pos: TilePos,
        facing: &Facing,
        demolition_query: &DemolitionQuery,
        signals: &Signals,
        rng: &mut ThreadRng,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        map_geometry: &MapGeometry,
    ) -> CurrentAction {
        let ahead = unit_tile_pos.neighbor(facing.direction);
        if let Some(workplace) =
            demolition_query.needs_demolition(ahead, structure_id, map_geometry)
        {
            CurrentAction::demolish(workplace)
        } else if let Some(workplace) =
            demolition_query.needs_demolition(unit_tile_pos, structure_id, map_geometry)
        {
            CurrentAction::demolish(workplace)
        } else {
            let neighboring_tiles = unit_tile_pos.all_neighbors(map_geometry);
            let mut demo_sites: Vec<(Entity, TilePos)> = Vec::new();

            for neighbor in neighboring_tiles {
                if let Some(demo_site) =
                    demolition_query.needs_demolition(neighbor, structure_id, map_geometry)
                {
                    demo_sites.push((demo_site, neighbor));
                }
            }

            if let Some(chosen_demo_site) = demo_sites.choose(rng) {
                CurrentAction::move_or_spin(
                    unit_tile_pos,
                    chosen_demo_site.1,
                    facing,
                    terrain_query,
                    terrain_manifest,
                    map_geometry,
                )
            } else if let Some(upstream) =
                signals.upstream(unit_tile_pos, &Goal::Demolish(structure_id), map_geometry)
            {
                CurrentAction::move_or_spin(
                    unit_tile_pos,
                    upstream,
                    facing,
                    terrain_query,
                    terrain_manifest,
                    map_geometry,
                )
            } else {
                CurrentAction::idle()
            }
        }
    }

    /// Spins 60 degrees left or right.
    pub(super) fn spin(rotation_direction: RotationDirection) -> Self {
        CurrentAction {
            action: UnitAction::Spin { rotation_direction },
            timer: Timer::from_seconds(0.1, TimerMode::Once),
            just_started: true,
        }
    }

    /// Rotate to face the `required_direction`.
    fn spin_towards(facing: &Facing, required_direction: hexx::Direction) -> Self {
        let mut working_direction_left = facing.direction;
        let mut working_direction_right = facing.direction;

        // Let's race!
        // Left gets an arbitrary unfair advantage though.
        // PERF: this could use a lookup table instead, and would probably be faster
        loop {
            working_direction_left = working_direction_left.left();
            if working_direction_left == required_direction {
                return CurrentAction::spin(RotationDirection::Left);
            }

            working_direction_right = working_direction_right.right();
            if working_direction_right == required_direction {
                return CurrentAction::spin(RotationDirection::Right);
            }
        }
    }

    /// Spins 60 degrees in a random direction
    pub(super) fn random_spin(rng: &mut ThreadRng) -> Self {
        let rotation_direction = RotationDirection::random(rng);

        CurrentAction::spin(rotation_direction)
    }

    /// Move toward the tile this unit is facing if able
    pub(super) fn move_forward(
        unit_tile_pos: TilePos,
        facing: &Facing,
        map_geometry: &MapGeometry,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
    ) -> Self {
        /// The time in seconds that it takes a standard unit to walk to an adjacent tile.
        const BASE_WALKING_DURATION: f32 = 0.5;

        let target_tile = unit_tile_pos.neighbor(facing.direction);
        let entity_standing_on = map_geometry.get_terrain(unit_tile_pos).unwrap();
        let terrain_standing_on = terrain_query.get(entity_standing_on).unwrap();
        let walking_speed = terrain_manifest.get(*terrain_standing_on).walking_speed;
        let walking_duration = BASE_WALKING_DURATION / walking_speed;

        if map_geometry.is_passable(target_tile) {
            CurrentAction {
                action: UnitAction::MoveForward,
                timer: Timer::from_seconds(walking_duration, TimerMode::Once),
                just_started: true,
            }
        } else {
            CurrentAction::idle()
        }
    }

    /// Attempt to move toward the `target_tile_pos`.
    pub(super) fn move_or_spin(
        unit_tile_pos: TilePos,
        target_tile_pos: TilePos,
        facing: &Facing,
        terrain_query: &Query<&Id<Terrain>>,
        terrain_manifest: &TerrainManifest,
        map_geometry: &MapGeometry,
    ) -> Self {
        let required_direction = unit_tile_pos.direction_to(target_tile_pos.hex);

        if required_direction == facing.direction {
            CurrentAction::move_forward(
                unit_tile_pos,
                facing,
                map_geometry,
                terrain_query,
                terrain_manifest,
            )
        } else {
            CurrentAction::spin_towards(facing, required_direction)
        }
    }

    /// Wait, as there is nothing to be done.
    pub(super) fn idle() -> Self {
        CurrentAction {
            action: UnitAction::Idle,
            timer: Timer::from_seconds(0.1, TimerMode::Once),
            just_started: true,
        }
    }

    /// Picks up the `item_id` at the `output_entity`.
    pub(super) fn pickup(
        item_id: Id<Item>,
        output_entity: Entity,
        facing: &Facing,
        unit_tile_pos: TilePos,
        output_tile_pos: TilePos,
    ) -> Self {
        let required_direction = unit_tile_pos.direction_to(output_tile_pos.hex);

        if required_direction == facing.direction {
            CurrentAction {
                action: UnitAction::PickUp {
                    item_id,
                    output_entity,
                },
                timer: Timer::from_seconds(0.5, TimerMode::Once),
                just_started: true,
            }
        } else {
            CurrentAction::spin_towards(facing, required_direction)
        }
    }

    /// Drops off the `item_id` at the `input_entity`.
    pub(super) fn dropoff(
        item_id: Id<Item>,
        input_entity: Entity,
        facing: &Facing,
        unit_tile_pos: TilePos,
        input_tile_pos: TilePos,
    ) -> Self {
        let required_direction = unit_tile_pos.direction_to(input_tile_pos.hex);

        if required_direction == facing.direction {
            CurrentAction {
                action: UnitAction::DropOff {
                    item_id,
                    input_entity,
                },
                timer: Timer::from_seconds(0.2, TimerMode::Once),
                just_started: true,
            }
        } else {
            CurrentAction::spin_towards(facing, required_direction)
        }
    }

    /// Eats one of the currently held item.
    pub(super) fn eat() -> Self {
        CurrentAction {
            action: UnitAction::Eat,
            timer: Timer::from_seconds(0.5, TimerMode::Once),
            just_started: true,
        }
    }

    /// Work at the specified structure
    pub(super) fn work(structure_entity: Entity) -> Self {
        CurrentAction {
            action: UnitAction::Work { structure_entity },
            timer: Timer::from_seconds(1.0, TimerMode::Once),
            just_started: true,
        }
    }

    /// Demolish the specified structure
    pub(super) fn demolish(structure_entity: Entity) -> Self {
        CurrentAction {
            action: UnitAction::Demolish { structure_entity },
            timer: Timer::from_seconds(1.0, TimerMode::Once),
            just_started: true,
        }
    }

    /// Eats one of the currently held item.
    pub(super) fn abandon() -> Self {
        CurrentAction {
            action: UnitAction::Abandon,
            timer: Timer::from_seconds(0.1, TimerMode::Once),
            just_started: true,
        }
    }
}
