use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord, ProxyBidRecord};
use crate::service::auction_service::{
    AuctionService, BidCursorPage, CloseListingAuctionSessionError, CreateAuctionCommand,
    CreateAuctionError, GetListingAuctionSessionError, ListBidsError,
    ListListingAuctionSessionsError, ListPendingClosureError, PlaceBidError,
};

pub struct CreateAuctionHandler<'a> {
    service: &'a AuctionService,
}

impl<'a> CreateAuctionHandler<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn execute(
        &self,
        command: CreateAuctionCommand,
    ) -> Result<ListingAuctionSessionRecord, CreateAuctionError> {
        self.service.create_auction_core(command).await
    }
}

pub struct CloseAuctionHandler<'a> {
    service: &'a AuctionService,
}

impl<'a> CloseAuctionHandler<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn close(
        &self,
        auction_id: &str,
    ) -> Result<ListingAuctionSessionRecord, CloseListingAuctionSessionError> {
        self.service.close_auction_core(auction_id).await
    }

    pub async fn process_one_pending(
        &self,
    ) -> Result<Option<ListingAuctionSessionRecord>, CloseListingAuctionSessionError> {
        self.service.process_one_pending_closure_core().await
    }
}

pub struct PlaceBidHandler<'a> {
    service: &'a AuctionService,
}

impl<'a> PlaceBidHandler<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn execute(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<BidRecord, PlaceBidError> {
        self.service
            .place_bid_core(auction_id, bidder_id, bid_amount_cents, bid_time)
            .await
    }
}

pub struct PlaceProxyBidHandler<'a> {
    service: &'a AuctionService,
}

impl<'a> PlaceProxyBidHandler<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn execute(
        &self,
        auction_id: &str,
        bidder_id: &str,
        max_bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<BidRecord, PlaceBidError> {
        self.service
            .place_proxy_bid_core(auction_id, bidder_id, max_bid_amount_cents, bid_time)
            .await
    }
}

pub struct ListAuctionQueryService<'a> {
    service: &'a AuctionService,
}

impl<'a> ListAuctionQueryService<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn get(
        &self,
        auction_id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, GetListingAuctionSessionError> {
        self.service.get_auction_by_id_core(auction_id).await
    }

    pub async fn list(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListListingAuctionSessionsError> {
        self.service.list_auctions_core().await
    }

    pub async fn list_pending_closure(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListPendingClosureError> {
        self.service.list_pending_closure_core().await
    }
}

pub struct ListBidQueryService<'a> {
    service: &'a AuctionService,
}

impl<'a> ListBidQueryService<'a> {
    pub fn new(service: &'a AuctionService) -> Self {
        Self { service }
    }

    pub async fn list(&self, auction_id: &str) -> Result<Vec<BidRecord>, ListBidsError> {
        self.service.list_bids_core(auction_id).await
    }

    pub async fn list_cursor(
        &self,
        auction_id: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<BidCursorPage, ListBidsError> {
        self.service
            .list_bids_with_cursor_core(auction_id, cursor, limit)
            .await
    }

    pub async fn get_proxy_bid(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<Option<ProxyBidRecord>, ListBidsError> {
        self.service.get_proxy_bid_core(auction_id, bidder_id).await
    }

    pub async fn delete_proxy_bid(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<(), ListBidsError> {
        self.service
            .delete_proxy_bid_core(auction_id, bidder_id)
            .await
    }
}
