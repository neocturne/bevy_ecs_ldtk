use crate::{
    app::LdtkEntityMap,
    assets::{LdtkAsset, LdtkExternalLevel},
    components::*,
    ldtk::{EntityDefinition, LayerInstance, TileInstance, TilesetDefinition, Type},
};

use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use std::collections::HashMap;

const CHUNK_SIZE: ChunkSize = ChunkSize(32, 32);

pub fn process_external_levels(
    mut level_events: EventReader<AssetEvent<LdtkExternalLevel>>,
    level_assets: Res<Assets<LdtkExternalLevel>>,
    mut ldtk_assets: ResMut<Assets<LdtkAsset>>,
) {
    for event in level_events.iter() {
        // creation and deletion events should be handled by the ldtk asset events
        let mut changed_levels = Vec::<Handle<LdtkExternalLevel>>::new();
        match event {
            AssetEvent::Created { handle } => {
                info!("External Level added!");
                changed_levels.push(handle.clone());
            }
            AssetEvent::Modified { handle } => {
                info!("External Level changed!");
                changed_levels.push(handle.clone());
            }
            _ => (),
        }

        let mut levels_to_update = Vec::new();
        for level_handle in changed_levels {
            for (ldtk_handle, ldtk_asset) in ldtk_assets.iter() {
                for (i, _) in ldtk_asset
                    .external_levels
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| **h == level_handle)
                {
                    levels_to_update.push((ldtk_handle, level_handle.clone(), i));
                }
            }
        }

        for (ldtk_handle, level_handle, level_index) in levels_to_update {
            if let Some(level) = level_assets.get(level_handle) {
                if let Some(ldtk_asset) = ldtk_assets.get_mut(ldtk_handle) {
                    if let Some(ldtk_level) = ldtk_asset.project.levels.get_mut(level_index) {
                        *ldtk_level = level.level.clone();
                    }
                }
            }
        }
    }
}

