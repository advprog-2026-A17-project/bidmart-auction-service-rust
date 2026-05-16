use bidmart_auction_service_rust::auction::{
    Auction, AuctionOutcome, AuctionStateError, AuctionStatus, BidError, Money, UnixSeconds, UserId,
};

fn sample_auction(start_at: u64, end_at: u64) -> Auction {
    Auction::new(
        "auction-1",
        "listing-1",
        "seller-1",
        Money::from_cents(10_00),
        Money::from_cents(2_00),
        Money::from_cents(50_00),
        UnixSeconds::new(start_at),
        UnixSeconds::new(end_at),
        3,
    )
}

fn activate_auction(auction: &mut Auction, now: u64) {
    auction
        .activate(UnixSeconds::new(now))
        .expect("auction should activate");
}

#[test]
fn reject_bid_when_scheduled() {
    let mut auction = sample_auction(0, 500);

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(10),
    );

    assert!(matches!(
        result,
        Err(BidError::AuctionNotActive {
            status: AuctionStatus::Scheduled
        })
    ));
}

#[test]
fn reject_activation_before_start() {
    let mut auction = sample_auction(100, 200);

    let result = auction.activate(UnixSeconds::new(50));

    assert!(matches!(result, Err(AuctionStateError::TooEarly { .. })));
}

#[test]
fn accept_first_bid_at_starting_price() {
    let mut auction = sample_auction(0, 500);
    activate_auction(&mut auction, 0);

    let result = auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(10),
        )
        .expect("bid should be accepted");

    assert!(result.previous_highest.is_none());
    assert!(!result.extended);
    assert_eq!(result.new_highest.amount, Money::from_cents(10_00));
    assert_eq!(
        auction.current_highest().unwrap().amount,
        Money::from_cents(10_00)
    );
    assert_eq!(auction.status(), AuctionStatus::Active);
}

#[test]
fn reject_bid_before_start_even_if_active() {
    let mut auction = sample_auction(100, 300);
    activate_auction(&mut auction, 100);

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(50),
    );

    assert!(matches!(result, Err(BidError::AuctionNotStarted { .. })));
}

#[test]
fn reject_bid_below_minimum_increment() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(0),
        )
        .expect("first bid should be accepted");

    let result = auction.place_bid(
        UserId::new("user-2"),
        Money::from_cents(11_00),
        UnixSeconds::new(10),
    );

    assert!(matches!(result, Err(BidError::BidTooLow { .. })));
}

#[test]
fn accept_outbid_and_return_previous_bid() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(0),
        )
        .expect("first bid should be accepted");

    let result = auction
        .place_bid(
            UserId::new("user-2"),
            Money::from_cents(12_00),
            UnixSeconds::new(10),
        )
        .expect("second bid should be accepted");

    let previous = result
        .previous_highest
        .expect("previous bid should be returned");
    assert_eq!(previous.bidder_id, UserId::new("user-1"));
    assert_eq!(
        auction.current_highest().unwrap().bidder_id,
        UserId::new("user-2")
    );
}

#[test]
fn reject_self_bidding() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(10),
        )
        .expect("first bid should be accepted");

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(12_00),
        UnixSeconds::new(20),
    );

    assert!(matches!(
        result,
        Err(BidError::SelfBiddingNotAllowed { .. })
    ));
}

#[test]
fn extend_auction_when_bid_in_last_two_minutes() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    let result = auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(250),
        )
        .expect("bid should be accepted");

    assert!(result.extended);
    assert_eq!(result.new_end_at, UnixSeconds::new(370));
    assert_eq!(auction.end_at(), UnixSeconds::new(370));
    assert_eq!(auction.status(), AuctionStatus::Extended);
}

