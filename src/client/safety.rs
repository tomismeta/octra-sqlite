#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseOperation {
    Query,
    Execute,
    ExecuteNoWait,
    AuthInfo,
    ProgramInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSafety {
    pub operation: DatabaseOperation,
    pub reads_sql: bool,
    pub mutates_state: bool,
    pub submits_transaction: bool,
    pub waits_for_receipt: bool,
    pub requires_signed_rpc: bool,
    pub requires_owner_write_intent: bool,
}

pub fn operation_safety(operation: DatabaseOperation) -> OperationSafety {
    match operation {
        DatabaseOperation::Query => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: false,
            submits_transaction: false,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: false,
        },
        DatabaseOperation::Execute => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: true,
            submits_transaction: true,
            waits_for_receipt: true,
            requires_signed_rpc: true,
            requires_owner_write_intent: true,
        },
        DatabaseOperation::ExecuteNoWait => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: true,
            submits_transaction: true,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: true,
        },
        DatabaseOperation::AuthInfo | DatabaseOperation::ProgramInfo => OperationSafety {
            operation,
            reads_sql: false,
            mutates_state: false,
            submits_transaction: false,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_safety_marks_execute_as_mutating_transaction() {
        let safety = operation_safety(DatabaseOperation::Execute);
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }

    #[test]
    fn operation_safety_marks_no_wait_without_receipt_wait() {
        let safety = operation_safety(DatabaseOperation::ExecuteNoWait);
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(!safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }
}
