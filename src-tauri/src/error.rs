/// Error returned to the frontend as `{ kind, message }`; the UI shows a Turkish text per kind.
pub use gamelib_core::ErrorInfo as CmdError;

pub type CmdResult<T> = Result<T, CmdError>;