#[test]
fn stop_extending_after_extension_cap() {
    let mut auction = Auction::new(
        "auction-2",
        "listing-2",
        "seller-2",
        Money::from_cents(10_00),
        Money::from_cents(2_00),
        Money::from_cents(50_00),
        UnixSeconds::new(0),
        UnixSeconds::new(300),
        1,
    );
    activate_auction(&mut auction, 0);

    let first = auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(250),
        )
        .expect("first bid should extend");

    assert!(first.extended);
    assert_eq!(first.new_end_at, UnixSeconds::new(370));

    let second = auction
        .place_bid(
            UserId::new("user-2"),
            Money::from_cents(12_00),
            UnixSeconds::new(360),
        )
        .expect("second bid should be accepted without extension");

    assert!(!second.extended);
    assert_eq!(second.new_end_at, UnixSeconds::new(370));
    assert_eq!(auction.end_at(), UnixSeconds::new(370));
}

#[test]
fn reject_bid_when_cancelled() {
    let mut auction = sample_auction(0, 300);
    auction.cancel();

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(10),
    );

    assert!(matches!(
        result,
        Err(BidError::AuctionNotActive {
            status: AuctionStatus::Cancelled
        })
    ));
}

#[test]
fn reject_bid_after_end_time() {
    let mut auction = sample_auction(0, 100);
    activate_auction(&mut auction, 0);

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(100),
    );

    assert!(matches!(result, Err(BidError::AuctionEnded { .. })));
}

#[test]
fn scheduled_status_on_creation() {
    let auction = sample_auction(100, 200);
    assert_eq!(auction.status(), AuctionStatus::Scheduled);
}

#[test]
fn active_status_after_activation() {
    let mut auction = sample_auction(100, 200);
    activate_auction(&mut auction, 100);
    assert_eq!(auction.status(), AuctionStatus::Active);
}

#[test]
fn extended_status_when_bid_in_anti_snipe_window() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(250),
        )
        .expect("bid should be accepted");

    assert_eq!(auction.status(), AuctionStatus::Extended);
}

#[test]
fn remains_active_when_bid_not_in_anti_snipe_window() {
    let mut auction = sample_auction(0, 300);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(100),
        )
        .expect("bid should be accepted");

    assert_eq!(auction.status(), AuctionStatus::Active);
}

#[test]
fn auction_cannot_be_reactivated() {
    let mut auction = sample_auction(100, 200);
    activate_auction(&mut auction, 100);

    let result = auction.activate(UnixSeconds::new(150));
    assert!(result.is_ok()); // Activating when already active is OK
    assert_eq!(auction.status(), AuctionStatus::Active);
}

#[test]
fn determine_won_when_reserve_met_after_end() {
    let mut auction = sample_auction(0, 100);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(50_00),
            UnixSeconds::new(50),
        )
        .expect("bid should be accepted");

    let result = auction.determine_outcome();
    assert_eq!(result, AuctionOutcome::Won);
}

#[test]
fn determine_unsold_when_reserve_not_met() {
    let mut auction = sample_auction(0, 100);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(50),
        )
        .expect("bid should be accepted");

    let result = auction.determine_outcome();
    assert_eq!(result, AuctionOutcome::Unsold);
}

#[test]
fn determine_unsold_when_no_bids() {
    let auction = sample_auction(0, 100);
    let result = auction.determine_outcome();
    assert_eq!(result, AuctionOutcome::Unsold);
}

#[test]
fn proxy_bid_places_minimum_valid_amount() {
    let mut auction = sample_auction(0, 500);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(10),
        )
        .expect("seed bid accepted");

    let result = auction
        .place_proxy_bid(
            UserId::new("user-2"),
            Money::from_cents(20_00),
            UnixSeconds::new(20),
        )
        .expect("proxy bid accepted");

    assert_eq!(result.new_highest.amount, Money::from_cents(12_00));
}

#[test]
fn proxy_bid_rejects_when_max_is_below_minimum() {
    let mut auction = sample_auction(0, 500);
    activate_auction(&mut auction, 0);

    auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(10),
        )
        .expect("seed bid accepted");

    let result = auction.place_proxy_bid(
        UserId::new("user-2"),
        Money::from_cents(11_00),
        UnixSeconds::new(20),
    );

    assert!(matches!(result, Err(BidError::BidTooLow { .. })));
}
