use crate::listing_auction_session::{BidError, ListingAuctionSessionStatus, Money, UserId};
use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord};

pub struct TimeBidPolicy;
pub struct IdentityBidPolicy;
pub struct AmountBidPolicy;
pub struct WalletBidPolicy;

impl TimeBidPolicy {
    pub fn validate(auction: &ListingAuctionSessionRecord, bid_time: i64) -> Result<(), BidError> {
        let status = map_status(&auction.status);
        if status != ListingAuctionSessionStatus::Active
            && status != ListingAuctionSessionStatus::Extended
        {
            return Err(BidError::AuctionNotActive { status });
        }

        if bid_time < auction.start_time {
            return Err(BidError::AuctionNotStarted {
                start_at: crate::listing_auction_session::UnixSeconds::new(
                    auction.start_time as u64,
                ),
            });
        }

        if bid_time >= auction.end_time {
            return Err(BidError::AuctionEnded {
                end_at: crate::listing_auction_session::UnixSeconds::new(auction.end_time as u64),
            });
        }

        Ok(())
    }
}

impl IdentityBidPolicy {
    pub fn validate(
        auction: &ListingAuctionSessionRecord,
        bidder_id: &str,
        winning_bid: Option<&BidRecord>,
    ) -> Result<(), BidError> {
        if bidder_id == auction.seller_id {
            return Err(BidError::SelfBiddingNotAllowed {
                bidder_id: UserId::new(bidder_id),
            });
        }

        if winning_bid
            .map(|bid| bid.bidder_id.as_str() == bidder_id)
            .unwrap_or(false)
        {
            return Err(BidError::SelfBiddingNotAllowed {
                bidder_id: UserId::new(bidder_id),
            });
        }

        Ok(())
    }
}

impl AmountBidPolicy {
    pub fn validate(
        auction: &ListingAuctionSessionRecord,
        bid_amount_cents: i64,
        winning_bid: Option<&BidRecord>,
    ) -> Result<(), BidError> {
        let minimum = winning_bid
            .map(|bid| bid.bid_amount_cents + auction.minimum_increment_cents)
            .unwrap_or(auction.starting_price_cents);

        if bid_amount_cents < minimum {
            return Err(BidError::BidTooLow {
                minimum: Money::from_cents(minimum as u64),
            });
        }

        Ok(())
    }
}

impl WalletBidPolicy {
    pub fn validate(accepted_bid_amount_cents: i64) -> Result<(), BidError> {
        if accepted_bid_amount_cents <= 0 {
            return Err(BidError::BidTooLow {
                minimum: Money::from_cents(1),
            });
        }
        Ok(())
    }
}

fn map_status(status: &str) -> ListingAuctionSessionStatus {
    match status {
        "DRAFT" | "SCHEDULED" => ListingAuctionSessionStatus::Draft,
        "ACTIVE" => ListingAuctionSessionStatus::Active,
        "EXTENDED" => ListingAuctionSessionStatus::Extended,
        "CLOSED" | "ENDED" => ListingAuctionSessionStatus::Closed,
        "WON" => ListingAuctionSessionStatus::Won,
        "UNSOLD" => ListingAuctionSessionStatus::Unsold,
        "CANCELLED" => ListingAuctionSessionStatus::Cancelled,
        _ => ListingAuctionSessionStatus::Draft,
    }
}
