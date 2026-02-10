use pyo3::prelude::*;

mod search;
mod types;
mod util;

use search::PySearch;
use types::{PyEngineError, PySearchOptions, PySearchResponse, PySearchResult};

/// Native Python bindings for a3s-search meta search engine.
#[pymodule]
fn _a3s_search(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySearch>()?;
    m.add_class::<PySearchResult>()?;
    m.add_class::<PySearchOptions>()?;
    m.add_class::<PySearchResponse>()?;
    m.add_class::<PyEngineError>()?;
    Ok(())
}
