use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord, ProxyBidRecord};
use crate::service::auction_commands::{ListAuctionQueryService, ListBidQueryService};
use crate::service::auction_core::AuctionService;
use crate::service::auction_types::{
    BidCursorPage, GetListingAuctionSessionError, ListBidsError, ListListingAuctionSessionsError,
    ListPendingClosureError, bid_cursor_from_bid, parse_bid_cursor,
};

impl AuctionService {
    pub async fn get_auction_by_id(
        &self,
        auction_id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, GetListingAuctionSessionError> {
        ListAuctionQueryService::new(self).get(auction_id).await
    }

    pub(crate) async fn get_auction_by_id_core(
        &self,
        auction_id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, GetListingAuctionSessionError> {
        let found = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| GetListingAuctionSessionError::DatabaseError(error.to_string()))?;
        if found.is_some() {
            return Ok(found);
        }
        self.listing_auction_session_repo
            .find_by_listing_id(auction_id)
            .await
            .map_err(|error| GetListingAuctionSessionError::DatabaseError(error.to_string()))
    }

    pub async fn list_auctions(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListListingAuctionSessionsError> {
        ListAuctionQueryService::new(self).list().await
    }

    pub(crate) async fn list_auctions_core(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListListingAuctionSessionsError> {
        self.listing_auction_session_repo
            .list_all()
            .await
            .map_err(|error| ListListingAuctionSessionsError::DatabaseError(error.to_string()))
    }

    pub async fn list_pending_closure(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListPendingClosureError> {
        ListAuctionQueryService::new(self)
            .list_pending_closure()
            .await
    }

    pub(crate) async fn list_pending_closure_core(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListPendingClosureError> {
        let now = chrono::Utc::now().timestamp();
        self.listing_auction_session_repo
            .list_pending_closure(now)
            .await
            .map_err(|error| ListPendingClosureError::DatabaseError(error.to_string()))
    }

    pub async fn list_bids(&self, auction_id: &str) -> Result<Vec<BidRecord>, ListBidsError> {
        ListBidQueryService::new(self).list(auction_id).await
    }

    pub(crate) async fn list_bids_core(
        &self,
        auction_id: &str,
    ) -> Result<Vec<BidRecord>, ListBidsError> {
        let canonical_auction_id = self.canonical_auction_id(auction_id).await?;
        self.bid_repo
            .list_by_auction_id_desc(&canonical_auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))
    }

    pub async fn get_proxy_bid(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<Option<ProxyBidRecord>, ListBidsError> {
        ListBidQueryService::new(self)
            .get_proxy_bid(auction_id, bidder_id)
            .await
    }

    pub(crate) async fn get_proxy_bid_core(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<Option<ProxyBidRecord>, ListBidsError> {
        let canonical_auction_id = self.canonical_auction_id(auction_id).await?;
        self.proxy_bid_repo
            .find_by_bidder(&canonical_auction_id, bidder_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))
    }

    pub async fn delete_proxy_bid(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<(), ListBidsError> {
        ListBidQueryService::new(self)
            .delete_proxy_bid(auction_id, bidder_id)
            .await
    }

    pub(crate) async fn delete_proxy_bid_core(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<(), ListBidsError> {
        let canonical_auction_id = self.canonical_auction_id(auction_id).await?;
        self.proxy_bid_repo
            .delete_for_bidder(&canonical_auction_id, bidder_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?;
        Ok(())
    }

    pub async fn list_bids_with_cursor(
        &self,
        auction_id: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<BidCursorPage, ListBidsError> {
        ListBidQueryService::new(self)
            .list_cursor(auction_id, cursor, limit)
            .await
    }

    pub(crate) async fn list_bids_with_cursor_core(
        &self,
        auction_id: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<BidCursorPage, ListBidsError> {
        let canonical_auction_id = self.canonical_auction_id(auction_id).await?;
        let sanitized_limit = limit.unwrap_or(20).clamp(1, 100);
        let parsed_cursor = match cursor {
            Some(value) => Some(parse_bid_cursor(value).map_err(ListBidsError::InvalidInput)?),
            None => None,
        };

        let mut bids = self
            .bid_repo
            .list_by_auction_cursor(
                &canonical_auction_id,
                parsed_cursor.map(|cursor| (cursor.amount_cents, cursor.bid_time, cursor.id)),
                sanitized_limit + 1,
            )
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?;

        let has_more = bids.len() as i64 > sanitized_limit;
        if has_more {
            bids.truncate(sanitized_limit as usize);
        }

        let next_cursor = if has_more {
            bids.last().map(|bid| bid_cursor_from_bid(bid).to_string())
        } else {
            None
        };

        Ok(BidCursorPage {
            items: bids,
            next_cursor,
            size: sanitized_limit,
        })
    }

    pub async fn get_auction_with_bids(
        &self,
        auction_id: &str,
    ) -> Result<Option<(String, Vec<String>)>, sqlx::Error> {
        match self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await?
            .or(self
                .listing_auction_session_repo
                .find_by_listing_id(auction_id)
                .await?)
        {
            Some(auction) => {
                let bids = self.bid_repo.list_by_auction_id_desc(&auction.id).await?;
                let bid_ids: Vec<String> = bids.iter().map(|b| b.id.clone()).collect();
                Ok(Some((auction.id, bid_ids)))
            }
            None => Ok(None),
        }
    }

    async fn canonical_auction_id(&self, auction_id: &str) -> Result<String, ListBidsError> {
        let record = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?
            .or(self
                .listing_auction_session_repo
                .find_by_listing_id(auction_id)
                .await
                .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?);
        Ok(record
            .map(|record| record.id)
            .unwrap_or_else(|| auction_id.to_string()))
    }
}
