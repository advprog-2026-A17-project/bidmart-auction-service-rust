//! Standalone binary for profiling the NAIVE (before-optimization) implementation.
//!
//! Usage:
//!   cargo build --release --bin profile_bidding_naive
//!   samply record ./target/release/profile_bidding_naive
//!
//! This exercises the naive O(n) linear-scan implementation so samply captures
//! the CPU hotspot on the linear scan. Compare the flamegraph with
//! profile_bidding (optimized) to see the improvement.

fn run_profile(iterations: u64, bids_per_iteration: u64) -> u64 {
    let mut accepted = 0u64;
    for _ in 0..iterations {
        let mut auction = NaiveAuction::new(
            "auction-prof",
            "seller-1",
            10_000,
            100,
            5_000_000,
            1_000_000,
            2_000_000,
        );

        for i in 0..bids_per_iteration {
            let bidder = if i % 2 == 0 { "bidder-a" } else { "bidder-b" };
            let amount = 10_000 + (i + 1) * 100;
            let time = 1_000_000 + i + 1;
            if auction.place_bid(bidder, amount, time).is_ok() {
                accepted += 1;
            }
        }
        let _ = std::hint::black_box(auction.determine_outcome());
    }
    accepted
}

#[cfg(not(test))]
fn main() {
    let iterations = 10_000u64;
    let bids_per_iteration = 200u64;

    eprintln!(
        "[profile_bidding_naive] Starting: {} iterations × {} bids = {} total place_bid calls",
        iterations,
        bids_per_iteration,
        iterations * bids_per_iteration,
    );

    let start = std::time::Instant::now();

    for iter in 0..iterations {
        let _ = run_profile(1, bids_per_iteration);
        if iter % 2000 == 0 {
            eprintln!("[profile_bidding_naive] Progress: {}/{}", iter, iterations);
        }
    }

    let elapsed = start.elapsed();
    let total_bids = iterations * bids_per_iteration;
    let ns_per_bid = elapsed.as_nanos() / total_bids as u128;

    eprintln!("[profile_bidding_naive] Done.");
    eprintln!("[profile_bidding_naive] Total time:   {:?}", elapsed);
    eprintln!("[profile_bidding_naive] Total bids:   {}", total_bids);
    eprintln!("[profile_bidding_naive] Per bid:      {} ns", ns_per_bid);
}

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    use super::{NaiveAuction, run_profile};

    #[test]
    fn run_profile_accepts_all_bids() {
        assert_eq!(run_profile(1, 5), 5);
    }

    #[test]
    fn place_bid_rejects_non_active_status() {
        let mut auction = NaiveAuction::new("a", "s", 100, 10, 1000, 0, 100);
        auction.status = "ENDED".to_string();
        assert!(auction.place_bid("u", 200, 1).is_err());
    }

    #[test]
    fn determine_outcome_unsold_without_bids() {
        let auction = NaiveAuction::new("a", "s", 100, 10, 1000, 0, 100);
        assert_eq!(auction.determine_outcome(), "UNSOLD");
    }
}

// ── Naive (before) implementation ───────────────────────────────────────────
// This deliberately uses:
// - String for status (heap allocation on every comparison)
// - Vec<Bid> for history (O(n) linear scan to find highest)
// - .iter().cloned().max_by_key() (redundant cloning of all bids)
// - String::to_string() for bidder_id (heap allocation per bid)

#[derive(Debug, Clone)]
struct Bid {
    #[allow(dead_code)]
    bidder_id: String,
    amount_cents: u64,
    #[allow(dead_code)]
    placed_at: u64,
}

#[derive(Debug)]
struct NaiveAuction {
    #[allow(dead_code)]
    id: String,
    seller_id: String,
    starting_price_cents: u64,
    minimum_increment_cents: u64,
    reserve_price_cents: u64,
    start_at: u64,
    end_at: u64,
    status: String,
    bids: Vec<Bid>,
    max_extensions: u32,
    extensions: u32,
}

impl NaiveAuction {
    fn new(
        id: &str,
        seller_id: &str,
        starting_price: u64,
        min_increment: u64,
        reserve: u64,
        start: u64,
        end: u64,
    ) -> Self {
        Self {
            id: id.to_string(),
            seller_id: seller_id.to_string(),
            starting_price_cents: starting_price,
            minimum_increment_cents: min_increment,
            reserve_price_cents: reserve,
            start_at: start,
            end_at: end,
            status: "ACTIVE".to_string(),
            bids: Vec::new(),
            max_extensions: 3,
            extensions: 0,
        }
    }

    fn place_bid(&mut self, bidder_id: &str, amount_cents: u64, now: u64) -> Result<(), String> {
        // String comparison (heap-allocated)
        if self.status != "ACTIVE" && self.status != "EXTENDED" {
            return Err(format!("not active: {}", self.status));
        }
        if now < self.start_at {
            return Err("not started".to_string());
        }
        if now >= self.end_at {
            self.status = "ENDED".to_string();
            return Err("ended".to_string());
        }
        if bidder_id == self.seller_id.as_str() {
            return Err("self bid".to_string());
        }

        // *** THE BOTTLENECK: O(n) linear scan over ALL bids ***
        let current_highest = self
            .bids
            .iter()
            .cloned() // Redundant clone of each Bid
            .max_by_key(|b| b.amount_cents);

        let minimum = match &current_highest {
            Some(bid) => bid.amount_cents + self.minimum_increment_cents,
            None => self.starting_price_cents,
        };

        if amount_cents < minimum {
            return Err(format!("too low: {}", minimum));
        }

        // Heap allocation for bidder_id String
        self.bids.push(Bid {
            bidder_id: bidder_id.to_string(),
            amount_cents,
            placed_at: now,
        });

        // Anti-sniping with String mutation
        let remaining = self.end_at.saturating_sub(now);
        if remaining <= 120 && self.extensions < self.max_extensions {
            self.end_at = now + 120;
            self.extensions += 1;
            self.status = "EXTENDED".to_string(); // Heap allocation
        }

        Ok(())
    }

    fn determine_outcome(&self) -> &str {
        // *** O(n) linear scan + clone to find winner ***
        let highest = self.bids.iter().cloned().max_by_key(|b| b.amount_cents);
        match highest {
            Some(bid) if bid.amount_cents >= self.reserve_price_cents => "WON",
            _ => "UNSOLD",
        }
    }
}
