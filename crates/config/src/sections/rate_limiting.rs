// Copyright 2024 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{num::NonZero, time::Duration};

use governor::Quota;
use schemars::JsonSchema;
use serde::{de::Error as _, Deserialize, Serialize};

use crate::ConfigurationSection;

/// Configuration related to sending emails
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RateLimitingConfig {
    /// Login-specific rate limits
    #[serde(default)]
    pub login: LoginRateLimitingConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LoginRateLimitingConfig {
    /// Controls how many login attempts are permitted
    /// based on source address.
    /// This can protect against brute force login attempts.
    ///
    /// Note: this limit also applies to password checks when a user attempts to
    /// change their own password.
    #[serde(default = "default_login_per_address")]
    pub per_address: RateLimiterConfiguration,
    /// Controls how many login attempts are permitted
    /// based on the account that is being attempted to be logged into.
    /// This can protect against a distributed brute force attack
    /// but should be set high enough to prevent someone's account being
    /// casually locked out.
    ///
    /// Note: this limit also applies to password checks when a user attempts to
    /// change their own password.
    #[serde(default = "default_login_per_account")]
    pub per_account: RateLimiterConfiguration,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RateLimiterConfiguration {
    /// A one-off burst of actions that the user can perform
    /// in one go without waiting.
    /// Replenishes at the rate.
    pub burst: u32,
    /// How quickly the allowance replenishes, in number of actions per second.
    /// Can be fractional to replenish slower.
    pub per_second: f64,
}

impl ConfigurationSection for RateLimitingConfig {
    const PATH: Option<&'static str> = Some("rate_limiting");

    fn validate(&self, figment: &figment::Figment) -> Result<(), figment::Error> {
        let metadata = figment.find_metadata(Self::PATH.unwrap());

        let error_on_nested_field =
            |mut error: figment::error::Error, container: &'static str, field: &'static str| {
                error.metadata = metadata.cloned();
                error.profile = Some(figment::Profile::Default);
                error.path = vec![
                    Self::PATH.unwrap().to_owned(),
                    container.to_owned(),
                    field.to_owned(),
                ];
                error
            };

        // Check one limiter's configuration for errors
        let error_on_limiter =
            |limiter: &RateLimiterConfiguration| -> Option<figment::error::Error> {
                if limiter.burst == 0 {
                    return Some(figment::error::Error::custom("`burst` must not be zero, as this would mean the action could never be performed"));
                }

                let recip = limiter.per_second.recip();
                // period must be at least 1 nanosecond according to the governor library
                if recip < 1.0e-9 || !recip.is_finite() {
                    return Some(figment::error::Error::custom(
                        "`per_second` must be a number that is more than zero and less than 1_000_000_000 (1e9)",
                    ));
                }

                None
            };

        if let Some(error) = error_on_limiter(&self.login.per_address) {
            return Err(error_on_nested_field(error, "login", "per_address"));
        }
        if let Some(error) = error_on_limiter(&self.login.per_account) {
            return Err(error_on_nested_field(error, "login", "per_account"));
        }

        Ok(())
    }
}

impl RateLimitingConfig {
    pub(crate) fn is_default(config: &RateLimitingConfig) -> bool {
        config == &RateLimitingConfig::default()
    }
}

impl RateLimiterConfiguration {
    pub fn to_quota(self) -> Option<Quota> {
        let reciprocal = self.per_second.recip();
        if !reciprocal.is_finite() {
            return None;
        }
        let burst = NonZero::new(self.burst)?;
        Some(Quota::with_period(Duration::from_secs_f64(reciprocal))?.allow_burst(burst))
    }
}

fn default_login_per_address() -> RateLimiterConfiguration {
    RateLimiterConfiguration {
        burst: 3,
        per_second: 3.0 / 60.0,
    }
}

fn default_login_per_account() -> RateLimiterConfiguration {
    RateLimiterConfiguration {
        burst: 1800,
        per_second: 1800.0 / 3600.0,
    }
}

#[allow(clippy::derivable_impls)] // when we add some top-level ratelimiters this will not be derivable anymore
impl Default for RateLimitingConfig {
    fn default() -> Self {
        RateLimitingConfig {
            login: LoginRateLimitingConfig::default(),
        }
    }
}

impl Default for LoginRateLimitingConfig {
    fn default() -> Self {
        LoginRateLimitingConfig {
            per_address: default_login_per_address(),
            per_account: default_login_per_account(),
        }
    }
}
