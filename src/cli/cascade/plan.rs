//! CLI clap glue for the library cascade plan.

use a3s_search::EngineTier;

/// Clap-facing tier token. Converts to the library [`EngineTier`].
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CliEngineTier {
    Headless,
    HttpRss,
    Api,
}

impl From<CliEngineTier> for EngineTier {
    fn from(value: CliEngineTier) -> Self {
        match value {
            CliEngineTier::Headless => Self::Headless,
            CliEngineTier::HttpRss => Self::HttpRss,
            CliEngineTier::Api => Self::Api,
        }
    }
}

impl From<EngineTier> for CliEngineTier {
    fn from(value: EngineTier) -> Self {
        match value {
            EngineTier::Headless => Self::Headless,
            EngineTier::HttpRss => Self::HttpRss,
            EngineTier::Api => Self::Api,
        }
    }
}
