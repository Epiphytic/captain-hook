use super::ScopeLevel;

impl ScopeLevel {
    /// Returns the precedence rank (higher = more authoritative).
    pub fn precedence(&self) -> u8 {
        match self {
            ScopeLevel::Org => 4,
            ScopeLevel::Project => 3,
            ScopeLevel::User => 2,
            ScopeLevel::Role => 1,
        }
    }
}
