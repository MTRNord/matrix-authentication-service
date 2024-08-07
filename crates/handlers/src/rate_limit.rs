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

use std::{net::IpAddr, sync::Arc, time::Duration};

use governor::{clock::QuantaClock, state::keyed::DashMapStateStore, RateLimiter};
use mas_config::RateLimitingConfig;
use mas_data_model::User;
use ulid::Ulid;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AccountRecoveryLimitedError {
    #[error("Too many account recovery requests for requester {0}")]
    Requester(RequesterFingerprint),

    #[error("Too many account recovery requests for e-mail {0}")]
    Email(String),
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum PasswordCheckLimitedError {
    #[error("Too many password checks for requester {0}")]
    Requester(RequesterFingerprint),

    #[error("Too many password checks for user {0}")]
    User(Ulid),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistrationLimitedError {
    #[error("Too many account registration requests for requester {0}")]
    Requester(RequesterFingerprint),
}

/// Key used to rate limit requests per requester
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequesterFingerprint {
    ip: Option<IpAddr>,
}

impl std::fmt::Display for RequesterFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ip) = self.ip {
            write!(f, "{ip}")
        } else {
            write!(f, "(NO CLIENT IP)")
        }
    }
}

impl RequesterFingerprint {
    /// An anonymous key with no IP address set. This should not be used in
    /// production, and we should warn users if we can't find their client IPs.
    pub const EMPTY: Self = Self { ip: None };

    /// Create a new anonymous key with the given IP address
    #[must_use]
    pub const fn new(ip: IpAddr) -> Self {
        Self { ip: Some(ip) }
    }
}

/// Rate limiters for the different operations
#[derive(Debug, Clone)]
pub struct Limiter {
    inner: Arc<LimiterInner>,
}

type KeyedRateLimiter<K> = RateLimiter<K, DashMapStateStore<K>, QuantaClock>;

#[derive(Debug)]
struct LimiterInner {
    account_recovery_per_requester: KeyedRateLimiter<RequesterFingerprint>,
    account_recovery_per_email: KeyedRateLimiter<String>, // TODO lowercased?
    password_check_for_requester: KeyedRateLimiter<RequesterFingerprint>,
    password_check_for_user: KeyedRateLimiter<Ulid>,
    registration_per_requester: KeyedRateLimiter<RequesterFingerprint>,
}

impl LimiterInner {
    fn new(config: &RateLimitingConfig) -> Option<Self> {
        Some(Self {
            account_recovery_per_requester: RateLimiter::keyed(
                config.account_recovery.per_ip.to_quota()?,
            ),
            account_recovery_per_email: RateLimiter::keyed(
                config.account_recovery.per_address.to_quota()?,
            ),
            password_check_for_requester: RateLimiter::keyed(config.login.per_ip.to_quota()?),
            password_check_for_user: RateLimiter::keyed(config.login.per_account.to_quota()?),
            registration_per_requester: RateLimiter::keyed(config.registration.to_quota()?),
        })
    }
}

impl Limiter {
    /// Creates a new `Limiter` based on a `RateLimitingConfig`.
    ///
    /// If the config is not valid, returns `None`.
    /// (This should not happen if the config was validated, though.)
    #[must_use]
    pub fn new(config: &RateLimitingConfig) -> Option<Self> {
        Some(Self {
            inner: Arc::new(LimiterInner::new(config)?),
        })
    }