pub fn process_loaded_ldtk(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut ldtk_events: EventReader<AssetEvent<LdtkAsset>>,
    mut ldtk_map_query: Query<(
        Entity,
        &Handle<LdtkAsset>,
        &LevelSelection,
        &mut Map,
        Option<&Children>,
    )>,
    ldtk_assets: Res<Assets<LdtkAsset>>,
    layer_query: Query<&Layer>,
    chunk_query: Query<&Chunk>,
    new_ldtks: Query<&Handle<LdtkAsset>, Added<Handle<LdtkAsset>>>,
    asset_server: Res<AssetServer>,
    bundle_map: NonSend<LdtkEntityMap>,
) {
    // This function uses code from the bevy_ecs_tilemap ldtk example
    // https://github.com/StarArawn/bevy_ecs_tilemap/blob/main/examples/ldtk/ldtk.rs
    let mut changed_ldtks = Vec::<Handle<LdtkAsset>>::new();
    for event in ldtk_events.iter() {
        match event {
            AssetEvent::Created { handle } => {
                info!("Ldtk added!");
                changed_ldtks.push(handle.clone());
            }
            AssetEvent::Modified { handle } => {
                info!("Ldtk changed!");
                changed_ldtks.push(handle.clone());
            }
            AssetEvent::Removed { handle } => {
                info!("Ldtk removed!");
                // if mesh was modified and removed in the same update, ignore the modification
                // events are ordered so future modification events are ok
                changed_ldtks = changed_ldtks
                    .into_iter()
                    .filter(|changed_handle| changed_handle == handle)
                    .collect();
            }
        }
    }

    for new_ldtk_handle in new_ldtks.iter() {
        changed_ldtks.push(new_ldtk_handle.clone());
    }

    for changed_ldtk in changed_ldtks.iter() {
        for (ldtk_entity, ldtk_handle, level_selection, mut map, children) in ldtk_map_query
            .iter_mut()
            .filter(|(_, l, _, _, _)| changed_ldtk == *l)
        {
            if let Some(ldtk_asset) = ldtk_assets.get(ldtk_handle) {
                for (layer_id, layer_entity) in map.get_layers() {
                    if let Ok(layer) = layer_query.get(layer_entity) {
                        for x in 0..layer.get_layer_size_in_tiles().0 {
                            for y in 0..layer.get_layer_size_in_tiles().1 {
                                let tile_pos = TilePos(x, y);
                                let chunk_pos = ChunkPos(
                                    tile_pos.0 / layer.settings.chunk_size.0,
                                    tile_pos.1 / layer.settings.chunk_size.1,
                                );
                                if let Some(chunk_entity) = layer.get_chunk(chunk_pos) {
                                    if let Ok(chunk) = chunk_query.get(chunk_entity) {
                                        let chunk_tile_pos = chunk.to_chunk_pos(tile_pos);
                                        if let Some(tile) = chunk.get_tile_entity(chunk_tile_pos) {
                                            commands.entity(tile).despawn_recursive();
                                        }
                                    }

                                    commands.entity(chunk_entity).despawn_recursive();
                                }
                            }
                        }

                        map.remove_layer(&mut commands, layer_id);
                    }
                }
                if let Some(children) = children {
                    for child in children.iter() {
                        commands.entity(*child).despawn_recursive();
                    }
                }

                let tileset_definition_map: HashMap<i32, &TilesetDefinition> = ldtk_asset
                    .project
                    .defs
                    .tilesets
                    .iter()
                    .map(|t| (t.uid, t))
                    .collect();

                let entity_definition_map: HashMap<i32, &EntityDefinition> = ldtk_asset
                    .project
                    .defs
                    .entities
                    .iter()
                    .map(|e| (e.uid, e))
                    .collect();

                for (_, level) in ldtk_asset
                    .project
                    .levels
                    .iter()
                    .enumerate()
                    .filter(|(i, l)| match level_selection {
                        LevelSelection::Identifier(s) => *s == l.identifier,
                        LevelSelection::Index(j) => j == i,
                        LevelSelection::Uid(u) => *u == l.uid,
                    })
                {
                    if let Some(layer_instances) = &level.layer_instances {
                        for (layer_z, layer_instance) in
                            layer_instances.into_iter().rev().enumerate()
                        {
                            match layer_instance.layer_instance_type {
                                Type::Entities => {
                                    for entity_instance in &layer_instance.entity_instances {
                                        let transform = calculate_transform_from_entity_instance(
                                            entity_instance,
                                            &entity_definition_map,
                                            level.px_hei,
                                            layer_z,
                                        );

                                        let mut entity_commands = match bundle_map
                                            .get(&entity_instance.identifier)
                                        {
                                            None => commands.spawn_bundle(EntityInstanceBundle {
                                                entity_instance: entity_instance.clone(),
                                            }),
                                            Some(phantom_ldtk_entity) => phantom_ldtk_entity
                                                .evaluate(
                                                    &mut commands,
                                                    &entity_instance,
                                                    &ldtk_asset.tileset_map,
                                                    &asset_server,
                                                    &mut materials,
                                                    &mut texture_atlases,
                                                ),
                                        };

                                        entity_commands
                                            .insert(transform)
                                            .insert(GlobalTransform::default())
                                            .insert(Parent(ldtk_entity));
                                    }
                                }
                                _ => {
                                    let map_size = MapSize(
                                        (layer_instance.c_wid as f32 / CHUNK_SIZE.0 as f32).ceil()
                                            as u32,
                                        (layer_instance.c_hei as f32 / CHUNK_SIZE.1 as f32).ceil()
                                            as u32,
                                    );

                                    let layer_entity = match layer_instance.tileset_def_uid {
                                        Some(tileset_uid) => {
                                            let tileset_definition =
                                                tileset_definition_map.get(&tileset_uid).unwrap();
                                            let settings = LayerSettings::new(
                                                map_size,
                                                CHUNK_SIZE,
                                                TileSize(
                                                    tileset_definition.tile_grid_size as f32,
                                                    tileset_definition.tile_grid_size as f32,
                                                ),
                                                TextureSize(
                                                    tileset_definition.px_wid as f32,
                                                    tileset_definition.px_hei as f32,
                                                ),
                                            );

                                            let texture_handle = ldtk_asset
                                                .tileset_map
                                                .get(&tileset_definition.uid)
                                                .unwrap();

                                            let material_handle = materials.add(
                                                ColorMaterial::texture(texture_handle.clone_weak()),
                                            );

                                            let mut grid_tiles = layer_instance.grid_tiles.clone();
                                            grid_tiles
                                                .extend(layer_instance.auto_layer_tiles.clone());
                                            let tile_maker = tile_pos_to_tile_maker(
                                                layer_instance.c_hei,
                                                (*tileset_definition).clone(),
                                                grid_tiles,
                                            );

                                            LayerBuilder::<TileBundle>::new_batch(
                                                &mut commands,
                                                settings,
                                                &mut meshes,
                                                material_handle,
                                                map.id,
                                                layer_z as u16,
                                                None,
                                                tile_pos_to_tile_bundle_maker(tile_maker),
                                            )
                                        }
                                        _ => {
                                            let settings = LayerSettings::new(
                                                map_size,
                                                CHUNK_SIZE,
                                                TileSize(
                                                    layer_instance.grid_size as f32,
                                                    layer_instance.grid_size as f32,
                                                ),
                                                TextureSize(0., 0.),
                                            );

                                            let material_handle =
                                                materials.add(ColorMaterial::default());

                                            let (mut layer_builder, layer_entity) =
                                                LayerBuilder::<TileBundle>::new(
                                                    &mut commands,
                                                    settings,
                                                    map.id,
                                                    layer_z as u16,
                                                    None,
                                                );

                                            for (i, value) in layer_instance
                                                .int_grid_csv
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, v)| **v != 0)
                                            {
                                                let tile_x = i as u32 % layer_instance.c_wid as u32;
                                                let tile_y = layer_instance.c_hei as u32
                                                    - ((i as u32 - tile_x)
                                                        / layer_instance.c_wid as u32)
                                                    - 1;

                                                let tile_pos = TilePos(tile_x, tile_y);

                                                layer_builder
                                                    .set_tile(
                                                        tile_pos,
                                                        TileBundle {
                                                            tile: Tile {
                                                                visible: false,
                                                                ..Default::default()
                                                            },
                                                            ..Default::default()
                                                        },
                                                    )
                                                    .unwrap();

                                                let tile_entity = layer_builder
                                                    .get_tile_entity(&mut commands, tile_pos)
                                                    .unwrap();

                                                commands.entity(tile_entity).insert_bundle(
                                                    IntGridCellBundle {
                                                        int_grid_cell: IntGridCell {
                                                            value: *value,
                                                        },
                                                    },
                                                );
                                            }

                                            layer_builder.build(
                                                &mut commands,
                                                &mut meshes,
                                                material_handle,
                                            );

                                            layer_entity
                                        }
                                    };

                                    map.add_layer(&mut commands, layer_z as u16, layer_entity);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn invisible_tile(_: TilePos) -> Option<Tile> {
    Some(Tile {
        visible: false,
        ..Default::default()
    })
}

fn tile_pos_to_tile_maker(
    layer_height_in_tiles: i32,
    tileset_definition: TilesetDefinition,
    grid_tiles: Vec<TileInstance>,
) -> impl FnMut(TilePos) -> Option<Tile> {
    let grid_tile_map: HashMap<TilePos, TileInstance> = grid_tiles
        .into_iter()
        .map(|t| {
            (
                TilePos(
                    (t.px[0] / tileset_definition.tile_grid_size) as u32,
                    layer_height_in_tiles as u32
                        - (t.px[1] / tileset_definition.tile_grid_size) as u32
                        - 1,
                ),
                t,
            )
        })
        .collect();

    move |tile_pos: TilePos| -> Option<Tile> {
        match grid_tile_map.get(&tile_pos) {
            Some(tile_instance) => {
                let tileset_x = tile_instance.src[0] / tileset_definition.tile_grid_size;
                let tileset_y = tile_instance.src[1] / tileset_definition.tile_grid_size;
                let (flip_x, flip_y) = match tile_instance.f {
                    1 => (true, false),
                    2 => (false, true),
                    3 => (true, true),
                    _ => (false, false),
                };
                Some(Tile {
                    texture_index: (tileset_y * tileset_definition.c_wid + tileset_x) as u16,
                    flip_x,
                    flip_y,
                    ..Default::default()
                })
            }
            None => None,
        }
    }
}

fn tile_pos_to_tile_bundle_maker(
    mut tile_maker: impl FnMut(TilePos) -> Option<Tile>,
) -> impl FnMut(TilePos) -> Option<TileBundle> {
    move |tile_pos: TilePos| -> Option<TileBundle> {
        match tile_maker(tile_pos) {
            Some(tile) => Some(TileBundle {
                tile,
                ..Default::default()
            }),
            None => None,
        }
    }
}

fn tile_pos_to_int_grid_cell_bundle(
    tile_pos: TilePos,
    int_grid_csv: &Vec<i32>,
    layer_height_in_tiles: i32,
    layer_width_in_tiles: i32,
) -> Option<IntGridCellBundle> {
    let ldtk_x = tile_pos.0 as i32;
    let ldtk_y = layer_height_in_tiles - tile_pos.1 as i32 - 1;

    if ldtk_y < 0 || ldtk_y >= layer_height_in_tiles || ldtk_x < 0 || ldtk_x >= layer_width_in_tiles
    {
        return None;
    }

    let csv_index = (ldtk_y * layer_width_in_tiles + ldtk_x) as usize;

    match int_grid_csv.get(csv_index) {
        Some(x) if *x != 0 => Some(IntGridCellBundle {
            int_grid_cell: IntGridCell { value: *x },
        }),
        _ => None,
    }
}

fn calculate_transform_from_ldtk_info(
    location: IVec2,
    pivot: Vec2,
    def_size: IVec2,
    size: IVec2,
    level_height: i32,
    layer_z: usize,
) -> Transform {
    let pivot_point = Vec2::new(location.x as f32, (level_height - location.y) as f32);

    let adjusted_pivot = Vec2::new(0.5 - pivot.x, pivot.y - 0.5);

    let offset = size.as_vec2() * adjusted_pivot;

    let translation = pivot_point + offset;

    let scale = size.as_vec2() / def_size.as_vec2();

    Transform::from_xyz(translation.x, translation.y, layer_z as f32)
        .with_scale(Vec3::new(scale.x, scale.y, 1.))
}

fn calculate_transform_from_entity_instance(
    entity_instance: &EntityInstance,
    entity_definition_map: &HashMap<i32, &EntityDefinition>,
    level_height: i32,
    layer_z: usize,
) -> Transform {
    let entity_definition = entity_definition_map.get(&entity_instance.def_uid).unwrap();

    let location = IVec2::from_slice(entity_instance.px.as_slice());

    let pivot = Vec2::from_slice(entity_instance.pivot.as_slice());

    let def_size = IVec2::new(entity_definition.width, entity_definition.height);

    let size = IVec2::new(entity_instance.width, entity_instance.height);

    calculate_transform_from_ldtk_info(location, pivot, def_size, size, level_height, layer_z)
}
