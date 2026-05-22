use bidmart_auction_service_rust::listing_auction_session::*;

fn session(status: ListingAuctionSessionStatus) -> ListingAuctionSession {
    ListingAuctionSession::with_status(
        "session-1",
        "listing-1",
        "seller-1",
        Money::from_cents(1000),
        Money::from_cents(100),
        Money::from_cents(5000),
        UnixSeconds::new(100),
        UnixSeconds::new(1000),
        status,
        None,
    )
}

fn session_with_bid(status: ListingAuctionSessionStatus, bid_cents: u64) -> ListingAuctionSession {
    ListingAuctionSession::with_status(
        "session-1",
        "listing-1",
        "seller-1",
        Money::from_cents(1000),
        Money::from_cents(100),
        Money::from_cents(5000),
        UnixSeconds::new(100),
        UnixSeconds::new(1000),
        status,
        Some(Bid {
            bidder_id: UserId::new("buyer-1"),
            amount: Money::from_cents(bid_cents),
            placed_at: UnixSeconds::new(500),
        }),
    )
}

// ============================================
// Constructor tests
// ============================================

#[test]
fn new_session_starts_in_draft_status() {
    let s = ListingAuctionSession::new(
        "s-1",
        "l-1",
        "seller-1",
        Money::from_cents(100),
        Money::from_cents(10),
        Money::from_cents(500),
        UnixSeconds::new(100),
        UnixSeconds::new(200),
    );
    assert_eq!(s.status(), ListingAuctionSessionStatus::Draft);
    assert!(s.current_highest().is_none());
    assert_eq!(s.extensions(), 0);
}

#[test]
fn with_status_preserves_given_status() {
    let s = session(ListingAuctionSessionStatus::Active);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Active);
}

#[test]
fn lifecycle_state_handlers_gate_bid_permissions() {
    assert!(
        lifecycle_state(ListingAuctionSessionStatus::Active)
            .ensure_can_bid()
            .is_ok()
    );
    assert!(
        lifecycle_state(ListingAuctionSessionStatus::Extended)
            .ensure_can_bid()
            .is_ok()
    );
    assert!(matches!(
        lifecycle_state(ListingAuctionSessionStatus::Draft)
            .ensure_can_bid()
            .unwrap_err(),
        BidError::AuctionNotActive { .. }
    ));
}

// ============================================
// Activate lifecycle tests
// ============================================

#[test]
fn activate_draft_to_active() {
    let mut s = session(ListingAuctionSessionStatus::Draft);
    assert!(s.activate(UnixSeconds::new(150)).is_ok());
    assert_eq!(s.status(), ListingAuctionSessionStatus::Active);
}

#[test]
fn activate_cancelled_returns_error() {
    let mut s = session(ListingAuctionSessionStatus::Cancelled);
    let err = s.activate(UnixSeconds::new(150)).unwrap_err();
    assert_eq!(err, ListingAuctionSessionStateError::Cancelled);
}

#[test]
fn activate_too_early_returns_error() {
    let mut s = session(ListingAuctionSessionStatus::Draft);
    let err = s.activate(UnixSeconds::new(50)).unwrap_err();
    assert!(matches!(
        err,
        ListingAuctionSessionStateError::TooEarly { .. }
    ));
}

#[test]
fn activate_closed_session_returns_already_ended() {
    let mut s = session(ListingAuctionSessionStatus::Closed);
    let err = s.activate(UnixSeconds::new(150)).unwrap_err();
    assert!(matches!(
        err,
        ListingAuctionSessionStateError::AlreadyEnded { .. }
    ));
}

#[test]
fn activate_won_session_returns_already_ended() {
    let mut s = session(ListingAuctionSessionStatus::Won);
    let err = s.activate(UnixSeconds::new(150)).unwrap_err();
    assert!(matches!(
        err,
        ListingAuctionSessionStateError::AlreadyEnded { .. }
    ));
}

#[test]
fn activate_unsold_session_returns_already_ended() {
    let mut s = session(ListingAuctionSessionStatus::Unsold);
    let err = s.activate(UnixSeconds::new(150)).unwrap_err();
    assert!(matches!(
        err,
        ListingAuctionSessionStateError::AlreadyEnded { .. }
    ));
}

#[test]
fn activate_past_end_time_transitions_to_closed() {
    let mut s = session(ListingAuctionSessionStatus::Draft);
    let err = s.activate(UnixSeconds::new(1500)).unwrap_err();
    assert!(matches!(
        err,
        ListingAuctionSessionStateError::AlreadyEnded { .. }
    ));
    assert_eq!(s.status(), ListingAuctionSessionStatus::Closed);
}

#[test]
fn activate_already_active_is_noop() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    assert!(s.activate(UnixSeconds::new(150)).is_ok());
    assert_eq!(s.status(), ListingAuctionSessionStatus::Active);
}

