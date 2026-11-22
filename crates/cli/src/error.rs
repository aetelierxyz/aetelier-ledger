use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Ok,
    Usage,
    Refused,
    Auth,
    Rejected,
    Pending,
    Transient,
}

impl Exit {
    pub fn code(self) -> u8 {
        match self {
            Exit::Ok => 0,
            Exit::Usage => 1,
            Exit::Refused => 2,
            Exit::Auth => 3,
            Exit::Rejected => 4,
            Exit::Pending => 5,
            Exit::Transient => 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}: {}", body.code, body.message)]
pub struct CliError {
    pub exit: Exit,
    pub body: ErrorBody,
}

impl CliError {
    pub fn new(exit: Exit, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            exit,
            body: ErrorBody {
                code: code.into(),
                message: message.into(),
                http_status: None,
                memo: None,
            },
        }
    }

    pub fn usage(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Exit::Usage, code, message)
    }

    pub fn refused(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Exit::Refused, code, message)
    }

    pub fn rejected(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Exit::Rejected, code, message)
    }

    pub fn transient(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Exit::Transient, code, message)
    }

    pub fn pending(memo: &str, message: impl Into<String>) -> Self {
        Self::new(Exit::Pending, "pending", message).with_memo(memo)
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.body.http_status = Some(status);
        self
    }

    pub fn with_memo(mut self, memo: &str) -> Self {
        if self.body.memo.is_none() {
            self.body.memo = Some(memo.to_string());
        }
        self
    }

    pub fn code(&self) -> &str {
        &self.body.code
    }

    pub fn to_json(&self) -> Value {
        json!({ "error": self.body })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub exit: Exit,
    pub body: Value,
}

impl Outcome {
    pub fn ok(body: Value) -> Self {
        Self {
            exit: Exit::Ok,
            body,
        }
    }

    pub fn with_exit(exit: Exit, body: Value) -> Self {
        Self { exit, body }
    }

    pub fn from_error(error: CliError) -> Self {
        Self {
            exit: error.exit,
            body: error.to_json(),
        }
    }
}

impl From<Result<Value, CliError>> for Outcome {
    fn from(result: Result<Value, CliError>) -> Self {
        match result {
            Ok(body) => Outcome::ok(body),
            Err(e) => Outcome::from_error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_documented_table() {
        let table = [
            (Exit::Ok, 0),
            (Exit::Usage, 1),
            (Exit::Refused, 2),
            (Exit::Auth, 3),
            (Exit::Rejected, 4),
            (Exit::Pending, 5),
            (Exit::Transient, 6),
        ];
        for (exit, code) in table {
            assert_eq!(exit.code(), code);
        }
    }

    #[test]
    fn error_json_carries_status_and_memo_only_when_known() {
        let bare = CliError::usage("missing_api_key", "set it");
        assert_eq!(
            bare.to_json(),
            json!({"error": {"code": "missing_api_key", "message": "set it"}})
        );
        let full = CliError::rejected("intent_failed", "rejected")
            .with_status(409)
            .with_memo("m1")
            .with_memo("m2");
        assert_eq!(
            full.to_json(),
            json!({"error": {"code": "intent_failed", "message": "rejected", "http_status": 409, "memo": "m1"}})
        );
        let outcome = Outcome::from(Err::<Value, _>(full));
        assert_eq!(outcome.exit, Exit::Rejected);
    }
}
