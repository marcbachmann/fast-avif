#[derive(Clone)]
pub struct Rav1dDataProps {
    pub timestamp: i64,
    pub duration: i64,
    pub offset: i64,
    pub size: usize,
}

impl Default for Rav1dDataProps {
    fn default() -> Self {
        Self {
            timestamp: i64::MIN,
            duration: 0,
            offset: -1,
            size: 0,
        }
    }
}
