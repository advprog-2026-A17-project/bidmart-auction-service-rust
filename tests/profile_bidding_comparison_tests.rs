//! Before/after domain-layer comparison (naive Vec+String vs optimized enum+Option).
//! Run: `cargo test profile_bidding_comparison -- --nocapture`
//! Writes: `artifacts/profiling-before-after.log`

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use bidmart_auction_service_rust::listing_auction_session::{
    ListingAuctionSession, ListingAuctionSessionStatus, Money, UnixSeconds, UserId,
};

const SEQUENTIAL_BIDS: usize = 100;
const SEQUENTIAL_ITERS: usize = 1000;

// ── Naive baseline (mirrors src/bin/profile_bidding_naive.rs) ───────────────

#[derive(Debug, Clone)]
struct NaiveBid {
    #[allow(dead_code)]
    bidder_id: String,
    amount_cents: u64,
    #[allow(dead_code)]
    placed_at: u64,
}

#[derive(Debug)]
struct NaiveAuction {
    seller_id: String,
    starting_price_cents: u64,
    minimum_increment_cents: u64,
    reserve_price_cents: u64,
    start_at: u64,
    end_at: u64,
    status: String,
    bids: Vec<NaiveBid>,
    max_extensions: u32,
    extensions: u32,
}

impl NaiveAuction {
    fn new(
        seller_id: &str,
        starting_price: u64,
        min_increment: u64,
        reserve: u64,
        start: u64,
        end: u64,
    ) -> Self {
        Self {
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
        if self.status != "ACTIVE" && self.status != "EXTENDED" {
            return Err("not active".to_string());
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

        let current_highest = self.bids.iter().cloned().max_by_key(|b| b.amount_cents);

        let minimum = match &current_highest {
            Some(bid) => bid.amount_cents + self.minimum_increment_cents,
            None => self.starting_price_cents,
        };

        if amount_cents < minimum {
            return Err("too low".to_string());
        }

        self.bids.push(NaiveBid {
            bidder_id: bidder_id.to_string(),
            amount_cents,
            placed_at: now,
        });

        let remaining = self.end_at.saturating_sub(now);
        if remaining <= 120 && self.extensions < self.max_extensions {
            self.end_at = now + 120;
            self.extensions += 1;
            self.status = "EXTENDED".to_string();
        }

        Ok(())
    }

    fn determine_outcome(&self) -> &'static str {
        let highest = self.bids.iter().cloned().max_by_key(|b| b.amount_cents);
        match highest {
            Some(bid) if bid.amount_cents >= self.reserve_price_cents => "WON",
            _ => "UNSOLD",
        }
    }
}

fn bench_naive_sequential() -> u128 {
    let start = Instant::now();
    for _ in 0..SEQUENTIAL_ITERS {
        let mut auction =
            NaiveAuction::new("seller-1", 10_000, 100, 5_000_000, 1_000_000, 2_000_000);
        for i in 0..SEQUENTIAL_BIDS {
            let bidder = if i % 2 == 0 { "bidder-a" } else { "bidder-b" };
            let amount = 10_000 + (i + 1) as u64 * 100;
            let time = 1_000_000 + i as u64 + 1;
            let _ = auction.place_bid(bidder, amount, time);
        }
        let _ = auction.determine_outcome();
    }
    start.elapsed().as_micros()
}

fn bench_optimized_sequential() -> u128 {
    let start = Instant::now();
    for _ in 0..SEQUENTIAL_ITERS {
        let mut session = ListingAuctionSession::with_status(
            "auction-1",
            "listing-1",
            "seller-1",
            Money::from_cents(10_000),
            Money::from_cents(100),
            Money::from_cents(5_000_000),
            UnixSeconds::new(1_000_000),
            UnixSeconds::new(2_000_000),
            ListingAuctionSessionStatus::Active,
            None,
        );
        for i in 0..SEQUENTIAL_BIDS {
            let bidder = if i % 2 == 0 {
                UserId::new("bidder-a")
            } else {
                UserId::new("bidder-b")
            };
            let amount = Money::from_cents(10_000 + (i + 1) as u64 * 100);
            let time = UnixSeconds::new(1_000_000 + i as u64 + 1);
            let _ = session.place_bid(bidder, amount, time);
        }
        let _ = session.determine_outcome();
    }
    start.elapsed().as_micros()
}

fn write_log(naive_us: u128, optimized_us: u128) {
    let improvement = if naive_us > 0 {
        100.0 * (1.0 - optimized_us as f64 / naive_us as f64)
    } else {
        0.0
    };
    let body = format!(
        "# Profiling before/after (domain layer)\n\
         generated_at: {}\n\
         workload: {} bids x {} iterations\n\
         naive_total_us: {}\n\
         optimized_total_us: {}\n\
         improvement_percent: {:.1}\n\
         naive_per_bid_ns: {}\n\
         optimized_per_bid_ns: {}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        SEQUENTIAL_BIDS,
        SEQUENTIAL_ITERS,
        naive_us,
        optimized_us,
        improvement,
        naive_us * 1000 / (SEQUENTIAL_BIDS * SEQUENTIAL_ITERS) as u128,
        optimized_us * 1000 / (SEQUENTIAL_BIDS * SEQUENTIAL_ITERS) as u128,
    );

    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/profiling-before-after.log");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, &body).expect("write profiling log");
    println!("{body}");
    println!("Wrote {}", path.display());
}

#[test]
fn profile_bidding_comparison_naive_vs_optimized() {
    let naive_us = bench_naive_sequential();
    let optimized_us = bench_optimized_sequential();
    assert!(
        optimized_us < naive_us,
        "optimized should be faster than naive"
    );
    write_log(naive_us, optimized_us);
}
