//! Action execution manager with timeout, interrupt, and resume.
//! Mirrors src/agent/action_manager.js.

use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub success: bool,
    pub message: Option<String>,
    pub interrupted: bool,
    pub timedout: bool,
}

struct ActionState {
    executing: bool,
    label: String,
    timedout: bool,
    output: String,
    interrupt: bool,
}

pub struct ActionManager {
    state: Mutex<ActionState>,
    done_notify: Notify,
    resume_func: Mutex<Option<ResumeEntry>>,
    last_action_time: Mutex<Instant>,
    recent_counter: Mutex<u32>,
}

struct ResumeEntry {
    label: String,
    // We store the action as a boxed async closure factory.
    // Actual execution goes through execute_action.
    func: Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync>,
}

impl ActionManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ActionState {
                executing: false,
                label: String::new(),
                timedout: false,
                output: String::new(),
                interrupt: false,
            }),
            done_notify: Notify::new(),
            resume_func: Mutex::new(None),
            last_action_time: Mutex::new(Instant::now() - Duration::from_secs(9999)),
            recent_counter: Mutex::new(0),
        })
    }

    pub async fn is_executing(&self) -> bool {
        self.state.lock().await.executing
    }

    pub async fn current_label(&self) -> String {
        self.state.lock().await.label.clone()
    }

    /// Append to the bot output buffer (called by skills during execution).
    pub async fn append_output(&self, text: &str) {
        let mut s = self.state.lock().await;
        s.output.push_str(text);
        s.output.push('\n');
    }

    pub async fn request_interrupt(&self) {
        self.state.lock().await.interrupt = true;
    }

    pub async fn clear_logs(&self) {
        let mut s = self.state.lock().await;
        s.output.clear();
        s.interrupt = false;
    }

    pub async fn is_interrupted(&self) -> bool {
        self.state.lock().await.interrupt
    }

    pub async fn cancel_resume(&self) {
        *self.resume_func.lock().await = None;
    }

    /// Wait until current action finishes (with 10-second kill timeout).
    pub async fn stop(&self) {
        if !self.state.lock().await.executing { return; }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.request_interrupt().await;
            if !self.state.lock().await.executing { break; }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                error!("Action refused stop after 10s — process should be killed");
                return;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    /// Execute an action function with optional timeout.
    pub async fn run_action<F, Fut>(
        &self,
        label: &str,
        func: F,
        timeout_mins: f64,
    ) -> ActionResult
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        // Rapid-fire detection
        {
            let mut counter = self.recent_counter.lock().await;
            let mut last = self.last_action_time.lock().await;
            let diff = last.elapsed().as_millis();
            if diff < 20 {
                *counter += 1;
            } else {
                *counter = 0;
            }
            *last = Instant::now();
            if *counter > 3 {
                warn!("Fast action loop detected, cancelling resume.");
                self.cancel_resume().await;
            }
            if *counter > 5 {
                error!("Infinite action loop detected.");
                return ActionResult {
                    success: false,
                    message: Some("Infinite action loop detected.".into()),
                    interrupted: false,
                    timedout: false,
                };
            }
        }

        // Stop any running action first
        self.stop().await;
        self.clear_logs().await;

        {
            let mut s = self.state.lock().await;
            s.executing = true;
            s.label = label.to_string();
            s.timedout = false;
        }

        info!("Executing action: {label}");

        let fut = func();
        let timeout_dur = if timeout_mins > 0.0 {
            Some(Duration::from_secs_f64(timeout_mins * 60.0))
        } else {
            None
        };

        let exec_result = if let Some(dur) = timeout_dur {
            match tokio::time::timeout(dur, fut).await {
                Ok(r) => r,
                Err(_) => {
                    self.state.lock().await.timedout = true;
                    warn!("Action {label} timed out after {timeout_mins:.1} min");
                    Err(anyhow::anyhow!("Action timed out"))
                }
            }
        } else {
            fut.await
        };

        let (output, interrupted, timedout) = {
            let mut s = self.state.lock().await;
            s.executing = false;
            s.label.clear();
            let out = std::mem::take(&mut s.output);
            let int = s.interrupt;
            let to = s.timedout;
            (out, int, to)
        };
        self.done_notify.notify_waiters();

        let (success, mut message) = match exec_result {
            Ok(_) => (true, format_output(&output)),
            Err(e) => {
                self.cancel_resume().await;
                let msg = format!(
                    "{}!!Code threw exception!!\nError: {e}\n",
                    format_output(&output)
                );
                (false, msg)
            }
        };

        // If interrupted (not timed out), suppress output
        if interrupted && !timedout {
            message = String::new();
        }

        ActionResult {
            success,
            message: if message.is_empty() { None } else { Some(message) },
            interrupted,
            timedout,
        }
    }

    pub async fn set_resume<F, Fut>(&self, label: String, func: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        *self.resume_func.lock().await = Some(ResumeEntry {
            label,
            func: Arc::new(move || Box::pin(func())),
        });
    }
}

fn format_output(raw: &str) -> String {
    const MAX: usize = 500;
    if raw.is_empty() { return String::new(); }
    if raw.len() > MAX {
        format!(
            "Action output is very long ({} chars) and has been shortened.\nFirst outputs:\n{}\n...skipping...\nFinal outputs:\n{}",
            raw.len(),
            &raw[..MAX / 2],
            &raw[raw.len() - MAX / 2..]
        )
    } else {
        format!("Action output:\n{raw}")
    }
}
