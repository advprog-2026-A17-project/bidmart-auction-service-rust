use crate::listing_auction_session::{ListingAuctionSession, ListingAuctionSessionOutcome};

/// Pluggable auction close outcome rules (Strategy pattern).
pub trait CloseStrategy: Send + Sync {
    fn determine_outcome(&self, session: &mut ListingAuctionSession) -> ListingAuctionSessionOutcome;
}

/// Default English auction with reserve: WON when highest bid >= reserve, else UNSOLD.
pub struct EnglishReserveClose;

impl CloseStrategy for EnglishReserveClose {
    fn determine_outcome(&self, session: &mut ListingAuctionSession) -> ListingAuctionSessionOutcome {
        session.determine_outcome()
    }
}

pub fn default_close_strategy() -> Box<dyn CloseStrategy> {
    Box::new(EnglishReserveClose)
}
