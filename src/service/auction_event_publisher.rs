use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord, NewOutboxEventRecord};
use crate::service::auction_core::AuctionService;

impl AuctionService {
    pub(super) async fn publish_bid_placed_event(
        &self,
        auction: &ListingAuctionSessionRecord,
        bid: &BidRecord,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "bidId": bid.id,
            "bidderId": bid.bidder_id,
            "amountCents": bid.bid_amount_cents,
            "currentPrice": bid.bid_amount_cents,
            "status": auction.status,
            "endTime": auction.end_time,
            "bidTime": bid.bid_time,
            "placedAt": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "BidPlaced".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    pub(super) async fn publish_outbid_event(
        &self,
        auction: &ListingAuctionSessionRecord,
        previous_bid: &BidRecord,
        new_amount_cents: i64,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "previousBidderId": previous_bid.bidder_id,
            "amountCents": new_amount_cents,
            "currentPrice": new_amount_cents,
            "outbidAt": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "Outbid".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    pub(super) async fn publish_auction_created_event(
        &self,
        auction: &ListingAuctionSessionRecord,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "startingPrice": auction.starting_price_cents,
            "reservePrice": auction.reserve_price_cents,
            "minimumIncrement": auction.minimum_increment_cents,
            "status": auction.status,
            "startTime": auction.start_time,
            "endTime": auction.end_time,
            "createdAt": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "AuctionCreated".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    pub(super) async fn publish_auction_ended_event_with_tx(
        &self,
        auction: &ListingAuctionSessionRecord,
        winning_bid: Option<&BidRecord>,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = if auction.status == "WON" {
            serde_json::json!({
                "auctionId": auction.id,
                "listingId": auction.listing_id,
                "sellerId": auction.seller_id,
                "status": auction.status,
                "reserveMet": true,
                "winnerId": winning_bid.map(|bid| bid.bidder_id.as_str()),
                "finalPrice": winning_bid.map(|bid| bid.bid_amount_cents),
                "endedAt": now
            })
        } else {
            serde_json::json!({
                "auctionId": auction.id,
                "listingId": auction.listing_id,
                "sellerId": auction.seller_id,
                "status": auction.status,
                "reserveMet": false,
                "endedAt": now
            })
        }
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "AuctionEnded".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert_with_tx(&event, tx).await?;
        Ok(())
    }
}
