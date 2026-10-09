/* Opponent Archetype and Style Classification */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpponentStyle {
    Unknown,
    CallingStation,
    Maniac,
    Rock,
    Tag,
    Lag,
}

impl std::fmt::Display for OpponentStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpponentStyle::Unknown => write!(f, "Unknown"),
            OpponentStyle::CallingStation => write!(f, "Calling Station (Loose-Passive)"),
            OpponentStyle::Maniac => write!(f, "Maniac (Hyper-Aggressive)"),
            OpponentStyle::Rock => write!(f, "Rock / Nit (Tight-Passive)"),
            OpponentStyle::Tag => write!(f, "TAG (Tight-Aggressive)"),
            OpponentStyle::Lag => write!(f, "LAG (Loose-Aggressive)"),
        }
    }
}
