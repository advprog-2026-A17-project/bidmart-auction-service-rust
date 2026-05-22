use std::sync::Arc;

use bidmart_auction_service_rust::client::{CatalogClient, CatalogClientError, ListingSummary};

/// Test double: every listing is ACTIVE so integration tests can run without a catalogue container.
pub struct AlwaysActiveCatalog;

#[async_trait::async_trait]
impl CatalogClient for AlwaysActiveCatalog {
    async fn get_listing_summary(
        &self,
        listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError> {
        Ok(ListingSummary {
            id: listing_id.to_string(),
            seller_id: String::new(),
            status: "ACTIVE".to_string(),
        })
    }
}

pub fn always_active_catalog() -> Arc<dyn CatalogClient> {
    Arc::new(AlwaysActiveCatalog)
}
