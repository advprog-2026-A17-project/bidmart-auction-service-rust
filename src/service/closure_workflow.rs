use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord};

pub struct ClosureDecision {
    pub status: &'static str,
    pub highest_bid_cents: Option<i64>,
}

pub trait AuctionClosureWorkflow: Send + Sync {
    fn determine_outcome(
        &self,
        auction: &ListingAuctionSessionRecord,
        bids: &[BidRecord],
    ) -> ClosureDecision;
}

pub struct EnglishReserveClosureWorkflow;

impl AuctionClosureWorkflow for EnglishReserveClosureWorkflow {
    fn determine_outcome(
        &self,
        auction: &ListingAuctionSessionRecord,
        bids: &[BidRecord],
    ) -> ClosureDecision {
        let highest_bid_cents = bids
            .first()
            .map(|bid| bid.bid_amount_cents)
            .or(auction.current_highest_bid_cents);
        let status = if highest_bid_cents
            .map(|amount| amount >= auction.reserve_price_cents)
            .unwrap_or(false)
        {
            "WON"
        } else {
            "UNSOLD"
        };

        ClosureDecision {
            status,
            highest_bid_cents,
        }
    }
}

pub struct AuctionClosureWorkflowFactory;

impl AuctionClosureWorkflowFactory {
    pub fn for_auction(_auction: &ListingAuctionSessionRecord) -> Box<dyn AuctionClosureWorkflow> {
        Box::new(EnglishReserveClosureWorkflow)
    }
}
