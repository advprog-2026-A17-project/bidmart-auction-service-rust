use std::sync::Arc;

use crate::service::auction_core::AuctionService;
use tokio::sync::OwnedMutexGuard;

impl AuctionService {
    pub(super) async fn auction_bid_guard(&self, auction_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut bid_locks = self.bid_locks.lock().expect("bid lock map poisoned");
            bid_locks
                .entry(auction_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };

        lock.lock_owned().await
    }
}