    /// Start the rate limiter housekeeping task
    ///
    /// This task will periodically remove old entries from the rate limiters,
    /// to make sure we don't build up a huge number of entries in memory.
    pub fn start(&self) {
        // Spawn a task that will periodically clean the rate limiters
        let this = self.clone();
        tokio::spawn(async move {
            // Run the task every minute
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                // Call the retain_recent method on each rate limiter
                this.inner.account_recovery_per_email.retain_recent();
                this.inner.account_recovery_per_requester.retain_recent();
                this.inner.password_check_for_requester.retain_recent();
                this.inner.password_check_for_user.retain_recent();
                this.inner.registration_per_requester.retain_recent();

                interval.tick().await;
            }
        });
    }

    /// Check if an account recovery can be performed
    ///
    /// # Errors
    ///
    /// Returns an error if the operation is rate limited.
    pub fn check_account_recovery(
        &self,
        requester: RequesterFingerprint,
        email_address: &str,
    ) -> Result<(), AccountRecoveryLimitedError> {
        self.inner
            .account_recovery_per_requester
            .check_key(&requester)
            .map_err(|_| AccountRecoveryLimitedError::Requester(requester))?;

        // Convert to lowercase to prevent bypassing the limit by enumerating different
        // case variations.
        // A case-folding transformation may be more proper.
        let canonical_email = email_address.to_lowercase();
        self.inner
            .account_recovery_per_email
            .check_key(&canonical_email)
            .map_err(|_| AccountRecoveryLimitedError::Email(canonical_email))?;

        Ok(())
    }

    /// Check if a password check can be performed
    ///
    /// # Errors
    ///
    /// Returns an error if the operation is rate limited
    pub fn check_password(
        &self,
        key: RequesterFingerprint,
        user: &User,
    ) -> Result<(), PasswordCheckLimitedError> {
        self.inner
            .password_check_for_requester
            .check_key(&key)
            .map_err(|_| PasswordCheckLimitedError::Requester(key))?;

        self.inner
            .password_check_for_user
            .check_key(&user.id)
            .map_err(|_| PasswordCheckLimitedError::User(user.id))?;

        Ok(())
    }

    /// Check if an account registration can be performed
    ///
    /// # Errors
    ///
    /// Returns an error if the operation is rate limited.
    pub fn check_registration(
        &self,
        requester: RequesterFingerprint,
    ) -> Result<(), RegistrationLimitedError> {
        self.inner
            .registration_per_requester
            .check_key(&requester)
            .map_err(|_| RegistrationLimitedError::Requester(requester))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mas_data_model::User;
    use mas_storage::{clock::MockClock, Clock};
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn test_password_check_limiter() {
        let now = MockClock::default().now();
        let mut rng = rand_chacha::ChaChaRng::seed_from_u64(42);

        let limiter = Limiter::new(&RateLimitingConfig::default()).unwrap();

        // Let's create a lot of requesters to test account-level rate limiting
        let requesters: [_; 768] = (0..=255)
            .flat_map(|a| (0..3).map(move |b| RequesterFingerprint::new([a, a, b, b].into())))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let alice = User {
            id: Ulid::from_datetime_with_source(now.into(), &mut rng),
            username: "alice".to_owned(),
            sub: "123-456".to_owned(),
            primary_user_email_id: None,
            created_at: now,
            locked_at: None,
            can_request_admin: false,
        };

        let bob = User {
            id: Ulid::from_datetime_with_source(now.into(), &mut rng),
            username: "bob".to_owned(),
            sub: "123-456".to_owned(),
            primary_user_email_id: None,
            created_at: now,
            locked_at: None,
            can_request_admin: false,
        };

        // Three times the same IP address should be allowed
        assert!(limiter.check_password(requesters[0], &alice).is_ok());
        assert!(limiter.check_password(requesters[0], &alice).is_ok());
        assert!(limiter.check_password(requesters[0], &alice).is_ok());

        // But the fourth time should be rejected
        assert!(limiter.check_password(requesters[0], &alice).is_err());
        // Using another user should also be rejected
        assert!(limiter.check_password(requesters[0], &bob).is_err());

        // Using a different IP address should be allowed, the account isn't locked yet
        assert!(limiter.check_password(requesters[1], &alice).is_ok());

        // At this point, we consumed 4 cells out of 1800 on alice, let's distribute the
        // requests with other IPs so that we get rate-limited on the account-level
        for requester in requesters.iter().skip(2).take(598) {
            assert!(limiter.check_password(*requester, &alice).is_ok());
            assert!(limiter.check_password(*requester, &alice).is_ok());
            assert!(limiter.check_password(*requester, &alice).is_ok());
            assert!(limiter.check_password(*requester, &alice).is_err());
        }

        // We now have consumed 4+598*3 = 1798 cells on the account, so we should be
        // rejected soon
        assert!(limiter.check_password(requesters[600], &alice).is_ok());
        assert!(limiter.check_password(requesters[601], &alice).is_ok());
        assert!(limiter.check_password(requesters[602], &alice).is_err());

        // The other account isn't rate-limited
        assert!(limiter.check_password(requesters[603], &bob).is_ok());
    }
}
