use crate::persistence::models::{ListingAuctionSessionRecord, NewListingAuctionSessionRecord};
use crate::service::auction_commands::CreateAuctionHandler;
use crate::service::auction_core::AuctionService;
use crate::service::auction_strategy::{AuctionType, AuctionWorkflowFactory};
use crate::service::auction_types::{CreateAuctionCommand, CreateAuctionError, initial_status};

impl AuctionService {
    pub async fn create_auction(
        &self,
        command: CreateAuctionCommand,
    ) -> Result<ListingAuctionSessionRecord, CreateAuctionError> {
        CreateAuctionHandler::new(self).execute(command).await
    }

    pub(crate) async fn create_auction_core(
        &self,
        command: CreateAuctionCommand,
    ) -> Result<ListingAuctionSessionRecord, CreateAuctionError> {
        let now = chrono::Utc::now().timestamp();
        command.validate(now)?;
        let auction_type = AuctionType::from_input(Some(&command.auction_type))
            .map_err(CreateAuctionError::InvalidInput)?;
        AuctionWorkflowFactory::create(auction_type).validate_create_request()?;
        self.validate_listing_for_auction(&command).await?;

        if let Some(existing) = self
            .listing_auction_session_repo
            .find_by_listing_id(&command.listing_id)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?
        {
            return Ok(existing);
        }

        let listing_id = command.listing_id.clone();
        let auction = NewListingAuctionSessionRecord {
            id: listing_id.clone(),
            listing_id,
            seller_id: command.seller_id,
            starting_price_cents: command.starting_price_cents,
            reserve_price_cents: command.reserve_price_cents,
            current_highest_bid_cents: None,
            minimum_increment_cents: command.minimum_increment_cents,
            status: initial_status(command.start_time, now),
            start_time: command.start_time,
            end_time: command.end_time,
            created_at: now,
            updated_at: now,
        };

        let inserted = self
            .listing_auction_session_repo
            .insert(&auction)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?;
        self.closure_job_repo
            .upsert_pending(&inserted.id, inserted.end_time, now)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?;
        self.publish_auction_created_event(&inserted)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?;
        Ok(inserted)
    }

    async fn validate_listing_for_auction(
        &self,
        command: &CreateAuctionCommand,
    ) -> Result<(), CreateAuctionError> {
        let listing = self
            .require_active_listing(&command.listing_id)
            .await
            .map_err(CreateAuctionError::InvalidInput)?;

        if !listing.seller_id.is_empty() && listing.seller_id != command.seller_id {
            return Err(CreateAuctionError::InvalidInput(
                "Listing seller does not match auction seller".to_string(),
            ));
        }

        Ok(())
    }
}
