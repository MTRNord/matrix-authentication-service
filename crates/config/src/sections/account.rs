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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ConfigurationSection;

const fn default_true() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_default_true(value: &bool) -> bool {
    *value == default_true()
}

const fn default_false() -> bool {
    false
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_default_false(value: &bool) -> bool {
    *value == default_false()
}

/// Configuration section to configure features related to account management
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct AccountConfig {
    /// Whether users are allowed to change their email addresses. Defaults to
    /// `true`.
    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub email_change_allowed: bool,

    /// Whether users are allowed to change their display names. Defaults to
    /// `true`.
    ///
    /// This should be in sync with the policy in the homeserver configuration.
    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub displayname_change_allowed: bool,

    /// Whether to enable self-service password registration. Defaults to
    /// `false` if password authentication is enabled.
    ///
    /// This has no effect if password login is disabled.
    #[serde(default = "default_false", skip_serializing_if = "is_default_false")]
    pub password_registration_enabled: bool,

    /// Whether users are allowed to change their passwords. Defaults to `true`.
    ///
    /// This has no effect if password login is disabled.
    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub password_change_allowed: bool,

    /// Whether email-based password recovery is enabled. Defaults to `false`.
    ///
    /// This has no effect if password login is disabled.
    #[serde(default = "default_false", skip_serializing_if = "is_default_false")]
    pub password_recovery_enabled: bool,
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self {
            email_change_allowed: default_true(),
            displayname_change_allowed: default_true(),
            password_registration_enabled: default_false(),
            password_change_allowed: default_true(),
            password_recovery_enabled: default_false(),
        }
    }
}

impl AccountConfig {
    /// Returns true if the configuration is the default one
    pub(crate) fn is_default(&self) -> bool {
        is_default_false(&self.password_registration_enabled)
            && is_default_true(&self.email_change_allowed)
            && is_default_true(&self.displayname_change_allowed)
            && is_default_true(&self.password_change_allowed)
            && is_default_false(&self.password_recovery_enabled)
    }
}

impl ConfigurationSection for AccountConfig {
    const PATH: Option<&'static str> = Some("account");
}