// ============================================
// Cancel tests
// ============================================

#[test]
fn cancel_transitions_to_cancelled() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    s.cancel();
    assert_eq!(s.status(), ListingAuctionSessionStatus::Cancelled);
}

// ============================================
// Close and determine_outcome tests
// ============================================

#[test]
fn close_sets_closed_status() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    s.close();
    assert_eq!(s.status(), ListingAuctionSessionStatus::Closed);
}

#[test]
fn determine_outcome_won_when_bid_above_reserve() {
    let mut s = session_with_bid(ListingAuctionSessionStatus::Active, 6000);
    s.close();
    let outcome = s.determine_outcome();
    assert_eq!(outcome, ListingAuctionSessionOutcome::Won);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Won);
}

#[test]
fn determine_outcome_unsold_when_bid_below_reserve() {
    let mut s = session_with_bid(ListingAuctionSessionStatus::Active, 3000);
    s.close();
    let outcome = s.determine_outcome();
    assert_eq!(outcome, ListingAuctionSessionOutcome::Unsold);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Unsold);
}

#[test]
fn determine_outcome_unsold_when_no_bids() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    s.close();
    let outcome = s.determine_outcome();
    assert_eq!(outcome, ListingAuctionSessionOutcome::Unsold);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Unsold);
}

#[test]
fn determine_outcome_won_when_bid_equals_reserve() {
    let mut s = session_with_bid(ListingAuctionSessionStatus::Active, 5000);
    s.close();
    let outcome = s.determine_outcome();
    assert_eq!(outcome, ListingAuctionSessionOutcome::Won);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Won);
}

// ============================================
// Place bid tests
// ============================================

#[test]
fn place_bid_succeeds_on_active_auction() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let result = s.place_bid(
        UserId::new("buyer-1"),
        Money::from_cents(1000),
        UnixSeconds::new(500),
    );
    assert!(result.is_ok());
    let accepted = result.unwrap();
    assert_eq!(accepted.new_highest.amount, Money::from_cents(1000));
    assert!(accepted.previous_highest.is_none());
}

#[test]
fn place_bid_succeeds_on_extended_auction() {
    let mut s = session(ListingAuctionSessionStatus::Extended);
    let result = s.place_bid(
        UserId::new("buyer-1"),
        Money::from_cents(1000),
        UnixSeconds::new(500),
    );
    assert!(result.is_ok());
}

#[test]
fn place_bid_rejects_draft_auction() {
    let mut s = session(ListingAuctionSessionStatus::Draft);
    let err = s
        .place_bid(
            UserId::new("buyer-1"),
            Money::from_cents(1000),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::AuctionNotActive { .. }));
}

#[test]
fn place_bid_rejects_before_start() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let err = s
        .place_bid(
            UserId::new("buyer-1"),
            Money::from_cents(1000),
            UnixSeconds::new(50),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::AuctionNotStarted { .. }));
}

#[test]
fn place_bid_rejects_after_end_and_closes() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let err = s
        .place_bid(
            UserId::new("buyer-1"),
            Money::from_cents(1000),
            UnixSeconds::new(1500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::AuctionEnded { .. }));
    assert_eq!(s.status(), ListingAuctionSessionStatus::Closed);
}

#[test]
fn place_bid_rejects_seller_bidding() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let err = s
        .place_bid(
            UserId::new("seller-1"),
            Money::from_cents(1000),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::SelfBiddingNotAllowed { .. }));
}

#[test]
fn place_bid_rejects_current_winner_rebid() {
    let mut s = session_with_bid(ListingAuctionSessionStatus::Active, 2000);
    let err = s
        .place_bid(
            UserId::new("buyer-1"),
            Money::from_cents(3000),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::SelfBiddingNotAllowed { .. }));
}

#[test]
fn place_bid_rejects_below_minimum() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let err = s
        .place_bid(
            UserId::new("buyer-1"),
            Money::from_cents(500),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}

#[test]
fn place_bid_increments_properly() {
    let mut s = session_with_bid(ListingAuctionSessionStatus::Active, 2000);
    // minimum_increment = 100, so minimum = 2100
    let err = s
        .place_bid(
            UserId::new("buyer-2"),
            Money::from_cents(2050),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));

    let result = s.place_bid(
        UserId::new("buyer-2"),
        Money::from_cents(2100),
        UnixSeconds::new(500),
    );
    assert!(result.is_ok());
}

// ============================================
// Anti-sniping extension tests
// ============================================

#[test]
fn bid_near_end_triggers_extension() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    // end_at = 1000, bid at 950 (50s remaining < 120s window)
    let result = s.place_bid(
        UserId::new("buyer-1"),
        Money::from_cents(1000),
        UnixSeconds::new(950),
    );
    assert!(result.is_ok());
    let accepted = result.unwrap();
    assert!(accepted.extended);
    assert_eq!(s.status(), ListingAuctionSessionStatus::Extended);
    assert_eq!(s.end_at(), UnixSeconds::new(950 + 120));
    assert_eq!(s.extensions(), 1);
}

