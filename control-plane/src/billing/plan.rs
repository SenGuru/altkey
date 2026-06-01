//! The three plans + mapping a Polar product id back to a plan (via config).
use crate::config::Config;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Founding,
    Standard,
    Pro,
}

impl Plan {
    pub fn as_str(self) -> &'static str {
        match self {
            Plan::Founding => "founding",
            Plan::Standard => "standard",
            Plan::Pro => "pro",
        }
    }
    pub fn from_str(s: &str) -> Option<Plan> {
        match s {
            "founding" => Some(Plan::Founding),
            "standard" => Some(Plan::Standard),
            "pro" => Some(Plan::Pro),
            _ => None,
        }
    }
    pub fn is_founding(self) -> bool {
        matches!(self, Plan::Founding)
    }
    /// Map a Polar product id to a Plan using the configured product ids.
    pub fn from_polar_product(config: &Config, product_id: &str) -> Option<Plan> {
        if config.polar_product_founding.as_deref() == Some(product_id) {
            Some(Plan::Founding)
        } else if config.polar_product_standard.as_deref() == Some(product_id) {
            Some(Plan::Standard)
        } else if config.polar_product_pro.as_deref() == Some(product_id) {
            Some(Plan::Pro)
        } else {
            None
        }
    }
    /// The Polar product id to use when creating a checkout for this plan.
    pub fn polar_product_id(self, config: &Config) -> Option<String> {
        match self {
            Plan::Founding => config.polar_product_founding.clone(),
            Plan::Standard => config.polar_product_standard.clone(),
            Plan::Pro => config.polar_product_pro.clone(),
        }
    }
}
