use crate::types::{RelationshipTuple, TupleKey};
use std::sync::Mutex;

/// Events that can be triggered by engine operations.
#[derive(Debug, Clone)]
pub enum HookEvent {
    OnWrite {
        tuple: RelationshipTuple,
    },
    OnDelete {
        key: TupleKey,
    },
    OnCheck {
        subject: String,
        permission: String,
        resource: String,
        allowed: bool,
    },
}

/// A hook function that receives a reference to a `HookEvent`.
pub type HookFn = Box<dyn Fn(&HookEvent) + Send + Sync>;

/// Registry of hook callbacks that are invoked when engine events occur.
pub struct HookRegistry {
    hooks: Vec<HookFn>,
}

impl HookRegistry {
    /// Create an empty hook registry.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a new hook callback.
    pub fn register(&mut self, hook: HookFn) {
        self.hooks.push(hook);
    }

    /// Trigger all registered hooks with the given event.
    pub fn trigger(&self, event: &HookEvent) {
        for hook in &self.hooks {
            hook(event);
        }
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A thread-safe wrapper around `HookRegistry` that can be shared across threads.
pub struct SharedHookRegistry {
    inner: Mutex<HookRegistry>,
}

impl SharedHookRegistry {
    /// Create a new shared hook registry.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HookRegistry::new()),
        }
    }

    /// Register a new hook callback.
    pub fn register(&self, hook: HookFn) {
        self.inner.lock().unwrap().register(hook);
    }

    /// Trigger all registered hooks with the given event.
    pub fn trigger(&self, event: &HookEvent) {
        let registry = self.inner.lock().unwrap();
        registry.trigger(event);
    }
}

impl Default for SharedHookRegistry {
    fn default() -> Self {
        Self::new()
    }
}
