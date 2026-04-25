//! World query helpers. Mirrors src/agent/library/world.js.

use std::sync::Arc;
use anyhow::Result;

use crate::bot::BotHandle;

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub name: String,
    pub position: [i32; 3],
}

#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub name: String,
    pub entity_type: String,
    pub position: [f64; 3],
    pub distance: f64,
}

/// Find nearest block of `block_name` within `max_distance`.
pub async fn get_nearest_block(
    bot: &BotHandle,
    block_name: &str,
    max_distance: f64,
) -> Option<BlockInfo> {
    // azalea: scan chunk data around player
    None
}

/// List all blocks within `distance` blocks.
pub async fn get_nearby_blocks(bot: &BotHandle, distance: f64) -> Vec<BlockInfo> {
    // azalea: iterate nearby chunks
    vec![]
}

/// List nearby entities.
pub async fn get_nearby_entities(bot: &BotHandle, distance: f64) -> Vec<EntityInfo> {
    // azalea: bot.entity_component::<EntityKind>() etc.
    vec![]
}

/// Get current bot position as (x, y, z).
pub async fn get_position(bot: &BotHandle) -> [f64; 3] {
    bot.position().await
}

/// Check if player with `name` is reachable.
pub async fn get_player_by_name(bot: &BotHandle, name: &str) -> Option<EntityInfo> {
    None
}

/// Nearest free surface position at given x, z (find top y).
pub async fn get_surface_height(bot: &BotHandle, x: i32, z: i32) -> i32 {
    64 // stub
}
