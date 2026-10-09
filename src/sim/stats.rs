/* Simulation and Match Statistics Tracking */

#[derive(Default, Clone, Debug)]
pub struct SimStats {
    pub hands: u64,
    pub wins: u64,
    pub ties: u64,
    pub losses: u64,
    pub total_chips: f64,
    pub sq_chips: f64,
}

impl SimStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, payoff: f64) {
        self.hands += 1;
        if payoff > 0.0 {
            self.wins += 1;
        } else if payoff == 0.0 {
            self.ties += 1;
        } else {
            self.losses += 1;
        }
        self.total_chips += payoff;
        self.sq_chips += payoff * payoff;
    }

    pub fn win_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.wins as f64 / self.hands as f64) * 100.0
        }
    }

    pub fn loss_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.losses as f64 / self.hands as f64) * 100.0
        }
    }

    pub fn tie_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.ties as f64 / self.hands as f64) * 100.0
        }
    }

    pub fn avg_chips(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            self.total_chips / self.hands as f64
        }
    }

    pub fn se_chips(&self) -> f64 {
        if self.hands <= 1 {
            return 0.0;
        }
        let n = self.hands as f64;
        let mean = self.avg_chips();
        let var = (self.sq_chips / n) - (mean * mean);
        (var.max(0.0) / n).sqrt()
    }
}
