//! High-level skill library: go_to, collect_block, craft, place_block, etc.
//! Mirrors src/agent/library/skills.js.
//!
//! Each function takes Arc<BotHandle> and returns Result<()>.
//! Skills write progress to bot.output_buffer and check bot.interrupt_code.

use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use crate::bot::BotHandle;

/// Move bot to (x, y, z) within `tolerance` blocks.
pub async fn go_to_position(bot: Arc<BotHandle>, x: f64, y: f64, z: f64, tolerance: f64) -> Result<bool> {
    info!("go_to_position({x}, {y}, {z}, tol={tolerance})");
    // azalea: bot.pathfind(BlockPos::new(x as i32, y as i32, z as i32))
    bot.output_buffer.write().await.push_str(&format!(
        "Going to position ({x:.1}, {y:.1}, {z:.1})\n"
    ));
    // TODO: real pathfinding via azalea pathfinder plugin
    Ok(true)
}

/// Mine the nearest block of `block_name` within `distance` blocks.
pub async fn collect_block(bot: Arc<BotHandle>, block_name: &str, num: u32, distance: f64) -> Result<()> {
    info!("collect_block({block_name}, {num}, dist={distance})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Collecting {num} {block_name}\n"));
    // TODO: azalea collectblock equivalent
    Ok(())
}

/// Place a block from inventory at world position.
pub async fn place_block(bot: Arc<BotHandle>, item_name: &str, x: i32, y: i32, z: i32) -> Result<bool> {
    info!("place_block({item_name}, {x}, {y}, {z})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Placing {item_name} at ({x}, {y}, {z})\n"));
    // TODO: azalea place block
    Ok(true)
}

/// Craft `item_name` x `quantity` using the crafting table.
pub async fn craft_item(bot: Arc<BotHandle>, item_name: &str, quantity: u32) -> Result<bool> {
    info!("craft_item({item_name}, {quantity})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Crafting {quantity} {item_name}\n"));
    // TODO: azalea crafting
    Ok(true)
}

/// Smelt item in furnace.
pub async fn smelt_item(bot: Arc<BotHandle>, item_name: &str, quantity: u32) -> Result<bool> {
    info!("smelt_item({item_name}, {quantity})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Smelting {quantity} {item_name}\n"));
    Ok(true)
}

/// Move away from current position by `distance` blocks.
pub async fn move_away(bot: Arc<BotHandle>, distance: f64) -> Result<bool> {
    info!("move_away({distance})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Moving {distance} blocks away\n"));
    Ok(true)
}

/// Attack the nearest entity of `entity_type`.
pub async fn attack_nearest(bot: Arc<BotHandle>, entity_type: &str, allow_sprint: bool) -> Result<bool> {
    info!("attack_nearest({entity_type})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Attacking nearest {entity_type}\n"));
    Ok(true)
}

/// Equip item in hand.
pub async fn equip_item(bot: Arc<BotHandle>, item_name: &str) -> Result<bool> {
    info!("equip_item({item_name})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Equipping {item_name}\n"));
    Ok(true)
}

/// Eat food item.
pub async fn eat(bot: Arc<BotHandle>, food_name: &str) -> Result<bool> {
    info!("eat({food_name})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Eating {food_name}\n"));
    Ok(true)
}

/// Discard items from inventory.
pub async fn discard_item(bot: Arc<BotHandle>, item_name: &str, quantity: u32) -> Result<bool> {
    info!("discard_item({item_name}, {quantity})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Discarding {quantity} {item_name}\n"));
    Ok(true)
}

/// Interact with (right-click) the nearest block/entity of a given type.
pub async fn activate_nearest_block(bot: Arc<BotHandle>, block_type: &str) -> Result<bool> {
    info!("activate_nearest_block({block_type})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Activating nearest {block_type}\n"));
    Ok(true)
}

/// Put item into chest.
pub async fn put_in_chest(bot: Arc<BotHandle>, item_name: &str, quantity: u32) -> Result<bool> {
    info!("put_in_chest({item_name}, {quantity})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Putting {quantity} {item_name} in chest\n"));
    Ok(true)
}

/// Take item from chest.
pub async fn take_from_chest(bot: Arc<BotHandle>, item_name: &str, quantity: u32) -> Result<bool> {
    info!("take_from_chest({item_name}, {quantity})");
    let mut out = bot.output_buffer.write().await;
    out.push_str(&format!("Taking {quantity} {item_name} from chest\n"));
    Ok(true)
}

/// Sleep in a nearby bed.
pub async fn sleep_in_bed(bot: Arc<BotHandle>) -> Result<bool> {
    info!("sleep_in_bed");
    let mut out = bot.output_buffer.write().await;
    out.push_str("Sleeping in bed\n");
    Ok(true)
}

/// Utility: check interrupt and short-circuit if requested.
pub async fn check_interrupt(bot: &BotHandle) -> bool {
    *bot.interrupt_code.read().await
}
