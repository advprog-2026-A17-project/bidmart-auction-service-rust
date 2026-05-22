use crate::service::auction_service::CreateAuctionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuctionType {
    English,
    Scholarship,
    MultiSlotRegional,
    Enterprise,
}

impl AuctionType {
    pub fn from_input(value: Option<&str>) -> Result<Self, String> {
        let normalized = value.unwrap_or("ENGLISH").trim().to_ascii_uppercase();
        match normalized.as_str() {
            "ENGLISH" => Ok(Self::English),
            "SCHOLARSHIP" => Ok(Self::Scholarship),
            "MULTI_SLOT" | "MULTI_SLOT_REGIONAL" => Ok(Self::MultiSlotRegional),
            "ENTERPRISE" => Ok(Self::Enterprise),
            _ => Err(format!(
                "Unsupported auction type: {normalized}. Supported types are ENGLISH, SCHOLARSHIP, MULTI_SLOT_REGIONAL, ENTERPRISE"
            )),
        }
    }

    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::English => "ENGLISH",
            Self::Scholarship => "SCHOLARSHIP",
            Self::MultiSlotRegional => "MULTI_SLOT_REGIONAL",
            Self::Enterprise => "ENTERPRISE",
        }
    }
}

pub trait AuctionStrategy: Send + Sync {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError>;
}

pub fn resolve_strategy(auction_type: AuctionType) -> Box<dyn AuctionStrategy> {
    match auction_type {
        AuctionType::English => Box::new(EnglishAuctionStrategy),
        AuctionType::Scholarship => Box::new(UnsupportedAuctionStrategy {
            auction_type: AuctionType::Scholarship,
        }),
        AuctionType::MultiSlotRegional => Box::new(UnsupportedAuctionStrategy {
            auction_type: AuctionType::MultiSlotRegional,
        }),
        AuctionType::Enterprise => Box::new(UnsupportedAuctionStrategy {
            auction_type: AuctionType::Enterprise,
        }),
    }
}

struct EnglishAuctionStrategy;

impl AuctionStrategy for EnglishAuctionStrategy {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError> {
        Ok(())
    }
}

struct UnsupportedAuctionStrategy {
    auction_type: AuctionType,
}

impl AuctionStrategy for UnsupportedAuctionStrategy {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError> {
        Err(CreateAuctionError::InvalidInput(format!(
            "Auction type {} is recognized but not enabled yet",
            self.auction_type.as_storage_value()
        )))
    }
}
