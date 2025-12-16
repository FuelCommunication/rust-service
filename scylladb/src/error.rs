use scylla::errors::{
    DeserializationError, ExecutionError, IntoRowsResultError, MaybeFirstRowError, NewSessionError, PagerExecutionError,
    PrepareError, RowsError,
};

pub type ScyllaResult<T> = Result<T, ScyllaError>;

#[derive(Debug, thiserror::Error)]
pub enum ScyllaError {
    #[error("Query execution error: {0}")]
    Execution(#[from] ExecutionError),
    #[error("Failed to convert query result into rows: {0}")]
    IntoRowsResult(#[from] IntoRowsResultError),
    #[error("Failed to extract the first row from result: {0}")]
    MaybeFirstRow(#[from] MaybeFirstRowError),
    #[error("Statement preparation error: {0}")]
    Prepare(#[from] PrepareError),
    #[error("Paged query execution error: {0}")]
    PagerExecution(#[from] PagerExecutionError),
    #[error("Failed to create Scylla session: {0}")]
    NewSession(#[from] NewSessionError),
    #[error("Error while accessing row data: {0}")]
    Rows(#[from] RowsError),
    #[error("Failed to deserialize row column value: {0}")]
    Deserialization(#[from] DeserializationError),
}
