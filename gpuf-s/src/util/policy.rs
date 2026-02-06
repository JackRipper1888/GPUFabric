#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessLevel(pub i32);

impl AccessLevel {
    pub const METERED: Self = Self(-1);

    pub fn is_metered(self) -> bool {
        self.0 == Self::METERED.0
    }
}

impl From<i32> for AccessLevel {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<AccessLevel> for i32 {
    fn from(value: AccessLevel) -> Self {
        value.0
    }
}

pub const REQUEST_MESSAGE_TOPIC: &str = "request-message";
