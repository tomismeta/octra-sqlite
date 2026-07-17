/// Public database operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Read-only SQL query.
    Query,
    /// Owner-signed write with receipt confirmation.
    Execute,
    /// Owner-signed write without receipt confirmation.
    ExecuteNoWait,
    /// Owner-write authorization inspection.
    AuthInfo,
    /// Deployed Circle program inspection.
    ProgramInfo,
}

/// Safety metadata for a database operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSafety {
    /// Operation described by this metadata.
    pub operation: Operation,
    /// Whether the operation carries SQL text.
    pub reads_sql: bool,
    /// Whether the operation can mutate Circle state.
    pub mutates_state: bool,
    /// Whether the operation submits an Octra transaction.
    pub submits_transaction: bool,
    /// Whether the operation waits for a confirmed receipt.
    pub waits_for_receipt: bool,
    /// Whether signed Octra RPC may be required by the target read mode.
    ///
    /// This metadata is target-independent and conservative: public reads may
    /// use unsigned Circle views even when this field is `true`.
    pub requires_signed_rpc: bool,
    /// Whether the operation requires OSW1 owner-write intent.
    pub requires_owner_write_intent: bool,
}

impl Operation {
    /// Return target-independent, conservative safety properties.
    pub fn safety(self) -> OperationSafety {
        match self {
            Operation::Query => OperationSafety {
                operation: self,
                reads_sql: true,
                mutates_state: false,
                submits_transaction: false,
                waits_for_receipt: false,
                requires_signed_rpc: true,
                requires_owner_write_intent: false,
            },
            Operation::Execute => OperationSafety {
                operation: self,
                reads_sql: true,
                mutates_state: true,
                submits_transaction: true,
                waits_for_receipt: true,
                requires_signed_rpc: true,
                requires_owner_write_intent: true,
            },
            Operation::ExecuteNoWait => OperationSafety {
                operation: self,
                reads_sql: true,
                mutates_state: true,
                submits_transaction: true,
                waits_for_receipt: false,
                requires_signed_rpc: true,
                requires_owner_write_intent: true,
            },
            Operation::AuthInfo | Operation::ProgramInfo => OperationSafety {
                operation: self,
                reads_sql: false,
                mutates_state: false,
                submits_transaction: false,
                waits_for_receipt: false,
                requires_signed_rpc: true,
                requires_owner_write_intent: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_method_marks_execute_as_mutating_transaction() {
        let safety = Operation::Execute.safety();
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }

    #[test]
    fn operation_method_marks_no_wait_without_receipt_wait() {
        let safety = Operation::ExecuteNoWait.safety();
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(!safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }
}
