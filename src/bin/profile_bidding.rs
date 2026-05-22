/// Standalone binary for real profiling with samply/perf/instruments.
///
/// Usage:
///   cargo build --release --bin profile_bidding
///   samply record ./target/release/profile_bidding
///
/// This exercises the critical bidding hot-path (domain layer) in a tight
/// loop so that samply can capture meaningful CPU samples.

use bidmart_auction_service_rust::listing_auction_session::{
    ListingAuctionSession, ListingAuctionSessionStatus, Money, UnixSeconds, UserId,
};

fn run_profile(iterations: u64, bids_per_iteration: u64) -> u64 {
    let mut accepted = 0u64;
    for _ in 0..iterations {
        let mut auction = ListingAuctionSession::with_status(
            "auction-prof",
            "listing-prof",
            "seller-1",
            Money::from_cents(10_000),
            Money::from_cents(100),
            Money::from_cents(5_000_000),
            UnixSeconds::new(1_000_000),
            UnixSeconds::new(2_000_000),
            ListingAuctionSessionStatus::Active,
            None,
        );

        for i in 0..bids_per_iteration {
            let bidder = if i % 2 == 0 { "bidder-a" } else { "bidder-b" };
            let amount = 10_000 + (i + 1) * 100;
            let time = 1_000_000 + i + 1;
            if auction
                .place_bid(
                    UserId::new(bidder),
                    Money::from_cents(amount),
                    UnixSeconds::new(time),
                )
                .is_ok()
            {
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
        "[profile_bidding] Starting: {} iterations × {} bids = {} total place_bid calls",
        iterations,
        bids_per_iteration,
        iterations * bids_per_iteration,
    );

    let start = std::time::Instant::now();

    for iter in 0..iterations {
        let _ = run_profile(1, bids_per_iteration);
        if iter % 2000 == 0 {
            eprintln!("[profile_bidding] Progress: {}/{}", iter, iterations);
        }
    }

    let elapsed = start.elapsed();
    let total_bids = iterations * bids_per_iteration;
    let ns_per_bid = elapsed.as_nanos() / total_bids as u128;

    eprintln!("[profile_bidding] Done.");
    eprintln!("[profile_bidding] Total time:   {:?}", elapsed);
    eprintln!("[profile_bidding] Total bids:   {}", total_bids);
    eprintln!("[profile_bidding] Per bid:      {} ns", ns_per_bid);
}

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    use super::run_profile;

    #[test]
    fn run_profile_accepts_all_bids() {
        assert_eq!(run_profile(1, 5), 5);
    }
}
