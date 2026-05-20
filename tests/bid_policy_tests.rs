use bidmart_auction_service_rust::listing_auction_session::BidError;
use bidmart_auction_service_rust::persistence::models::{BidRecord, ListingAuctionSessionRecord};
use bidmart_auction_service_rust::service::bid_policies::{
    AmountBidPolicy, IdentityBidPolicy, TimeBidPolicy, WalletBidPolicy,
};

fn test_auction(status: &str, start: i64, end: i64) -> ListingAuctionSessionRecord {
    ListingAuctionSessionRecord {
        id: "auction-1".to_string(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        auction_type: "ENGLISH".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 2000,
        minimum_increment_cents: 100,
        current_highest_bid_cents: None,
        status: status.to_string(),
        start_time: start,
        end_time: end,
        created_at: 0,
        updated_at: 0,
    }
}

fn test_bid(bidder_id: &str, amount: i64) -> BidRecord {
    BidRecord {
        id: "bid-1".to_string(),
        auction_id: "auction-1".to_string(),
        bidder_id: bidder_id.to_string(),
        bid_amount_cents: amount,
        wallet_hold_id: None,
        bid_time: 100,
    }
}

// ============================================
// TimeBidPolicy tests
// ============================================

#[test]
fn time_policy_accepts_bid_in_active_window() {
    let auction = test_auction("ACTIVE", 100, 200);
    assert!(TimeBidPolicy::validate(&auction, 150).is_ok());
}

#[test]
fn time_policy_accepts_bid_in_extended_status() {
    let auction = test_auction("EXTENDED", 100, 200);
    assert!(TimeBidPolicy::validate(&auction, 150).is_ok());
}

#[test]
fn time_policy_rejects_draft_auction() {
    let auction = test_auction("DRAFT", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_rejects_closed_auction() {
    let auction = test_auction("CLOSED", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_rejects_won_auction() {
    let auction = test_auction("WON", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_rejects_unsold_auction() {
    let auction = test_auction("UNSOLD", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_rejects_cancelled_auction() {
    let auction = test_auction("CANCELLED", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_rejects_bid_before_start() {
    let auction = test_auction("ACTIVE", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 50).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotStarted { .. }));
}

#[test]
fn time_policy_rejects_bid_at_exact_end() {
    let auction = test_auction("ACTIVE", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 200).unwrap_err();
    assert!(matches!(err, BidError::AuctionEnded { .. }));
}

#[test]
fn time_policy_rejects_bid_well_after_end() {
    let auction = test_auction("ACTIVE", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 999).unwrap_err();
    assert!(matches!(err, BidError::AuctionEnded { .. }));
}

#[test]
fn time_policy_maps_scheduled_to_draft() {
    let auction = test_auction("SCHEDULED", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_maps_ended_to_closed() {
    let auction = test_auction("ENDED", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn time_policy_maps_unknown_status_to_draft() {
    let auction = test_auction("UNKNOWN_STATUS", 100, 200);
    let err = TimeBidPolicy::validate(&auction, 150).unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

// ============================================
// IdentityBidPolicy tests
// ============================================

#[test]
fn identity_policy_accepts_different_bidder() {
    let auction = test_auction("ACTIVE", 100, 200);
    assert!(IdentityBidPolicy::validate(&auction, "buyer-1", None).is_ok());
}

#[test]
fn identity_policy_rejects_seller_bidding() {
    let auction = test_auction("ACTIVE", 100, 200);
    let err = IdentityBidPolicy::validate(&auction, "seller-1", None).unwrap_err();
    assert!(matches!(err, BidError::SelfBiddingNotAllowed { .. }));
}

#[test]
fn identity_policy_rejects_current_winner_bidding_again() {
    let auction = test_auction("ACTIVE", 100, 200);
    let bid = test_bid("buyer-1", 2000);
    let err = IdentityBidPolicy::validate(&auction, "buyer-1", Some(&bid)).unwrap_err();
    assert!(matches!(err, BidError::SelfBiddingNotAllowed { .. }));
}

#[test]
fn identity_policy_accepts_different_bidder_when_winner_exists() {
    let auction = test_auction("ACTIVE", 100, 200);
    let bid = test_bid("buyer-1", 2000);
    assert!(IdentityBidPolicy::validate(&auction, "buyer-2", Some(&bid)).is_ok());
}

// ============================================
// AmountBidPolicy tests
// ============================================

#[test]
fn amount_policy_accepts_starting_price_with_no_bids() {
    let auction = test_auction("ACTIVE", 100, 200);
    assert!(AmountBidPolicy::validate(&auction, 1000, None).is_ok());
}

#[test]
fn amount_policy_rejects_below_starting_price() {
    let auction = test_auction("ACTIVE", 100, 200);
    let err = AmountBidPolicy::validate(&auction, 500, None).unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}

#[test]
fn amount_policy_accepts_increment_above_previous() {
    let auction = test_auction("ACTIVE", 100, 200);
    let bid = test_bid("buyer-1", 2000);
    assert!(AmountBidPolicy::validate(&auction, 2100, Some(&bid)).is_ok());
}

#[test]
fn amount_policy_rejects_below_increment() {
    let auction = test_auction("ACTIVE", 100, 200);
    let bid = test_bid("buyer-1", 2000);
    let err = AmountBidPolicy::validate(&auction, 2050, Some(&bid)).unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}

#[test]
fn amount_policy_accepts_exact_increment() {
    let auction = test_auction("ACTIVE", 100, 200);
    let bid = test_bid("buyer-1", 2000);
    // minimum = 2000 + 100 = 2100
    assert!(AmountBidPolicy::validate(&auction, 2100, Some(&bid)).is_ok());
}

// ============================================
// WalletBidPolicy tests
// ============================================

#[test]
fn wallet_policy_accepts_positive_amount() {
    assert!(WalletBidPolicy::validate(500).is_ok());
}

#[test]
fn wallet_policy_rejects_zero() {
    let err = WalletBidPolicy::validate(0).unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}

#[test]
fn wallet_policy_rejects_negative() {
    let err = WalletBidPolicy::validate(-100).unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}
