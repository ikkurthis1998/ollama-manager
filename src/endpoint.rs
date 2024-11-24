use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct Endpoint {
    pub url: String,
    pub weight: u32,
    pub max_connections: u32,
    healthy: Arc<AtomicBool>,
    current_connections: Arc<AtomicU32>,
}

impl Endpoint {
    pub fn new(url: String, weight: u32, max_connections: u32) -> Self {
        Self {
            url,
            weight,
            max_connections,
            healthy: Arc::new(AtomicBool::new(true)),
            current_connections: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub fn mark_healthy(&self) {
        self.healthy.store(true, Ordering::Relaxed);
    }

    pub fn mark_unhealthy(&self) {
        self.healthy.store(false, Ordering::Relaxed);
    }

    pub fn increment_connections(&self) -> bool {
        let current = self.current_connections.fetch_add(1, Ordering::SeqCst);
        if current >= self.max_connections {
            self.current_connections.fetch_sub(1, Ordering::SeqCst);
            false
        } else {
            true
        }
    }

    pub fn decrement_connections(&self) {
        self.current_connections.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn get_connections(&self) -> u32 {
        self.current_connections.load(Ordering::Relaxed)
    }
}
