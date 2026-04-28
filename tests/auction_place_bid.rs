use bidmart_auction_service_rust::auction::{
    Auction, AuctionStatus, BidError, Money, UnixSeconds, UserId,
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
    )
}

#[test]
fn reject_bid_before_start() {
    let mut auction = sample_auction(100, 200);

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(50),
    );

    assert!(matches!(result, Err(BidError::AuctionNotStarted { .. })));
}

#[test]
fn accept_first_bid_at_starting_price() {
    let mut auction = sample_auction(100, 200);

    let result = auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(100),
        )
        .expect("bid should be accepted");

    assert!(result.previous_highest.is_none());
    assert_eq!(result.new_highest.amount, Money::from_cents(10_00));
    assert_eq!(auction.current_highest().unwrap().amount, Money::from_cents(10_00));
    assert_eq!(auction.status(), AuctionStatus::Active);
}

#[test]
fn reject_bid_below_minimum_increment() {
    let mut auction = sample_auction(0, 300);

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

    let previous = result.previous_highest.expect("previous bid should be returned");
    assert_eq!(previous.bidder_id, UserId::new("user-1"));
    assert_eq!(auction.current_highest().unwrap().bidder_id, UserId::new("user-2"));
}

#[test]
fn extend_auction_when_bid_in_last_two_minutes() {
    let mut auction = sample_auction(0, 300);

    let result = auction
        .place_bid(
            UserId::new("user-1"),
            Money::from_cents(10_00),
            UnixSeconds::new(250),
        )
        .expect("bid should be accepted");

    assert!(result.extended);
    assert_eq!(result.new_end_at, UnixSeconds::new(420));
    assert_eq!(auction.end_at(), UnixSeconds::new(420));
    assert_eq!(auction.status(), AuctionStatus::Extended);
}

#[test]
fn reject_bid_after_end_time() {
    let mut auction = sample_auction(0, 100);

    let result = auction.place_bid(
        UserId::new("user-1"),
        Money::from_cents(10_00),
        UnixSeconds::new(100),
    );

    assert!(matches!(result, Err(BidError::AuctionEnded { .. })));
}
