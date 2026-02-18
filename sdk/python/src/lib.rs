use pyo3::prelude::*;

mod search;
mod types;
mod util;

use search::PySearch;
use types::{PyEngineError, PySearchOptions, PySearchResponse, PySearchResult};

/// Download Chrome for Testing if not already installed (async version).
///
/// Returns the path to the Chrome executable.
#[pyfunction]
fn ensure_chrome(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async {
        a3s_search::browser_setup::ensure_chrome()
            .await
            .map(|p| p.to_string_lossy().to_string())
            .map_err(util::to_py_error)
    })
}

/// Download Chrome for Testing if not already installed (sync version).
///
/// Blocks until Chrome is ready. Suitable for post-install scripts.
#[pyfunction]
fn ensure_chrome_sync() -> PyResult<String> {
    let rt = tokio::runtime::Runtime::new().map_err(util::to_py_error)?;
    rt.block_on(async {
        a3s_search::browser_setup::ensure_chrome()
            .await
            .map(|p| p.to_string_lossy().to_string())
            .map_err(util::to_py_error)
    })
}

/// Native Python bindings for a3s-search meta search engine.
#[pymodule]
fn _a3s_search(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySearch>()?;
    m.add_class::<PySearchResult>()?;
    m.add_class::<PySearchOptions>()?;
    m.add_class::<PySearchResponse>()?;
    m.add_class::<PyEngineError>()?;
    m.add_function(wrap_pyfunction!(ensure_chrome, m)?)?;
    m.add_function(wrap_pyfunction!(ensure_chrome_sync, m)?)?;
    Ok(())
}
