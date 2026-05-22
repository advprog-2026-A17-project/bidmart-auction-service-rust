use crate::listing_auction_session::{ListingAuctionSession, ListingAuctionSessionOutcome};

pub trait CloseStrategy: Send + Sync {
    fn determine_outcome(
        &self,
        session: &mut ListingAuctionSession,
    ) -> ListingAuctionSessionOutcome;
}

pub struct EnglishReserveClose;

impl CloseStrategy for EnglishReserveClose {
    fn determine_outcome(
        &self,
        session: &mut ListingAuctionSession,
    ) -> ListingAuctionSessionOutcome {
        session.determine_outcome()
    }
}

pub fn default_close_strategy() -> Box<dyn CloseStrategy> {
    Box::new(EnglishReserveClose)
}
