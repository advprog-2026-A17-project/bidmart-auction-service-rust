use std::ops::{Add, AddAssign};

use thiserror::Error;

const ANTI_SNIPING_WINDOW_SECS: u64 = 120;
const ANTI_SNIPING_EXTENSION_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(u64);

impl Money {
    pub fn from_cents(value: u64) -> Self {
        Self(value)
    }

    pub fn cents(self) -> u64 {
        self.0
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixSeconds(u64);

impl UnixSeconds {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    pub fn add_secs(self, seconds: u64) -> Self {
        Self(self.0 + seconds)
    }

    fn seconds_until(self, later: UnixSeconds) -> u64 {
        later.0.saturating_sub(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListingAuctionSessionId(String);

impl ListingAuctionSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatalogueListingId(String);

impl CatalogueListingId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(String);

impl UserId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bid {
    pub bidder_id: UserId,
    pub amount: Money,
    pub placed_at: UnixSeconds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingAuctionSessionStatus {
    Draft,
    Active,
    Extended,
    Closed,
    Won,
    Unsold,
    Cancelled,
}

pub trait AuctionLifecycleState: Sync {
    fn status(&self) -> ListingAuctionSessionStatus;

    fn ensure_can_bid(&self) -> Result<(), BidError> {
        Err(BidError::AuctionNotActive {
            status: self.status(),
        })
    }

    fn status_after_extension(&self) -> ListingAuctionSessionStatus {
        self.status()
    }
}

struct DraftState;
struct ActiveState;
struct ExtendedState;
struct ClosedState;
struct WonState;
struct UnsoldState;
struct CancelledState;

impl AuctionLifecycleState for DraftState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Draft
    }
}

impl AuctionLifecycleState for ActiveState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Active
    }

    fn ensure_can_bid(&self) -> Result<(), BidError> {
        Ok(())
    }

    fn status_after_extension(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Extended
    }
}

impl AuctionLifecycleState for ExtendedState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Extended
    }

    fn ensure_can_bid(&self) -> Result<(), BidError> {
        Ok(())
    }

    fn status_after_extension(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Extended
    }
}

impl AuctionLifecycleState for ClosedState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Closed
    }
}

impl AuctionLifecycleState for WonState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Won
    }
}

impl AuctionLifecycleState for UnsoldState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Unsold
    }
}

impl AuctionLifecycleState for CancelledState {
    fn status(&self) -> ListingAuctionSessionStatus {
        ListingAuctionSessionStatus::Cancelled
    }
}

static DRAFT_STATE: DraftState = DraftState;
static ACTIVE_STATE: ActiveState = ActiveState;
static EXTENDED_STATE: ExtendedState = ExtendedState;
static CLOSED_STATE: ClosedState = ClosedState;
static WON_STATE: WonState = WonState;
static UNSOLD_STATE: UnsoldState = UnsoldState;
static CANCELLED_STATE: CancelledState = CancelledState;

