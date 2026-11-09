use scylla::errors::{
    DeserializationError, ExecutionError, IntoRowsResultError, MaybeFirstRowError, NewSessionError, PagerExecutionError,
    PrepareError, RowsError,
};

pub type ScyllaResult<T> = Result<T, ScyllaError>;

macro_rules! impl_boxed_from {
    ($err:ty { $($variant:ident <= $src:ty),+ $(,)? }) => {
        $(
            impl From<$src> for $err {
                fn from(e: $src) -> Self {
                    Self::$variant(Box::new(e))
                }
            }
        )+
    };
}

#[derive(Debug, thiserror::Error)]
pub enum ScyllaError {
    #[error("Query execution error: {0}")]
    Execution(Box<ExecutionError>),
    #[error("Failed to convert query result into rows: {0}")]
    IntoRowsResult(Box<IntoRowsResultError>),
    #[error("Failed to extract the first row from result: {0}")]
    MaybeFirstRow(#[from] MaybeFirstRowError),
    #[error("Statement preparation error: {0}")]
    Prepare(#[from] PrepareError),
    #[error("Paged query execution error: {0}")]
    PagerExecution(Box<PagerExecutionError>),
    #[error("Failed to create Scylla session: {0}")]
    NewSession(Box<NewSessionError>),
    #[error("Error while accessing row data: {0}")]
    Rows(#[from] RowsError),
    #[error("Failed to deserialize row column value: {0}")]
    Deserialization(#[from] DeserializationError),
}

impl_boxed_from!(ScyllaError {
    IntoRowsResult <= IntoRowsResultError,
    NewSession <= NewSessionError,
    PagerExecution <= PagerExecutionError,
    Execution <= ExecutionError,
});
