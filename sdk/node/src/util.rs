use napi::Error as NapiError;

/// Convert a search error into a napi::Error.
pub fn to_napi_error(err: impl std::fmt::Display) -> NapiError {
    NapiError::from_reason(err.to_string())
}
