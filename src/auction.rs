use std::ops::{Add, AddAssign};

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
pub struct AuctionId(String);

impl AuctionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListingId(String);

impl ListingId {
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
pub enum AuctionStatus {
    Scheduled,
    Active,
    Extended,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BidError {
    AuctionNotStarted { start_at: UnixSeconds },
    AuctionEnded { end_at: UnixSeconds },
    BidTooLow { minimum: Money },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidAccepted {
    pub new_highest: Bid,
    pub previous_highest: Option<Bid>,
    pub extended: bool,
    pub new_end_at: UnixSeconds,
}

#[derive(Debug, Clone)]
pub struct Auction {
    id: AuctionId,
    listing_id: ListingId,
    seller_id: UserId,
    starting_price: Money,
    minimum_increment: Money,
    reserve_price: Money,
    start_at: UnixSeconds,
    end_at: UnixSeconds,
    status: AuctionStatus,
    current_highest: Option<Bid>,
    extensions: u32,
}

impl Auction {
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
            id: AuctionId::new(id),
            listing_id: ListingId::new(listing_id),
            seller_id: UserId::new(seller_id),
            starting_price,
            minimum_increment,
            reserve_price,
            start_at,
            end_at,
            status: AuctionStatus::Scheduled,
            current_highest: None,
            extensions: 0,
        }
    }

    pub fn status(&self) -> AuctionStatus {
        self.status
    }

    pub fn end_at(&self) -> UnixSeconds {
        self.end_at
    }

    pub fn current_highest(&self) -> Option<Bid> {
        self.current_highest.clone()
    }

    pub fn place_bid(
        &mut self,
        bidder_id: UserId,
        amount: Money,
        now: UnixSeconds,
    ) -> Result<BidAccepted, BidError> {
        if now < self.start_at {
            return Err(BidError::AuctionNotStarted {
                start_at: self.start_at,
            });
        }

        if now >= self.end_at {
            return Err(BidError::AuctionEnded { end_at: self.end_at });
        }

        if self.status == AuctionStatus::Scheduled {
            self.status = AuctionStatus::Active;
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

    fn minimum_required_bid(&self) -> Money {
        match &self.current_highest {
            Some(bid) => bid.amount + self.minimum_increment,
            None => self.starting_price,
        }
    }

    fn maybe_extend(&mut self, now: UnixSeconds) -> bool {
        let remaining = now.seconds_until(self.end_at);
        if remaining <= ANTI_SNIPING_WINDOW_SECS {
            self.end_at = self.end_at.add_secs(ANTI_SNIPING_EXTENSION_SECS);
            self.extensions += 1;
            self.status = AuctionStatus::Extended;
            return true;
        }
        false
    }
}
