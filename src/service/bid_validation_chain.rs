use crate::listing_auction_session::BidError;
use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord};
use crate::service::bid_policies::{
    AmountBidPolicy, IdentityBidPolicy, TimeBidPolicy, WalletBidPolicy,
};

pub trait BidValidationLink: Send + Sync {
    fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError>;
}

pub struct BidValidationContext<'a> {
    pub auction: &'a ListingAuctionSessionRecord,
    pub bidder_id: &'a str,
    pub bid_time: i64,
    pub amount_cents: i64,
    pub winning_bid: Option<&'a BidRecord>,
}

pub struct BidValidationChain {
    links: Vec<Box<dyn BidValidationLink>>,
}

impl BidValidationChain {
    pub fn standard_bid() -> Self {
        Self {
            links: vec![
                Box::new(TimeValidationLink),
                Box::new(IdentityValidationLink),
                Box::new(AmountValidationLink),
                Box::new(WalletAmountValidationLink),
            ],
        }
    }

    pub fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError> {
        for link in &self.links {
            link.validate(context)?;
        }
        Ok(())
    }
}

struct TimeValidationLink;

impl BidValidationLink for TimeValidationLink {
    fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError> {
        TimeBidPolicy::validate(context.auction, context.bid_time)
    }
}

struct IdentityValidationLink;

impl BidValidationLink for IdentityValidationLink {
    fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError> {
        IdentityBidPolicy::validate(context.auction, context.bidder_id, context.winning_bid)
    }
}

struct AmountValidationLink;

impl BidValidationLink for AmountValidationLink {
    fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError> {
        AmountBidPolicy::validate(context.auction, context.amount_cents, context.winning_bid)
    }
}

struct WalletAmountValidationLink;

impl BidValidationLink for WalletAmountValidationLink {
    fn validate(&self, context: &BidValidationContext<'_>) -> Result<(), BidError> {
        WalletBidPolicy::validate(context.amount_cents)
    }
}