pub fn lifecycle_state(status: ListingAuctionSessionStatus) -> &'static dyn AuctionLifecycleState {
    match status {
        ListingAuctionSessionStatus::Draft => &DRAFT_STATE,
        ListingAuctionSessionStatus::Active => &ACTIVE_STATE,
        ListingAuctionSessionStatus::Extended => &EXTENDED_STATE,
        ListingAuctionSessionStatus::Closed => &CLOSED_STATE,
        ListingAuctionSessionStatus::Won => &WON_STATE,
        ListingAuctionSessionStatus::Unsold => &UNSOLD_STATE,
        ListingAuctionSessionStatus::Cancelled => &CANCELLED_STATE,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingAuctionSessionOutcome {
    Won,    // Reserve met and has winner
    Unsold, // Reserve not met or no bids
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ListingAuctionSessionStateError {
    #[error("auction cannot start before start time")]
    TooEarly { start_at: UnixSeconds },
    #[error("auction already ended")]
    AlreadyEnded { end_at: UnixSeconds },
    #[error("auction is cancelled")]
    Cancelled,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum BidError {
    #[error("auction is not active")]
    AuctionNotActive { status: ListingAuctionSessionStatus },
    #[error("auction has not started yet")]
    AuctionNotStarted { start_at: UnixSeconds },
    #[error("auction already ended")]
    AuctionEnded { end_at: UnixSeconds },
    #[error("bid amount below minimum required")]
    BidTooLow { minimum: Money },
    #[error("self bidding is not allowed")]
    SelfBiddingNotAllowed { bidder_id: UserId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidAccepted {
    pub new_highest: Bid,
    pub previous_highest: Option<Bid>,
    pub extended: bool,
    pub new_end_at: UnixSeconds,
}

#[derive(Debug, Clone)]
pub struct ListingAuctionSession {
    id: ListingAuctionSessionId,
    listing_id: CatalogueListingId,
    seller_id: UserId,
    starting_price: Money,
    minimum_increment: Money,
    reserve_price: Money,
    start_at: UnixSeconds,
    end_at: UnixSeconds,
    status: ListingAuctionSessionStatus,
    current_highest: Option<Bid>,
    extensions: u32,
}

impl ListingAuctionSession {
    pub fn new(
        id: impl Into<String>,
        listing_id: impl Into<String>,
        seller_id: impl Into<String>,
        starting_price: Money,
        minimum_increment: Money,
        reserve_price: Money,
        start_at: UnixSeconds,
        end_at: UnixSeconds,
    ) -> Self {
        Self {
            id: ListingAuctionSessionId::new(id),
            listing_id: CatalogueListingId::new(listing_id),
            seller_id: UserId::new(seller_id),
            starting_price,
            minimum_increment,
            reserve_price,
            start_at,
            end_at,
            status: ListingAuctionSessionStatus::Draft,
            current_highest: None,
            extensions: 0,
        }
    }

    pub fn with_status(
        id: impl Into<String>,
        listing_id: impl Into<String>,
        seller_id: impl Into<String>,
        starting_price: Money,
        minimum_increment: Money,
        reserve_price: Money,
        start_at: UnixSeconds,
        end_at: UnixSeconds,
        status: ListingAuctionSessionStatus,
        current_highest: Option<Bid>,
    ) -> Self {
        Self {
            id: ListingAuctionSessionId::new(id),
            listing_id: CatalogueListingId::new(listing_id),
            seller_id: UserId::new(seller_id),
            starting_price,
            minimum_increment,
            reserve_price,
            start_at,
            end_at,
            status,
            current_highest,
            extensions: 0,
        }
    }

    pub fn activate(&mut self, now: UnixSeconds) -> Result<(), ListingAuctionSessionStateError> {
        if self.status == ListingAuctionSessionStatus::Cancelled {
            return Err(ListingAuctionSessionStateError::Cancelled);
        }

        if now < self.start_at {
            return Err(ListingAuctionSessionStateError::TooEarly {
                start_at: self.start_at,
            });
        }

        if self.status == ListingAuctionSessionStatus::Closed
            || self.status == ListingAuctionSessionStatus::Won
            || self.status == ListingAuctionSessionStatus::Unsold
        {
            return Err(ListingAuctionSessionStateError::AlreadyEnded {
                end_at: self.end_at,
            });
        }

        if now >= self.end_at {
            self.status = ListingAuctionSessionStatus::Closed;
            return Err(ListingAuctionSessionStateError::AlreadyEnded {
                end_at: self.end_at,
            });
        }

        if self.status == ListingAuctionSessionStatus::Draft {
            self.status = ListingAuctionSessionStatus::Active;
        }

        Ok(())
    }

    pub fn cancel(&mut self) {
        self.status = ListingAuctionSessionStatus::Cancelled;
    }

    pub fn status(&self) -> ListingAuctionSessionStatus {
        self.status
    }

    pub fn end_at(&self) -> UnixSeconds {
        self.end_at
    }

    pub fn current_highest(&self) -> Option<Bid> {
        self.current_highest.clone()
    }

    pub fn seller_id(&self) -> &UserId {
        &self.seller_id
    }

    pub fn starting_price(&self) -> Money {
        self.starting_price
    }

    pub fn reserve_price(&self) -> Money {
        self.reserve_price
    }

    pub fn minimum_increment(&self) -> Money {
        self.minimum_increment
    }

    pub fn place_bid(
        &mut self,
        bidder_id: UserId,
        amount: Money,
        now: UnixSeconds,
    ) -> Result<BidAccepted, BidError> {
        self.ensure_accepting_bids(now)?;

        // Seller cannot bid on their own auction
        if bidder_id == self.seller_id {
            return Err(BidError::SelfBiddingNotAllowed { bidder_id });
        }

        if let Some(current) = &self.current_highest {
            if current.bidder_id == bidder_id {
                return Err(BidError::SelfBiddingNotAllowed { bidder_id });
            }
        }

        let minimum_required = self.minimum_required_bid();
        if amount < minimum_required {
            return Err(BidError::BidTooLow {
                minimum: minimum_required,
            });
        }

        let previous = self.current_highest.take();
        let new_bid = Bid {
            bidder_id,
            amount,
            placed_at: now,
        };
        self.current_highest = Some(new_bid.clone());

        let extended = self.maybe_extend(now);

        Ok(BidAccepted {
            new_highest: new_bid,
            previous_highest: previous,
            extended,
            new_end_at: self.end_at,
        })
    }

    pub fn place_proxy_bid(
        &mut self,
        bidder_id: UserId,
        max_amount: Money,
        now: UnixSeconds,
    ) -> Result<BidAccepted, BidError> {
        self.ensure_accepting_bids(now)?;

        if bidder_id == self.seller_id {
            return Err(BidError::SelfBiddingNotAllowed { bidder_id });
        }

        if let Some(current) = &self.current_highest {
            if current.bidder_id == bidder_id {
                if max_amount < current.amount {
                    return Err(BidError::BidTooLow {
                        minimum: current.amount,
                    });
                }

                return Ok(BidAccepted {
                    new_highest: current.clone(),
                    previous_highest: Some(current.clone()),
                    extended: false,
                    new_end_at: self.end_at,
                });
            }
        }

        let minimum_required = self.minimum_required_bid();
        if max_amount < minimum_required {
            return Err(BidError::BidTooLow {
                minimum: minimum_required,
            });
        }

        self.place_bid(bidder_id, minimum_required, now)
    }

    fn minimum_required_bid(&self) -> Money {
        match &self.current_highest {
            Some(bid) => bid.amount + self.minimum_increment,
            None => self.starting_price,
        }
    }

    fn ensure_accepting_bids(&mut self, now: UnixSeconds) -> Result<(), BidError> {
        lifecycle_state(self.status).ensure_can_bid()?;

        if now < self.start_at {
            return Err(BidError::AuctionNotStarted {
                start_at: self.start_at,
            });
        }

        if now >= self.end_at {
            self.status = ListingAuctionSessionStatus::Closed;
            return Err(BidError::AuctionEnded {
                end_at: self.end_at,
            });
        }

        Ok(())
    }

    /// Anti-sniping: extends auction by 2 minutes if bid is within last 2 minutes.
    /// Per spec: "tanpa batas maksimum perpanjangan" — no extension limit.
    fn maybe_extend(&mut self, now: UnixSeconds) -> bool {
        let remaining = now.seconds_until(self.end_at);
        if remaining <= ANTI_SNIPING_WINDOW_SECS {
            self.end_at = now.add_secs(ANTI_SNIPING_EXTENSION_SECS);
            self.extensions += 1;
            self.status = lifecycle_state(self.status).status_after_extension();
            return true;
        }
        false
    }

    /// Close the auction and transition to CLOSED state.
    /// Call determine_outcome() after to get WON/UNSOLD.
    pub fn close(&mut self) {
        self.status = ListingAuctionSessionStatus::Closed;
    }

    pub fn determine_outcome(&mut self) -> ListingAuctionSessionOutcome {
        self.status = ListingAuctionSessionStatus::Closed;
        match &self.current_highest {
            Some(bid) if bid.amount >= self.reserve_price => {
                self.status = ListingAuctionSessionStatus::Won;
                ListingAuctionSessionOutcome::Won
            }
            _ => {
                self.status = ListingAuctionSessionStatus::Unsold;
                ListingAuctionSessionOutcome::Unsold
            }
        }
    }

    pub fn extensions(&self) -> u32 {
        self.extensions
    }

    pub fn id(&self) -> &str {
        &self.id.0
    }

    pub fn listing_id(&self) -> &str {
        &self.listing_id.0
    }
}
