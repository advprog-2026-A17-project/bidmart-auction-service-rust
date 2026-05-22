use bidmart_auction_service_rust::listing_auction_session::{
    Bid, ListingAuctionSession, ListingAuctionSessionOutcome, ListingAuctionSessionStatus, Money, UnixSeconds,
    UserId,
};
use bidmart_auction_service_rust::service::close_strategy::{
    default_close_strategy, CloseStrategy, EnglishReserveClose,
};

fn active_session_with_bid(bid_cents: u64) -> ListingAuctionSession {
    ListingAuctionSession::with_status(
        "session-1",
        "listing-1",
        "seller-1",
        Money::from_cents(1000),
        Money::from_cents(100),
        Money::from_cents(5000),
        UnixSeconds::new(100),
        UnixSeconds::new(1000),
        ListingAuctionSessionStatus::Active,
        Some(Bid {
            bidder_id: UserId::new("buyer-1"),
            amount: Money::from_cents(bid_cents),
            placed_at: UnixSeconds::new(500),
        }),
    )
}

#[test]
fn english_reserve_close_delegates_to_session_outcome() {
    let mut session = active_session_with_bid(6000);
    let strategy = EnglishReserveClose;
    let outcome = strategy.determine_outcome(&mut session);
    assert_eq!(ListingAuctionSessionOutcome::Won, outcome);
}

#[test]
fn default_close_strategy_returns_unsold_when_reserve_not_met() {
    let mut session = active_session_with_bid(1000);
    let outcome = default_close_strategy().determine_outcome(&mut session);
    assert_eq!(ListingAuctionSessionOutcome::Unsold, outcome);
}
