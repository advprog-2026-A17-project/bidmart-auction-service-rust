use crate::service::auction_service::CreateAuctionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

type StrategyFactory = fn() -> Box<dyn AuctionStrategy>;

pub struct AuctionStrategyRegistry {
    factories: std::collections::HashMap<AuctionType, StrategyFactory>,
}

impl AuctionStrategyRegistry {
    pub fn default_registry() -> Self {
        let mut registry = Self {
            factories: std::collections::HashMap::new(),
        };
        registry.register(AuctionType::English, english_strategy_factory);
        registry.register(AuctionType::Scholarship, scholarship_strategy_factory);
        registry.register(AuctionType::MultiSlotRegional, multi_slot_strategy_factory);
        registry.register(AuctionType::Enterprise, enterprise_strategy_factory);
        registry
    }

    pub fn register(&mut self, auction_type: AuctionType, factory: StrategyFactory) {
        self.factories.insert(auction_type, factory);
    }

    pub fn resolve(&self, auction_type: AuctionType) -> Box<dyn AuctionStrategy> {
        let factory = self
            .factories
            .get(&auction_type)
            .copied()
            .unwrap_or(unsupported_strategy_factory);
        factory()
    }
}

pub fn resolve_strategy(auction_type: AuctionType) -> Box<dyn AuctionStrategy> {
    AuctionStrategyRegistry::default_registry().resolve(auction_type)
}

pub trait AuctionWorkflow: Send + Sync {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError>;
}

pub struct AuctionWorkflowFactory;

impl AuctionWorkflowFactory {
    pub fn create(auction_type: AuctionType) -> Box<dyn AuctionWorkflow> {
        match auction_type {
            AuctionType::English => Box::new(EnglishAuctionWorkflow),
            AuctionType::Scholarship => Box::new(DisabledAuctionWorkflow { auction_type }),
            AuctionType::MultiSlotRegional => Box::new(DisabledAuctionWorkflow { auction_type }),
            AuctionType::Enterprise => Box::new(DisabledAuctionWorkflow { auction_type }),
        }
    }
}

struct EnglishAuctionWorkflow;

impl AuctionWorkflow for EnglishAuctionWorkflow {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError> {
        Ok(())
    }
}

struct DisabledAuctionWorkflow {
    auction_type: AuctionType,
}

impl AuctionWorkflow for DisabledAuctionWorkflow {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError> {
        Err(CreateAuctionError::InvalidInput(format!(
            "Auction type {} is recognized but not enabled yet",
            self.auction_type.as_storage_value()
        )))
    }
}

fn english_strategy_factory() -> Box<dyn AuctionStrategy> {
    Box::new(EnglishAuctionStrategy)
}

fn scholarship_strategy_factory() -> Box<dyn AuctionStrategy> {
    Box::new(UnsupportedAuctionStrategy {
        auction_type: AuctionType::Scholarship,
    })
}

fn multi_slot_strategy_factory() -> Box<dyn AuctionStrategy> {
    Box::new(UnsupportedAuctionStrategy {
        auction_type: AuctionType::MultiSlotRegional,
    })
}

fn enterprise_strategy_factory() -> Box<dyn AuctionStrategy> {
    Box::new(UnsupportedAuctionStrategy {
        auction_type: AuctionType::Enterprise,
    })
}

fn unsupported_strategy_factory() -> Box<dyn AuctionStrategy> {
    Box::new(UnsupportedAuctionStrategy {
        auction_type: AuctionType::English,
    })
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
