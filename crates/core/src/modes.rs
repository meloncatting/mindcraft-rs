//! Reactive behavior modes (self_preservation, unstuck, cowardice, etc.).
//! Mirrors src/agent/modes.js.
//!
//! Each mode runs every tick (ModeManager::update) and can trigger actions
//! without blocking the update loop.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::info;

/// Result of a mode tick
pub enum ModeAction {
    None,
    Execute { label: String, priority: u8 },
}

/// Trait for a reactive mode.
#[async_trait::async_trait]
pub trait Mode: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn is_on(&self) -> bool;
    fn set_on(&mut self, val: bool);
    fn is_active(&self) -> bool;

    /// Called every ~300ms. Return ModeAction::Execute to trigger an action.
    async fn update(&mut self, ctx: &dyn ModeContext) -> ModeAction;
    async fn on_pause(&mut self) {}
    async fn on_resume(&mut self) {}
}

/// Minimal context provided to modes during update tick.
#[async_trait::async_trait]
pub trait ModeContext: Send + Sync {
    fn agent_name(&self) -> &str;
    async fn bot_health(&self) -> f32;
    async fn bot_position(&self) -> [f64; 3];
    async fn block_at(&self, pos: [f64; 3]) -> Option<String>;
    async fn last_damage_time_ms(&self) -> u64;
    async fn last_damage_taken(&self) -> f32;
    async fn is_in_water(&self) -> bool;
    async fn is_in_fire(&self) -> bool;
    fn behavior_log(&self) -> &str;
    fn append_behavior_log(&self, msg: &str);
    fn should_narrate(&self) -> bool;
    async fn open_chat(&self, msg: &str);
}

pub struct ModeManager {
    modes: Mutex<HashMap<String, Box<dyn Mode>>>,
    behavior_log: Mutex<String>,
    paused: Mutex<Vec<String>>,
}

impl ModeManager {
    pub fn new() -> Arc<Self> {
        let mut modes: HashMap<String, Box<dyn Mode>> = HashMap::new();
        modes.insert("self_preservation".into(), Box::new(SelfPreservationMode::default()));
        modes.insert("unstuck".into(), Box::new(UnstuckMode::default()));
        modes.insert("cowardice".into(), Box::new(CowardiceMode::default()));
        modes.insert("self_defense".into(), Box::new(SelfDefenseMode::default()));
        modes.insert("hunting".into(), Box::new(HuntingMode::default()));
        modes.insert("item_collecting".into(), Box::new(ItemCollectingMode::default()));
        modes.insert("torch_placing".into(), Box::new(TorchPlacingMode::default()));
        modes.insert("mob_avoidance".into(), Box::new(MobAvoidanceMode::default()));

        Arc::new(Self {
            modes: Mutex::new(modes),
            behavior_log: Mutex::new(String::new()),
            paused: Mutex::new(Vec::new()),
        })
    }

    pub async fn set(&self, name: &str, enabled: bool) {
        let mut modes = self.modes.lock().await;
        if let Some(m) = modes.get_mut(name) {
            m.set_on(enabled);
        }
    }

    pub async fn get(&self, name: &str) -> bool {
        self.modes.lock().await.get(name).map(|m| m.is_on()).unwrap_or(false)
    }

    pub async fn pause(&self, name: &str) {
        self.paused.lock().await.push(name.to_string());
    }

    pub async fn un_pause_all(&self) {
        self.paused.lock().await.clear();
    }

    pub async fn flush_behavior_log(&self) -> String {
        let mut log = self.behavior_log.lock().await;
        std::mem::take(&mut *log)
    }

    pub async fn on_idle(&self, agent_name: &str) {
        info!("{agent_name} is idle, modes notified");
    }

    /// Called every ~300ms from the update loop.
    pub async fn update(&self, ctx: &dyn ModeContext) {
        let paused = self.paused.lock().await.clone();
        let mut modes = self.modes.lock().await;

        for (name, mode) in modes.iter_mut() {
            if !mode.is_on() { continue; }
            if paused.contains(name) { continue; }
            match mode.update(ctx).await {
                ModeAction::None => {}
                ModeAction::Execute { label, .. } => {
                    // Signal the action manager via context
                    // (actual interruption handled by the agent)
                }
            }
        }
    }
}

// ── Concrete mode implementations ──────────────────────────────────────────

