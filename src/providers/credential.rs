//! Redacted credential and provider readiness handling.

use std::env;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{ProviderError, ProviderErrorKind, Result};

/// How a provider request will authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderAuthentication {
    /// The provider explicitly supports requests without credentials.
    Anonymous,
    /// A configured credential is available.
    Authenticated,
}

/// Whether a provider can accept requests with the current configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderReadiness {
    /// The provider is ready for requests.
    Ready {
        /// Authentication mode that will be used.
        authentication: ProviderAuthentication,
    },
    /// A required credential is absent.
    MissingCredential {
        /// Environment variable expected by the configured source.
        #[serde(skip_serializing_if = "Option::is_none")]
        environment_variable: Option<String>,
    },
    /// A credential exists but cannot be represented safely.
    InvalidCredential,
}

impl ProviderReadiness {
    /// Returns whether a provider is ready to accept a request.
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

#[derive(Clone)]
pub(crate) struct SecretString(Arc<str>);

impl SecretString {
    pub(crate) fn new(value: String) -> Option<Self> {
        let value = value.trim().to_string();
        (!value.is_empty()).then(|| Self(Arc::from(value)))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Clone)]
enum CredentialSourceKind {
    None,
    Environment(String),
    InvalidEnvironment,
    Value(SecretString),
}

/// A redacted source for API credentials and credential-like headers.
///
/// `Debug` output never includes static values or resolved environment values.
#[derive(Clone)]
pub struct CredentialSource {
    inner: CredentialSourceKind,
}

impl CredentialSource {
    /// Creates an explicitly absent credential source.
    pub const fn none() -> Self {
        Self {
            inner: CredentialSourceKind::None,
        }
    }

    /// Reads a credential from an environment variable at request time.
    pub fn environment(variable: impl Into<String>) -> Self {
        let variable = variable.into();
        if !valid_environment_variable(&variable) {
            return Self {
                inner: CredentialSourceKind::InvalidEnvironment,
            };
        }
        Self {
            inner: CredentialSourceKind::Environment(variable),
        }
    }

    /// Stores a credential value in redacted memory.
    ///
    /// Empty and whitespace-only values are treated as absent.
    pub fn value(value: impl Into<String>) -> Self {
        let inner = SecretString::new(value.into())
            .map(CredentialSourceKind::Value)
            .unwrap_or(CredentialSourceKind::None);
        Self { inner }
    }

    /// Returns the configured environment variable name, if any.
    pub fn environment_variable(&self) -> Option<&str> {
        match &self.inner {
            CredentialSourceKind::Environment(variable) => Some(variable),
            _ => None,
        }
    }

    pub(crate) fn resolve(&self, provider: &str) -> Result<Option<SecretString>> {
        match &self.inner {
            CredentialSourceKind::None => Ok(None),
            CredentialSourceKind::Value(value) => Ok(Some(value.clone())),
            CredentialSourceKind::InvalidEnvironment => Err(ProviderError::new(
                provider,
                ProviderErrorKind::Authentication,
                "credential environment variable name is invalid",
            )
            .into()),
            CredentialSourceKind::Environment(variable) => match env::var_os(variable) {
                None => Ok(None),
                Some(value) => {
                    let value = value.into_string().map_err(|_| {
                        ProviderError::new(
                            provider,
                            ProviderErrorKind::Authentication,
                            format!(
                                "credential environment variable {variable} is not valid UTF-8"
                            ),
                        )
                    })?;
                    Ok(SecretString::new(value))
                }
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn readiness(&self, provider: &str, anonymous_allowed: bool) -> ProviderReadiness {
        match self.resolve(provider) {
            Ok(Some(_)) => ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Authenticated,
            },
            Ok(None) if anonymous_allowed => ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Anonymous,
            },
            Ok(None) => ProviderReadiness::MissingCredential {
                environment_variable: self.environment_variable().map(str::to_string),
            },
            Err(_) => ProviderReadiness::InvalidCredential,
        }
    }
}

impl Default for CredentialSource {
    fn default() -> Self {
        Self::none()
    }
}

impl fmt::Debug for CredentialSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            CredentialSourceKind::None => formatter.write_str("CredentialSource::None"),
            CredentialSourceKind::Environment(variable) => formatter
                .debug_tuple("CredentialSource::Environment")
                .field(variable)
                .finish(),
            CredentialSourceKind::InvalidEnvironment => {
                formatter.write_str("CredentialSource::InvalidEnvironment")
            }
            CredentialSourceKind::Value(_) => {
                formatter.write_str("CredentialSource::Value([REDACTED])")
            }
        }
    }
}

fn valid_environment_variable(variable: &str) -> bool {
    let mut characters = variable.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_credentials_are_redacted() {
        let source = CredentialSource::value("super-secret");
        let debug = format!("{source:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret"));
        assert_eq!(
            source.resolve("test").unwrap().unwrap().expose(),
            "super-secret"
        );
    }

    #[test]
    fn missing_environment_supports_anonymous_readiness() {
        let source =
            CredentialSource::environment("A3S_SEARCH_TEST_CREDENTIAL_THAT_MUST_NOT_EXIST");

        assert_eq!(
            source.readiness("test", true),
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Anonymous,
            }
        );
    }

    #[test]
    fn missing_required_environment_is_reported_without_a_value() {
        let variable = "A3S_SEARCH_TEST_REQUIRED_CREDENTIAL_THAT_MUST_NOT_EXIST";
        let source = CredentialSource::environment(variable);

        assert_eq!(
            source.readiness("test", false),
            ProviderReadiness::MissingCredential {
                environment_variable: Some(variable.to_string()),
            }
        );
    }

    #[test]
    fn invalid_environment_name_is_typed_instead_of_panicking() {
        let source = CredentialSource::environment("INVALID=ENVIRONMENT");

        assert_eq!(
            source.readiness("test", true),
            ProviderReadiness::InvalidCredential
        );
        assert_eq!(
            source.resolve("test").unwrap_err().kind(),
            "provider_authentication"
        );
        assert!(!format!("{source:?}").contains("INVALID=ENVIRONMENT"));
    }
}
