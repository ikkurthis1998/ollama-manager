use metrics::{register_counter, register_gauge, Counter, Gauge};

pub struct Metrics {
    requests_total: Counter,
    active_connections: Gauge,
    healthy_endpoints: Gauge,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            requests_total: register_counter!("lb_requests_total"),
            active_connections: register_gauge!("lb_active_connections"),
            healthy_endpoints: register_gauge!("lb_healthy_endpoints"),
        }
    }

    pub fn increment_requests(&self) {
        self.requests_total.increment(1);
    }

    pub fn set_active_connections(&self, count: u64) {
        self.active_connections.set(count as f64);
    }

    pub fn set_healthy_endpoints(&self, count: u64) {
        self.healthy_endpoints.set(count as f64);
    }
}
