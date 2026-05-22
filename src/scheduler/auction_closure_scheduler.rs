use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinHandle;

use crate::service::auction_service::{
    AuctionService, CloseListingAuctionSessionError, ListPendingClosureError,
};

#[derive(Debug, Clone)]
pub struct AuctionClosureScheduler {
    auction_service: AuctionService,
}

impl AuctionClosureScheduler {
    pub fn new(auction_service: AuctionService) -> Self {
        Self { auction_service }
    }

    pub async fn close_pending(
        &self,
    ) -> Result<AuctionClosureReport, AuctionClosureSchedulerError> {
        let mut report = AuctionClosureReport {
            attempted: 0,
            closed: 0,
            failed: 0,
        };
        let limit = crate::config::resolve_auction_closure_batch_size() as usize;

        while report.attempted < limit {
            match self.auction_service.process_one_pending_closure().await {
                Ok(Some(_)) => {
                    report.attempted += 1;
                    report.closed += 1;
                }
                Ok(None) => break,
                Err(error) => {
                    eprintln!(
                        "auction closure failed (wallet/convert/escrow may need retry): {error}"
                    );
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
                sleep_jitter().await;
                let _ = scheduler.close_pending().await;
            }
        })
    }
}

async fn sleep_jitter() {
    let jitter_ms = crate::config::resolve_scheduler_jitter_ms();
    if jitter_ms == 0 {
        return;
    }
    let now = chrono::Utc::now().timestamp_subsec_millis() as u64;
    let delay = now % jitter_ms;
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
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
