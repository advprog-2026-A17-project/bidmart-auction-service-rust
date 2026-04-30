use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinHandle;

use crate::service::auction_service::{AuctionService, CloseAuctionError, ListPendingClosureError};

#[derive(Debug, Clone)]
pub struct AuctionClosureScheduler {
    auction_service: AuctionService,
}

impl AuctionClosureScheduler {
    pub fn new(auction_service: AuctionService) -> Self {
        Self { auction_service }
    }

    pub async fn close_pending(&self) -> Result<AuctionClosureReport, AuctionClosureSchedulerError> {
        let auctions = self
            .auction_service
            .list_pending_closure()
            .await
            .map_err(AuctionClosureSchedulerError::from)?;
        let mut report = AuctionClosureReport {
            attempted: auctions.len(),
            closed: 0,
            failed: 0,
        };

        for auction in auctions {
            match self.auction_service.close_auction(&auction.id).await {
                Ok(_) => report.closed += 1,
                Err(_) => report.failed += 1,
            }
        }

        Ok(report)
    }

    pub fn spawn_polling(&self, interval: Duration) -> JoinHandle<()> {
        let scheduler = self.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;
                let _ = scheduler.close_pending().await;
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuctionClosureReport {
    pub attempted: usize,
    pub closed: usize,
    pub failed: usize,
}

#[derive(Debug, Error)]
pub enum AuctionClosureSchedulerError {
    #[error("List pending closure error: {0}")]
    ListPendingClosure(#[from] ListPendingClosureError),
    #[error("Close auction error: {0}")]
    CloseAuction(#[from] CloseAuctionError),
}