macro_rules! simple_mode {
    ($name:ident, $str_name:literal, $desc:literal, $default_on:literal) => {
        #[derive(Debug, Default)]
        pub struct $name {
            on: bool,
            active: bool,
        }
        impl $name {
            pub fn new() -> Self {
                Self { on: $default_on, active: false }
            }
        }
        #[async_trait::async_trait]
        impl Mode for $name {
            fn name(&self) -> &str { $str_name }
            fn description(&self) -> &str { $desc }
            fn is_on(&self) -> bool { self.on }
            fn set_on(&mut self, v: bool) { self.on = v; }
            fn is_active(&self) -> bool { self.active }
            async fn update(&mut self, _ctx: &dyn ModeContext) -> ModeAction { ModeAction::None }
        }
    };
}

simple_mode!(CowardiceMode, "cowardice", "Run away from mobs that can attack you.", false);
simple_mode!(SelfDefenseMode, "self_defense", "Attack back if hit.", true);
simple_mode!(HuntingMode, "hunting", "Kill nearby animals for food.", false);
simple_mode!(ItemCollectingMode, "item_collecting", "Collect dropped items.", true);
simple_mode!(TorchPlacingMode, "torch_placing", "Place torches in dark areas.", false);
simple_mode!(MobAvoidanceMode, "mob_avoidance", "Avoid hostile mobs.", false);

#[derive(Debug)]
pub struct SelfPreservationMode {
    on: bool,
    active: bool,
}
impl Default for SelfPreservationMode {
    fn default() -> Self { Self { on: true, active: false } }
}
#[async_trait::async_trait]
impl Mode for SelfPreservationMode {
    fn name(&self) -> &str { "self_preservation" }
    fn description(&self) -> &str {
        "Respond to drowning, burning, and damage at low health. Interrupts all actions."
    }
    fn is_on(&self) -> bool { self.on }
    fn set_on(&mut self, v: bool) { self.on = v; }
    fn is_active(&self) -> bool { self.active }

    async fn update(&mut self, ctx: &dyn ModeContext) -> ModeAction {
        let health = ctx.bot_health().await;
        let in_fire = ctx.is_in_fire().await;
        let last_dmg_ms = ctx.last_damage_time_ms().await;
        let last_dmg = ctx.last_damage_taken().await;

        if in_fire {
            ctx.append_behavior_log("I'm on fire!");
            if ctx.should_narrate() { ctx.open_chat("I'm on fire!").await; }
            return ModeAction::Execute { label: "escape_fire".into(), priority: 10 };
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if now.saturating_sub(last_dmg_ms) < 3000 && (health < 5.0 || last_dmg >= health) {
            ctx.append_behavior_log("I'm dying!");
            if ctx.should_narrate() { ctx.open_chat("I'm dying!").await; }
            return ModeAction::Execute { label: "flee".into(), priority: 10 };
        }

        ModeAction::None
    }
}

#[derive(Debug)]
pub struct UnstuckMode {
    on: bool,
    active: bool,
    prev_pos: Option<[f64; 3]>,
    stuck_time_ms: u64,
    max_stuck_ms: u64,
}
impl Default for UnstuckMode {
    fn default() -> Self {
        Self { on: true, active: false, prev_pos: None, stuck_time_ms: 0, max_stuck_ms: 20_000 }
    }
}
#[async_trait::async_trait]
impl Mode for UnstuckMode {
    fn name(&self) -> &str { "unstuck" }
    fn description(&self) -> &str { "Attempt to get unstuck when in the same place for a while." }
    fn is_on(&self) -> bool { self.on }
    fn set_on(&mut self, v: bool) { self.on = v; }
    fn is_active(&self) -> bool { self.active }

    async fn update(&mut self, ctx: &dyn ModeContext) -> ModeAction {
        let pos = ctx.bot_position().await;
        const THRESHOLD: f64 = 2.0;

        if let Some(prev) = self.prev_pos {
            let dist = ((pos[0]-prev[0]).powi(2) + (pos[2]-prev[2]).powi(2)).sqrt();
            if dist < THRESHOLD {
                self.stuck_time_ms += 300; // approximate tick interval
            } else {
                self.stuck_time_ms = 0;
            }
        }
        self.prev_pos = Some(pos);

        if self.stuck_time_ms >= self.max_stuck_ms {
            self.stuck_time_ms = 0;
            ctx.append_behavior_log("I'm stuck, attempting to get unstuck.");
            return ModeAction::Execute { label: "unstuck".into(), priority: 5 };
        }

        ModeAction::None
    }
}
