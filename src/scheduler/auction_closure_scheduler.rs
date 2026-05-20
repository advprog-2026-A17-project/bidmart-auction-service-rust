use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinHandle;

use crate::service::auction_service::{AuctionService, CloseListingAuctionSessionError, ListPendingClosureError};

#[derive(Debug, Clone)]
pub struct AuctionClosureScheduler {
    auction_service: AuctionService,
}

impl AuctionClosureScheduler {
    pub fn new(auction_service: AuctionService) -> Self {
        Self { auction_service }
    }

    pub async fn close_pending(&self) -> Result<AuctionClosureReport, AuctionClosureSchedulerError> {
        let mut report = AuctionClosureReport {
            attempted: 0,
            closed: 0,
            failed: 0,
        };

        loop {
            match self.auction_service.process_one_pending_closure().await {
                Ok(Some(_)) => {
                    report.attempted += 1;
                    report.closed += 1;
                }
                Ok(None) => break,
                Err(_) => {
                    report.attempted += 1;
                    report.failed += 1;
                }
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
    CloseAuction(#[from] CloseListingAuctionSessionError),
}
