use bidmart_auction_service_rust::service::auction_service::CreateAuctionError;
use bidmart_auction_service_rust::service::auction_strategy::{
    AuctionStrategy, AuctionStrategyRegistry, AuctionType, resolve_strategy,
};

#[test]
fn parse_english_type_default() {
    let result = AuctionType::from_input(None);
    assert_eq!(result.unwrap(), AuctionType::English);
}

#[test]
fn parse_english_type_explicit() {
    assert_eq!(
        AuctionType::from_input(Some("ENGLISH")).unwrap(),
        AuctionType::English
    );
}

#[test]
fn parse_english_type_lowercase() {
    assert_eq!(
        AuctionType::from_input(Some("english")).unwrap(),
        AuctionType::English
    );
}

#[test]
fn parse_english_type_with_whitespace() {
    assert_eq!(
        AuctionType::from_input(Some("  English  ")).unwrap(),
        AuctionType::English
    );
}

#[test]
fn parse_scholarship_type() {
    assert_eq!(
        AuctionType::from_input(Some("SCHOLARSHIP")).unwrap(),
        AuctionType::Scholarship
    );
}

#[test]
fn parse_multi_slot_type() {
    assert_eq!(
        AuctionType::from_input(Some("MULTI_SLOT")).unwrap(),
        AuctionType::MultiSlotRegional
    );
}

#[test]
fn parse_multi_slot_regional_type() {
    assert_eq!(
        AuctionType::from_input(Some("MULTI_SLOT_REGIONAL")).unwrap(),
        AuctionType::MultiSlotRegional
    );
}

#[test]
fn parse_enterprise_type() {
    assert_eq!(
        AuctionType::from_input(Some("ENTERPRISE")).unwrap(),
        AuctionType::Enterprise
    );
}

#[test]
fn parse_unknown_type_returns_error() {
    let result = AuctionType::from_input(Some("DUTCH"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unsupported auction type"));
}

#[test]
fn storage_value_english() {
    assert_eq!(AuctionType::English.as_storage_value(), "ENGLISH");
}

#[test]
fn storage_value_scholarship() {
    assert_eq!(AuctionType::Scholarship.as_storage_value(), "SCHOLARSHIP");
}

#[test]
fn storage_value_multi_slot() {
    assert_eq!(
        AuctionType::MultiSlotRegional.as_storage_value(),
        "MULTI_SLOT_REGIONAL"
    );
}

#[test]
fn storage_value_enterprise() {
    assert_eq!(AuctionType::Enterprise.as_storage_value(), "ENTERPRISE");
}

#[test]
fn english_strategy_validates_successfully() {
    let strategy = resolve_strategy(AuctionType::English);
    assert!(strategy.validate_create_request().is_ok());
}

#[test]
fn scholarship_strategy_returns_unsupported_error() {
    let strategy = resolve_strategy(AuctionType::Scholarship);
    let err = strategy.validate_create_request().unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("SCHOLARSHIP"));
    assert!(message.contains("not enabled"));
}

#[test]
fn multi_slot_strategy_returns_unsupported_error() {
    let strategy = resolve_strategy(AuctionType::MultiSlotRegional);
    let err = strategy.validate_create_request().unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("MULTI_SLOT_REGIONAL"));
}

#[test]
fn enterprise_strategy_returns_unsupported_error() {
    let strategy = resolve_strategy(AuctionType::Enterprise);
    let err = strategy.validate_create_request().unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("ENTERPRISE"));
}

struct MbgStubStrategy;

impl AuctionStrategy for MbgStubStrategy {
    fn validate_create_request(&self) -> Result<(), CreateAuctionError> {
        Ok(())
    }
}

fn mbg_stub_factory() -> Box<dyn AuctionStrategy> {
    Box::new(MbgStubStrategy)
}

#[test]
fn registry_can_add_mbg_strategy_without_changing_core_resolver() {
    let mut registry = AuctionStrategyRegistry::default_registry();
    registry.register(AuctionType::MultiSlotRegional, mbg_stub_factory);

    assert!(
        registry
            .resolve(AuctionType::MultiSlotRegional)
            .validate_create_request()
            .is_ok()
    );
    assert!(
        resolve_strategy(AuctionType::MultiSlotRegional)
            .validate_create_request()
            .is_err()
    );
}
