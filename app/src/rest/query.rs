#[derive(serde::Serialize, serde::Deserialize)]
pub struct DateQuery {
    from: chrono::DateTime<chrono::Utc>,
    until: chrono::DateTime<chrono::Utc>,
}

impl DateQuery {
    pub fn from(&self) -> chrono::DateTime<chrono::Utc> {
        self.from
    }

    pub fn until(&self) -> chrono::DateTime<chrono::Utc> {
        self.until
    }

    pub fn is_valid(&self) -> bool {
        self.from < self.until
    }

    pub fn is_larger_than(&self, duration: chrono::Duration) -> bool {
        self.until - self.from > duration
    }
}
