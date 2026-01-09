use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeFilter {
    OneHour,
    FourHours,
    TwelveHours,
    TwentyFourHours,
    SevenDays,
}

impl TimeFilter {
    pub fn seconds(&self) -> u64 {
        match self {
            Self::OneHour => 3600,
            Self::FourHours => 14400,
            Self::TwelveHours => 43200,
            Self::TwentyFourHours => 86400,
            Self::SevenDays => 604800,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::OneHour => "1h",
            Self::FourHours => "4h",
            Self::TwelveHours => "12h",
            Self::TwentyFourHours => "24h",
            Self::SevenDays => "7d",
        }
    }

    pub fn cycle_next(current: Option<Self>) -> Option<Self> {
        match current {
            None => Some(Self::OneHour),
            Some(Self::OneHour) => Some(Self::FourHours),
            Some(Self::FourHours) => Some(Self::TwelveHours),
            Some(Self::TwelveHours) => Some(Self::TwentyFourHours),
            Some(Self::TwentyFourHours) => Some(Self::SevenDays),
            Some(Self::SevenDays) => None,
        }
    }
}
