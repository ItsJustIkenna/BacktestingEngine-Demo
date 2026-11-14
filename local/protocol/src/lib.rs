//! Fresh in-repo protocol types for standalone demo builds.

pub mod broker {
    pub mod messages;
}
pub mod generated {
    pub use crate::broker::messages::{Wallet, WalletData};
}

pub use broker::messages::*;
