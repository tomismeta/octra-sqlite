/// Public database operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Query,
    Execute,
    ExecuteNoWait,
    AuthInfo,
    ProgramInfo,
}

/// Safety metadata for a database operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSafety {
    pub operation: Operation,
    pub reads_sql: bool,
    pub mutates_state: bool,
    pub submits_transaction: bool,
    pub waits_for_receipt: bool,
    pub requires_signed_rpc: bool,
    pub requires_owner_write_intent: bool,
}

impl Operation {
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