#[test]
fn bid_not_near_end_does_not_extend() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    // end_at = 1000, bid at 500 (500s remaining > 120s window)
    let result = s.place_bid(
        UserId::new("buyer-1"),
        Money::from_cents(1000),
        UnixSeconds::new(500),
    );
    assert!(result.is_ok());
    assert!(!result.unwrap().extended);
    assert_eq!(s.extensions(), 0);
    assert_eq!(s.end_at(), UnixSeconds::new(1000)); // unchanged
}

#[test]
fn unlimited_extensions_allowed() {
    let mut s = session(ListingAuctionSessionStatus::Active);

    // Track end_at; each bid lands 1s before current end, triggering a 120s extension.
    let mut current_end: u64 = 1000;

    for i in 0u64..10 {
        // Bid 1 second before end_at → within the 120s anti-sniping window
        let bid_time = current_end - 1;
        let amount = 1000 + (i * 100);
        let bidder = if i % 2 == 0 { "buyer-a" } else { "buyer-b" };
        let result = s.place_bid(
            UserId::new(bidder),
            Money::from_cents(amount),
            UnixSeconds::new(bid_time),
        );
        assert!(result.is_ok(), "bid {i} should succeed");
        let accepted = result.unwrap();
        assert!(accepted.extended, "bid {i} should extend");
        // After extension: end_at = bid_time + 120
        current_end = bid_time + 120;
        assert_eq!(s.end_at(), UnixSeconds::new(current_end));
    }

    assert_eq!(s.extensions(), 10);
}

// ============================================
// Proxy bid tests
// ============================================

#[test]
fn proxy_bid_uses_minimum_required_amount() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let result = s.place_proxy_bid(
        UserId::new("buyer-1"),
        Money::from_cents(5000),
        UnixSeconds::new(500),
    );
    assert!(result.is_ok());
    // Starting price is 1000, so proxy bid should be exactly 1000 (minimum required)
    assert_eq!(result.unwrap().new_highest.amount, Money::from_cents(1000));
}

#[test]
fn proxy_bid_rejects_when_max_below_minimum() {
    let mut s = session(ListingAuctionSessionStatus::Active);
    let err = s
        .place_proxy_bid(
            UserId::new("buyer-1"),
            Money::from_cents(500),
            UnixSeconds::new(500),
        )
        .unwrap_err();
    assert!(matches!(err, BidError::BidTooLow { .. }));
}

// ============================================
// Newtype wrapper tests
// ============================================

#[test]
fn money_add() {
    let a = Money::from_cents(100);
    let b = Money::from_cents(50);
    assert_eq!((a + b).cents(), 150);
}

#[test]
fn money_add_assign() {
    let mut a = Money::from_cents(100);
    a += Money::from_cents(50);
    assert_eq!(a.cents(), 150);
}

#[test]
fn money_ordering() {
    assert!(Money::from_cents(200) > Money::from_cents(100));
    assert!(Money::from_cents(100) < Money::from_cents(200));
    assert_eq!(Money::from_cents(100), Money::from_cents(100));
}

#[test]
fn unix_seconds_add_secs() {
    let t = UnixSeconds::new(100);
    assert_eq!(t.add_secs(50).value(), 150);
}

// ============================================
// Accessor tests
// ============================================

#[test]
fn accessors_return_correct_values() {
    let s = session(ListingAuctionSessionStatus::Active);
    assert_eq!(s.id(), "session-1");
    assert_eq!(s.listing_id(), "listing-1");
    assert_eq!(s.starting_price(), Money::from_cents(1000));
    assert_eq!(s.reserve_price(), Money::from_cents(5000));
    assert_eq!(s.minimum_increment(), Money::from_cents(100));
    assert_eq!(s.end_at(), UnixSeconds::new(1000));
    assert_eq!(*s.seller_id(), UserId::new("seller-1"));
}

// ============================================
// Display/Debug for error types
// ============================================

#[test]
fn bid_error_display_messages() {
    let e = BidError::AuctionNotActive {
        status: ListingAuctionSessionStatus::Draft,
    };
    assert!(format!("{e}").contains("not active"));

    let e = BidError::BidTooLow {
        minimum: Money::from_cents(100),
    };
    assert!(format!("{e}").contains("minimum"));

    let e = BidError::SelfBiddingNotAllowed {
        bidder_id: UserId::new("u-1"),
    };
    assert!(format!("{e}").contains("self bidding"));

    let e = BidError::AuctionNotStarted {
        start_at: UnixSeconds::new(100),
    };
    assert!(format!("{e}").contains("not started"));

    let e = BidError::AuctionEnded {
        end_at: UnixSeconds::new(200),
    };
    assert!(format!("{e}").contains("ended"));
}
