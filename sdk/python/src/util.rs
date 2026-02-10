use pyo3::exceptions::PyRuntimeError;
use pyo3::PyErr;

/// Convert any error into a PyErr (RuntimeError).
pub fn to_py_error(err: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}
